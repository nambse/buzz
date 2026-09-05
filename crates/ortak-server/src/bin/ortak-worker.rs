#![deny(unsafe_code)]
//! Explicit opt-in private worker: Office inbox → pinned runtime → durable events.

use std::time::Duration;

use ortak_control::runtime::{RuntimeAdapter, RuntimeCapability, ACTIVATION_REQUIRED_CAPABILITIES};
use ortak_control::{
    ports::CompanyDirectory,
    service::{InboxRoutingService, RoutingWorkerConfig},
    PgControlPlane,
};
use ortak_office::transport::{
    EnvOfficeSigner, HttpOfficePublisher, OfficeRelayBinding, OfficeSignerBinding,
};
use ortak_office::{DeliveryConfig, OfficeDeliveryService, PgChannelNormalizer};
use ortak_runtime::{
    hermes::HermesAdapter, office_output::schedule_office_outputs,
    reconciliation::reconcile_runtime, RunSupervisor, SupervisorConfig,
};
use ortak_server::shutdown::{Outcome, Shutdown};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

#[path = "../worker_memory.rs"]
mod worker_memory;
use worker_memory::{MemoryConfig, WorkerMemory};

#[path = "../worker_semantic.rs"]
mod worker_semantic;
use worker_semantic::WorkerSemantic;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    company_slug: String,
    bridge_origin: String,
    #[serde(default)]
    memory: Option<MemoryConfig>,
    #[serde(default)]
    semantic: Option<serde_json::Value>,
    #[serde(default)]
    office_signers: Vec<OfficeSignerBinding>,
    #[serde(default)]
    office_relays: Vec<OfficeRelayBinding>,
    #[serde(default = "poll_interval")]
    poll_interval_ms: u64,
    #[serde(default = "batch_limit")]
    batch_limit: usize,
}
fn poll_interval() -> u64 {
    1000
}
fn batch_limit() -> usize {
    8
}

#[tokio::main]
async fn main() {
    // Watch the entire startup/cycle future, including in-flight stop replay,
    // routing, external start, memory and delivery. A signal never waits for the
    // current batch to finish and never implies that a remote run has stopped.
    let result = match Shutdown::install() {
        Ok(mut shutdown) => match shutdown.until(run()).await {
            Ok(Outcome::Completed(result)) => result,
            Ok(Outcome::Interrupted) => {
                eprintln!("ortak-worker: local shutdown; durable work remains recoverable; remote execution is unchanged");
                Ok(())
            }
            Err(_) => Err("shutdown signal failed"),
        },
        Err(_) => Err("shutdown signal registration failed"),
    };
    if let Err(code) = result {
        eprintln!("ortak-worker: {code}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    if std::env::var("ORTAK_WORKER_ENABLED").as_deref() != Ok("true") {
        return Err("explicit ORTAK_WORKER_ENABLED=true is required");
    }
    let config =
        std::env::var("ORTAK_WORKER_CONFIG_JSON").map_err(|_| "worker configuration required")?;
    if config.len() > 16_384 {
        return Err("worker configuration exceeds limit");
    }
    let config: Config =
        serde_json::from_str(&config).map_err(|_| "invalid worker configuration")?;
    if config.company_slug.is_empty()
        || config.company_slug.len() > 128
        || !(100..=300_000).contains(&config.poll_interval_ms)
        || !(1..=32).contains(&config.batch_limit)
    {
        return Err("invalid worker configuration bounds");
    }
    let database = std::env::var("ORTAK_DATABASE_URL").map_err(|_| "database URL required")?;
    let pool = ortak_server::connect_worker_database(&database)
        .await
        .map_err(|_| "database connection failed")?;
    let control = PgControlPlane::new(pool.clone());
    // Slug resolution also works after company suspension or Office unbinding:
    // restart must still discover and cancel previously admitted work.
    let scope = control
        .resolve_company_by_slug(&config.company_slug)
        .await
        .map_err(|_| "company resolution failed")?;
    let delivery = (|| {
        if config.office_signers.is_empty()
            || config.office_relays.len() != 1
            || config
                .office_signers
                .iter()
                .any(|binding| binding.company_id != scope.company_id())
            || config.office_relays[0].company_id != scope.company_id()
        {
            return Err("Office bindings must select this worker's company");
        }
        let signer = EnvOfficeSigner::from_env(config.office_signers)
            .map_err(|_| "Office signer configuration failed")?;
        let publisher = HttpOfficePublisher::new(
            control.clone(),
            signer.clone(),
            config.office_relays,
            Duration::from_secs(20),
        )
        .map_err(|_| "Office publisher configuration failed")?;
        Ok(OfficeDeliveryService::new(
            control.clone(),
            signer,
            publisher,
            DeliveryConfig::default(),
        ))
    })();
    if delivery.is_err() {
        eprintln!("ortak-worker: Office delivery unavailable; new work is paused");
    }
    let delivery = delivery.ok();
    let token = std::env::var("ORTAK_HERMES_BRIDGE_TOKEN").map_err(|_| "bridge token required")?;
    let adapter = HermesAdapter::new(scope.company_id(), &config.bridge_origin, &token)
        .map_err(|_| "invalid bridge connection")?;
    let capabilities = adapter
        .probe_capabilities()
        .await
        .map_err(|_| "bridge capabilities unavailable")?;
    let can_start = capabilities
        .missing(&ACTIVATION_REQUIRED_CAPABILITIES)
        .is_empty();
    if !capabilities
        .missing(&[
            RuntimeCapability::RunCancelStart,
            RuntimeCapability::RunLookup,
            RuntimeCapability::RunEvents,
        ])
        .is_empty()
    {
        return Err("bridge lacks required durable recovery capabilities");
    }
    if !can_start {
        eprintln!("ortak-worker: bridge start capabilities unavailable; recovery only");
    }
    let supervisor_config = SupervisorConfig::default();
    if config.memory.is_none() {
        eprintln!("ortak-worker: memory configuration unavailable; new work is paused");
    }
    let memory = config
        .memory
        .and_then(|config| match WorkerMemory::new(&scope, config) {
            Ok(memory) => Some(memory),
            Err(code) => {
                eprintln!("ortak-worker: {code}; new work is paused");
                None
            }
        })
        .unwrap_or_else(WorkerMemory::disabled);
    let supervisor =
        RunSupervisor::new(control.clone(), adapter.clone(), supervisor_config.clone())
            .with_memory(&memory);
    let routing_config = RoutingWorkerConfig::default();
    let worker_id = routing_config.worker_id.clone();
    let router = InboxRoutingService::new(
        control.clone(),
        PgChannelNormalizer::new(pool.clone()),
        WorkerSemantic::new(&scope, config.semantic),
        routing_config,
    );
    let mut after_run = None::<Uuid>;
    let mut interval = tokio::time::interval(Duration::from_millis(config.poll_interval_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        // Failures propagate to process supervision; all work remains in its
        // durable inbox/outbox/stop queue with leases and bounded retry budgets.
        reconcile_runtime(
            &control,
            &adapter,
            &scope,
            &supervisor_config,
            config.batch_limit,
        )
        .await
        .map_err(|_| "runtime reconciliation failed")?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM companies c
               JOIN office_company_bindings b ON b.company_id=c.id
               JOIN communities cm ON cm.id=b.community_id
               WHERE c.id=$1 AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL)",
        ).bind(scope.company_id()).fetch_one(&pool).await.map_err(|_| "company state read failed")?;
        if active {
            if let Some(ready) = memory.refresh_one().await {
                eprintln!(
                    "ortak-worker: memory roundtrip {}",
                    if ready {
                        "validated"
                    } else {
                        "unavailable; new work is paused"
                    }
                );
            }
        }
        if active && can_start && delivery.is_some() && memory.ready() {
            router
                .claim_and_route(&scope)
                .await
                .map_err(|_| "routing attempt failed")?;
            // One external start per cycle keeps cancellation ahead of a
            // backlog; the durable queue remains available to other workers.
            let mut leases = control
                .claim_runtime_dispatches(
                    &scope,
                    adapter.adapter_name(),
                    &worker_id,
                    Duration::from_secs(60),
                    1,
                )
                .await
                .map_err(|_| "dispatch claim failed")?;
            if let Some(lease) = leases.pop() {
                supervisor
                    .dispatch(&scope, &lease)
                    .await
                    .map_err(|_| "dispatch attempt failed")?;
            }
        }
        let runs = sqlx::query(
            "SELECT id FROM runs r WHERE company_id=$1 AND runtime_adapter=$2
               AND status IN ('queued','running','waiting') AND runtime_run_ref IS NOT NULL
               AND ($3::uuid IS NULL OR id>$3)
               AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
               ORDER BY id LIMIT $4",
        ).bind(scope.company_id()).bind(adapter.adapter_name()).bind(after_run)
            .bind(1_i64).fetch_all(&pool).await.map_err(|_| "run cursor scan failed")?;
        if runs.is_empty() {
            after_run = None;
        }
        for row in runs {
            let run_id: Uuid = row.try_get("id").map_err(|_| "invalid run cursor")?;
            supervisor
                .pump(&scope, run_id)
                .await
                .map_err(|_| "runtime event read failed")?;
            after_run = Some(run_id);
        }
        schedule_office_outputs(&control, &scope, config.batch_limit)
            .await
            .map_err(|_| "Office completion scheduling failed")?;
        if memory.ready() {
            ortak_runtime::memory_output::schedule_memory_output(&control, &memory, &scope)
                .await
                .map_err(|_| "memory output scheduling failed")?;
        }
        if let Some(delivery) = &delivery {
            ortak_runtime::office_delivery::deliver_one_office_output(
                &control, &scope, &worker_id, delivery,
            )
            .await
            .map_err(|_| "Office delivery scheduling failed")?;
        }
    }
}
