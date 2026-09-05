//! Shared Postgres fixture for the channel-normalization seam tests.
//!
//! Builds one company bound to one community with one channel, a human
//! channel member, and the Cem/Zeynep fixtures activated with generated
//! Office keys. Events are stored the way the relay stores them (signed
//! `events` rows plus optional `thread_metadata`) and handed to the control
//! plane through the production `insert_accepted_event` path.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_control::inbox::InboxEvent;
use ortak_control::ports::{CompanyDirectory, InboxRepository};
use ortak_control::{
    CompanyScope, DisabledSemanticScorer, InboxRoutingService, MessageId, PgControlPlane,
    RoutingWorkerConfig, ServiceOutcome,
};
use ortak_domain::{Employee, EmployeeManifest, EmployeeStatus, RoutingPolicy};
use ortak_office::PgChannelNormalizer;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub const KIND_STREAM_MESSAGE: i32 = 9;
pub const KIND_STREAM_MESSAGE_V2: i32 = 40002;
pub const KIND_GIFT_WRAP: i32 = 1059;

/// The disposable database these tests may migrate and populate. Only the
/// explicit Ortak variable is honoured: a generic `DATABASE_URL` or a
/// desktop-relay default could point at a shared or production database.
fn database_url() -> String {
    std::env::var("ORTAK_TEST_DATABASE_URL").expect(
        "ORTAK_TEST_DATABASE_URL must name a disposable Postgres database; \
         DATABASE_URL and other generic variables are deliberately ignored",
    )
}

async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    buzz_db::migration::run_migrations(&pool)
        .await
        .expect("apply migrations");
    pool
}

/// A fresh 32-byte Office key that is unrelated to any real identity.
pub fn generated_key() -> [u8; 32] {
    nostr::Keys::generate().public_key().to_bytes()
}

pub fn message_id() -> MessageId {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    MessageId::from_bytes(bytes)
}

fn fixture_employee(name: &str, key: [u8; 32]) -> Employee {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
    let mut employee = manifest.employee;
    employee.office.public_key = hex::encode(key);
    employee
}

/// One stored event as the relay would persist it.
pub struct StoredEvent {
    pub id: MessageId,
    pub created_at: DateTime<Utc>,
}

/// Everything a stored event needs; defaults describe a plain channel text.
pub struct EventSpec<'a> {
    pub kind: i32,
    pub author: [u8; 32],
    pub content: &'a str,
    pub tags: serde_json::Value,
    pub channel_id: Option<Uuid>,
    /// Relay-persisted reply parent (`thread_metadata`), if any.
    pub parent: Option<&'a StoredEvent>,
}

pub struct Fixture {
    pub pool: PgPool,
    pub control: PgControlPlane,
    pub community_id: Uuid,
    pub scope: CompanyScope,
    pub channel_id: Uuid,
    pub human_key: [u8; 32],
    pub cem_key: [u8; 32],
    pub zeynep_key: [u8; 32],
    pub cem_revision: Uuid,
}

impl Fixture {
    pub async fn new() -> Self {
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let community_id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(community_id)
            .bind(format!(
                "ortak-normalizer-{}.example",
                community_id.simple()
            ))
            .execute(&pool)
            .await
            .expect("insert community");
        let company_id: Uuid = sqlx::query(
            "INSERT INTO companies (slug, display_name, routing_policy)
             VALUES ($1, 'Ortak normalizer test', $2) RETURNING id",
        )
        .bind(format!("co-{}", Uuid::new_v4().simple()))
        .bind(serde_json::to_value(RoutingPolicy::default()).expect("policy"))
        .fetch_one(&pool)
        .await
        .expect("insert company")
        .try_get("id")
        .expect("company id");
        sqlx::query(
            "INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)",
        )
        .bind(community_id)
        .bind(company_id)
        .execute(&pool)
        .await
        .expect("insert binding");

        let human_key = generated_key();
        let channel_id = create_channel(&pool, community_id, &human_key).await;
        add_channel_member(&pool, community_id, channel_id, &human_key).await;

        let cem_key = generated_key();
        let zeynep_key = generated_key();
        let cem_revision =
            activate_employee(&pool, company_id, &fixture_employee("cem", cem_key)).await;
        activate_employee(&pool, company_id, &fixture_employee("zeynep", zeynep_key)).await;
        // Both employees' verified keys are live members of the channel; the
        // normalizer derives conversation eligibility from this membership.
        add_channel_member(&pool, community_id, channel_id, &cem_key).await;
        add_channel_member(&pool, community_id, channel_id, &zeynep_key).await;

        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve scope");
        Self {
            pool,
            control,
            community_id,
            scope,
            channel_id,
            human_key,
            cem_key,
            zeynep_key,
            cem_revision,
        }
    }

    pub fn company_id(&self) -> Uuid {
        self.scope.company_id()
    }

    pub fn service(
        &self,
    ) -> InboxRoutingService<PgControlPlane, PgChannelNormalizer, DisabledSemanticScorer> {
        InboxRoutingService::new(
            self.control.clone(),
            PgChannelNormalizer::new(self.pool.clone()),
            DisabledSemanticScorer::new(),
            RoutingWorkerConfig {
                worker_id: "normalizer-test".to_owned(),
                retry_backoff: Duration::ZERO,
                ..RoutingWorkerConfig::default()
            },
        )
    }

    /// Stores the signed event row (and its thread metadata when it is a
    /// reply) exactly as the relay would, without an inbox row.
    pub async fn store_event(&self, spec: EventSpec<'_>) -> StoredEvent {
        let id = message_id();
        let created_at = Utc::now();
        sqlx::query(
            "INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig, channel_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(self.community_id)
        .bind(id.as_bytes().as_slice())
        .bind(spec.author.as_slice())
        .bind(created_at)
        .bind(spec.kind)
        .bind(&spec.tags)
        .bind(spec.content)
        .bind([9u8; 64].as_slice())
        .bind(spec.channel_id)
        .execute(&self.pool)
        .await
        .expect("insert event");
        if let (Some(parent), Some(channel_id)) = (spec.parent, spec.channel_id) {
            self.store_thread_parent(&id, created_at, channel_id, parent)
                .await;
        }
        StoredEvent { id, created_at }
    }

    /// Writes the relay's `thread_metadata` row naming `parent` as the reply
    /// parent of the stored event.
    pub async fn store_thread_parent(
        &self,
        id: &MessageId,
        created_at: DateTime<Utc>,
        channel_id: Uuid,
        parent: &StoredEvent,
    ) {
        sqlx::query(
            "INSERT INTO thread_metadata
                 (community_id, event_created_at, event_id, channel_id,
                  parent_event_id, parent_event_created_at, root_event_id, root_event_created_at, depth)
             VALUES ($1, $2, $3, $4, $5, $6, $5, $6, 1)",
        )
        .bind(self.community_id)
        .bind(created_at)
        .bind(id.as_bytes().as_slice())
        .bind(channel_id)
        .bind(parent.id.as_bytes().as_slice())
        .bind(parent.created_at)
        .execute(&self.pool)
        .await
        .expect("insert thread metadata");
    }

    /// Hands a stored event to the inbox the way the atomic ingress does.
    pub async fn accept(&self, event: &StoredEvent, kind: i32, author: [u8; 32]) {
        self.control
            .insert_accepted_event(
                self.community_id,
                &InboxEvent {
                    event_id: event.id,
                    event_created_at: event.created_at,
                    event_kind: kind,
                    author_pubkey: author,
                    channel_id: if kind == KIND_GIFT_WRAP {
                        None
                    } else {
                        Some(self.channel_id)
                    },
                },
            )
            .await
            .expect("insert inbox row");
    }

    /// Stores a human channel text, accepts it, and routes it.
    pub async fn route_human_text(&self, content: &str) -> (StoredEvent, ServiceOutcome) {
        let event = self
            .store_event(EventSpec {
                kind: KIND_STREAM_MESSAGE,
                author: self.human_key,
                content,
                tags: serde_json::json!([["h", self.channel_id.to_string()]]),
                channel_id: Some(self.channel_id),
                parent: None,
            })
            .await;
        self.accept(&event, KIND_STREAM_MESSAGE, self.human_key)
            .await;
        let outcome = self.route(&event).await.expect("route");
        (event, outcome)
    }

    /// Claims the accepted event and routes it through the production service.
    pub async fn route(&self, event: &StoredEvent) -> ortak_control::Result<ServiceOutcome> {
        let claim = self
            .control
            .claim_message(
                &self.scope,
                event.id,
                "normalizer-test",
                Duration::from_secs(60),
                5,
            )
            .await
            .expect("claim")
            .expect("row is claimable");
        self.service().route_claim(&self.scope, &claim).await
    }

    pub async fn count(&self, sql: &'static str, id: MessageId) -> i64 {
        sqlx::query(sql)
            .bind(self.company_id())
            .bind(id.as_bytes().to_vec())
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get::<i64, _>(0)
            .expect("count column")
    }

    /// `(origin_type, origin_id, root_message_id)` of the stored decision.
    pub async fn decision_provenance(&self, id: MessageId) -> (String, Option<String>, MessageId) {
        let row = sqlx::query(
            "SELECT origin_type, origin_id, root_message_id FROM routing_decisions
              WHERE company_id = $1 AND message_id = $2",
        )
        .bind(self.company_id())
        .bind(id.as_bytes().to_vec())
        .fetch_one(&self.pool)
        .await
        .expect("decision row");
        let root: Vec<u8> = row.try_get("root_message_id").expect("root");
        (
            row.try_get("origin_type").expect("origin_type"),
            row.try_get("origin_id").expect("origin_id"),
            MessageId::try_from_slice(&root).expect("root id"),
        )
    }

    pub async fn run_dispatch_rows(&self, id: MessageId) -> i64 {
        self.count(
            "SELECT count(*) FROM outbox o JOIN routing_decisions d
               ON d.company_id = o.company_id AND d.id = o.routing_decision_id
             WHERE o.company_id = $1 AND o.kind = 'run_dispatch' AND d.message_id = $2",
            id,
        )
        .await
    }

    /// Adds a retired, unverified Office binding for `employee_id` under a
    /// fresh key, the shape a rotated-away historical key leaves behind.
    pub async fn add_retired_binding(&self, employee_id: &str, revision_id: Uuid) -> [u8; 32] {
        let key = generated_key();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO employee_office_bindings
                 (company_id, employee_id, revision_id, provisioning_mode, public_key, signer_ref,
                  verified_at, valid_from, valid_until)
             VALUES ($1, $2, $3, 'adopt', $4, $5, NULL, $6, $7)",
        )
        .bind(self.company_id())
        .bind(employee_id)
        .bind(revision_id)
        .bind(key.as_slice())
        .bind(format!("credential://ortak-runtime/{employee_id}/retired"))
        .bind(now - chrono::Duration::days(30))
        .bind(now - chrono::Duration::days(1))
        .execute(&self.pool)
        .await
        .expect("insert retired binding");
        key
    }

    /// Marks `key`'s membership of the fixture channel as removed, the way
    /// the relay records a leave/kick.
    pub async fn remove_channel_member(&self, key: &[u8; 32]) {
        sqlx::query(
            "UPDATE channel_members SET removed_at = now()
              WHERE community_id = $1 AND channel_id = $2 AND pubkey = $3",
        )
        .bind(self.community_id)
        .bind(self.channel_id)
        .bind(key.as_slice())
        .execute(&self.pool)
        .await
        .expect("remove channel member");
    }

    /// Adds `key` to the fixture channel with the relay's `bot` role, the
    /// shape a legacy ACP agent membership leaves behind.
    pub async fn add_bot_member(&self, key: &[u8; 32]) {
        sqlx::query(
            "INSERT INTO channel_members (community_id, channel_id, pubkey, role)
             VALUES ($1, $2, $3, 'bot')",
        )
        .bind(self.community_id)
        .bind(self.channel_id)
        .bind(key.as_slice())
        .execute(&self.pool)
        .await
        .expect("insert bot member");
    }

    /// Inserts a `users` row for `key`; `agent_owner` marks it as a legacy
    /// relay agent and `deactivated` as a deactivated user.
    pub async fn add_user(
        &self,
        key: &[u8; 32],
        agent_owner: Option<&[u8; 32]>,
        deactivated: bool,
    ) {
        sqlx::query(
            "INSERT INTO users (community_id, pubkey, agent_owner_pubkey, deactivated_at)
             VALUES ($1, $2, $3, CASE WHEN $4 THEN now() ELSE NULL END)",
        )
        .bind(self.community_id)
        .bind(key.as_slice())
        .bind(agent_owner.map(|owner| owner.to_vec()))
        .bind(deactivated)
        .execute(&self.pool)
        .await
        .expect("insert user");
    }

    /// Applies one channel-state mutation to the fixture channel.
    pub async fn set_channel_state(&self, sql: &'static str) {
        sqlx::query(sql)
            .bind(self.community_id)
            .bind(self.channel_id)
            .execute(&self.pool)
            .await
            .expect("update channel");
    }

    /// Sets the fixture channel's canonical `visibility` (`open` or `private`).
    pub async fn set_visibility(&self, visibility: &str) {
        sqlx::query(
            "UPDATE channels SET visibility = $3::channel_visibility
              WHERE community_id = $1 AND id = $2",
        )
        .bind(self.community_id)
        .bind(self.channel_id)
        .bind(visibility)
        .execute(&self.pool)
        .await
        .expect("set channel visibility");
    }

    /// Inserts the next revision for `employee_id` with the active
    /// revision's manifest after `mutate` ran on it, and activates it. No
    /// Office binding is written: provisioning reuses the introduced key
    /// when the manifest keeps it. Returns the new revision id.
    pub async fn activate_manifest_revision(
        &self,
        employee_id: &str,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> Uuid {
        let row = sqlx::query(
            "SELECT rev.manifest, rev.revision_number
               FROM employees e
               JOIN employee_revisions rev
                 ON rev.company_id = e.company_id AND rev.id = e.active_revision_id
              WHERE e.company_id = $1 AND e.id = $2",
        )
        .bind(self.company_id())
        .bind(employee_id)
        .fetch_one(&self.pool)
        .await
        .expect("active revision");
        let mut manifest: serde_json::Value = row.try_get("manifest").expect("manifest");
        let revision_number: i64 = row.try_get("revision_number").expect("revision number");
        mutate(&mut manifest);
        let fingerprint = Sha256::digest(manifest.to_string().as_bytes()).to_vec();
        let revision_id: Uuid = sqlx::query(
            "INSERT INTO employee_revisions
                 (company_id, employee_id, revision_number, manifest, manifest_fingerprint, provisioning_mode)
             VALUES ($1, $2, $3, $4, $5, 'adopt') RETURNING id",
        )
        .bind(self.company_id())
        .bind(employee_id)
        .bind(revision_number + 1)
        .bind(manifest)
        .bind(fingerprint)
        .fetch_one(&self.pool)
        .await
        .expect("insert revision")
        .try_get("id")
        .expect("revision id");
        sqlx::query(
            "UPDATE employees SET active_revision_id = $3, updated_at = now()
              WHERE company_id = $1 AND id = $2",
        )
        .bind(self.company_id())
        .bind(employee_id)
        .bind(revision_id)
        .execute(&self.pool)
        .await
        .expect("activate revision");
        revision_id
    }

    /// The introducing `revision_id` recorded on the binding for `key`.
    pub async fn binding_revision(&self, key: &[u8; 32]) -> Uuid {
        sqlx::query_scalar(
            "SELECT revision_id FROM employee_office_bindings
              WHERE company_id = $1 AND public_key = $2",
        )
        .bind(self.company_id())
        .bind(key.as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("binding revision")
    }

    pub async fn add_relay_member(&self, key: &[u8; 32]) {
        sqlx::query(
            "INSERT INTO relay_members (community_id, pubkey, role) VALUES ($1, $2, 'member')",
        )
        .bind(self.community_id)
        .bind(hex::encode(key))
        .execute(&self.pool)
        .await
        .expect("insert relay member");
    }

    /// Records a completed `reply` run for `decision_id` (which must have
    /// woken `employee_id`) and freezes `published` as its `office_publish`
    /// outbox row, the persisted provenance of an Ortak-published event.
    pub async fn record_published_run(
        &self,
        decision_id: Uuid,
        employee_id: &str,
        revision_id: Uuid,
        message: &StoredEvent,
        root: &StoredEvent,
        published: &StoredEvent,
    ) -> Uuid {
        let run_id: Uuid = sqlx::query(
            "INSERT INTO runs
                 (company_id, employee_id, employee_revision_id, routing_decision_id, message_id,
                  root_message_id, runtime_adapter, status, delivery_intent, started_at, finished_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'fake-runtime', 'completed', 'reply', now(), now())
             RETURNING id",
        )
        .bind(self.company_id())
        .bind(employee_id)
        .bind(revision_id)
        .bind(decision_id)
        .bind(message.id.as_bytes().as_slice())
        .bind(root.id.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("insert run")
        .try_get("id")
        .expect("run id");
        sqlx::query(
            "INSERT INTO outbox
                 (company_id, kind, dedup_key, run_id, payload, signed_event_id, signed_event_bytes,
                  state, delivered_at)
             VALUES ($1, 'office_publish', $2, $3, '{}'::jsonb, $4, $5, 'delivered', now())",
        )
        .bind(self.company_id())
        .bind(format!("office_publish:{run_id}"))
        .bind(run_id)
        .bind(published.id.as_bytes().as_slice())
        .bind(b"frozen".as_slice())
        .execute(&self.pool)
        .await
        .expect("insert office_publish row");
        run_id
    }
}

async fn create_channel(pool: &PgPool, community_id: Uuid, creator: &[u8; 32]) -> Uuid {
    sqlx::query(
        "INSERT INTO channels (community_id, name, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(community_id)
    .bind(format!("office-{}", Uuid::new_v4().simple()))
    .bind(creator.as_slice())
    .fetch_one(pool)
    .await
    .expect("insert channel")
    .try_get("id")
    .expect("channel id")
}

pub async fn add_channel_member(
    pool: &PgPool,
    community_id: Uuid,
    channel_id: Uuid,
    key: &[u8; 32],
) {
    sqlx::query(
        "INSERT INTO channel_members (community_id, channel_id, pubkey) VALUES ($1, $2, $3)",
    )
    .bind(community_id)
    .bind(channel_id)
    .bind(key.as_slice())
    .execute(pool)
    .await
    .expect("insert channel member");
}

/// Inserts the employee, one active revision carrying the full manifest, and
/// a verified open-ended Office binding for the manifest key.
async fn activate_employee(pool: &PgPool, company_id: Uuid, employee: &Employee) -> Uuid {
    sqlx::query("INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')")
        .bind(company_id)
        .bind(employee.id.as_str())
        .execute(pool)
        .await
        .expect("insert employee");
    let mut manifest = employee.clone();
    manifest.status = EmployeeStatus::Active;
    let manifest = serde_json::to_value(&manifest).expect("manifest json");
    let fingerprint = Sha256::digest(manifest.to_string().as_bytes()).to_vec();
    let revision_id: Uuid = sqlx::query(
        "INSERT INTO employee_revisions
             (company_id, employee_id, revision_number, manifest, manifest_fingerprint, provisioning_mode)
         VALUES ($1, $2, 1, $3, $4, 'adopt') RETURNING id",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(manifest)
    .bind(fingerprint)
    .fetch_one(pool)
    .await
    .expect("insert revision")
    .try_get("id")
    .expect("revision id");
    sqlx::query(
        "UPDATE employees SET active_revision_id = $3, status = 'active', updated_at = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .execute(pool)
    .await
    .expect("activate revision");
    let key = hex::decode(&employee.office.public_key).expect("fixture key hex");
    sqlx::query(
        "INSERT INTO employee_office_bindings
             (company_id, employee_id, revision_id, provisioning_mode, public_key, signer_ref, verified_at)
         VALUES ($1, $2, $3, 'adopt', $4, $5, now())",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .bind(key)
    .bind(employee.office.signer_ref.as_str())
    .execute(pool)
    .await
    .expect("insert office binding");
    revision_id
}
