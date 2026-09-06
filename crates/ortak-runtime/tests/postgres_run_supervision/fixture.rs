use super::*;
use ortak_control::fakes::FakeMemoryAdapter;
use ortak_runtime::memory_context::AdapterRunMemory;

#[path = "../../../ortak-control/tests/cohort_support.rs"]
pub(super) mod cohort_support;

pub(super) const PROFILE_REF: &str = "fake://profiles/cem";

pub(super) fn database_url() -> String {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL")
        .expect("ORTAK_TEST_DATABASE_URL must explicitly name disposable localhost:55432; generic database variables are ignored");
    let options: sqlx::postgres::PgConnectOptions =
        url.parse().expect("valid disposable database URL");
    assert!(
        matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432,
        "Postgres supervision tests require disposable localhost:55432"
    );
    url
}

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

pub(super) async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    MIGRATED
        .get_or_init(|| async {
            buzz_db::migration::run_migrations(&pool)
                .await
                .expect("apply migrations once before concurrent fixtures");
        })
        .await;
    pool
}

pub(super) fn message_id() -> MessageId {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    MessageId::from_bytes(bytes)
}

pub(super) fn employee_id(value: &str) -> EmployeeId {
    EmployeeId::parse(value).expect("valid employee id")
}

/// The Cem fixture rebound to the fake runtime; secrets remain references.
pub(super) fn fixture_employee() -> Employee {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/cem.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
    let mut employee = manifest.employee;
    employee.status = EmployeeStatus::Active;
    employee.runtime.adapter = "fake-runtime".to_owned();
    employee.runtime.profile_ref = Some(PROFILE_REF.to_owned());
    if let Some(memory) = &mut employee.memory {
        memory.adapter = "fake-memory".to_owned();
    }
    employee
}

pub(super) struct RunRow {
    pub(super) status: String,
    pub(super) runtime_run_ref: Option<String>,
    pub(super) employee_revision_id: Uuid,
    pub(super) message_id: Vec<u8>,
    pub(super) delivery_intent: Option<String>,
    pub(super) cancel_reason: Option<String>,
    pub(super) error_code: Option<String>,
    pub(super) finished_at: Option<chrono::DateTime<Utc>>,
}

pub(super) struct OutboxRow {
    pub(super) state: String,
    pub(super) attempt_count: i32,
    pub(super) lease_token: Option<Uuid>,
    pub(super) run_id: Option<Uuid>,
    pub(super) last_error: Option<String>,
}

pub(super) struct Fixture {
    pub(super) pool: PgPool,
    pub(super) control: PgControlPlane,
    pub(super) community_id: Uuid,
    pub(super) scope: CompanyScope,
    pub(super) revision_id: Uuid,
    pub(super) policy: RoutingPolicy,
    pub(super) adapter: FakeRuntimeAdapter,
    pub(super) memory: FakeMemoryAdapter,
    pub(super) employee: Employee,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        Self::new_for_employee(fixture_employee()).await
    }

    pub(super) async fn new_for_employee(employee: Employee) -> Self {
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let policy = RoutingPolicy::default();
        let (community_id, company_id) = create_company(&pool, &policy).await;
        let revision_id = activate_employee(&pool, company_id, &employee, true).await;
        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve scope");
        let channel: Uuid = sqlx::query_scalar(
            "INSERT INTO channels(community_id,name,created_by) VALUES ($1,$2,$3) RETURNING id",
        )
        .bind(community_id)
        .bind(format!("supervision-cohort-{company_id}"))
        .bind([7u8; 32].as_slice())
        .fetch_one(&pool)
        .await
        .expect("fixture cohort channel");
        cohort_support::select_and_reconcile(
            &control,
            &scope,
            &[channel],
            std::slice::from_ref(&employee.id),
        )
        .await;
        let mut memory = FakeMemoryAdapter::new();
        if let Some(binding) = &employee.memory {
            memory = memory.with_existing_binding(binding);
        }
        Self {
            memory,
            employee,
            pool,
            control,
            community_id,
            scope,
            revision_id,
            policy,
            adapter: FakeRuntimeAdapter::new().with_existing_profile(PROFILE_REF, true),
        }
    }

    pub(super) fn supervisor(
        &self,
        config: SupervisorConfig,
    ) -> RunSupervisor<PgControlPlane, &FakeRuntimeAdapter, AdapterRunMemory<'_, FakeMemoryAdapter>>
    {
        RunSupervisor::new(self.control.clone(), &self.adapter, config).with_memory(&self.memory)
    }

    pub(super) fn config(&self) -> SupervisorConfig {
        SupervisorConfig {
            retry_backoff: Duration::ZERO,
            ..SupervisorConfig::default()
        }
    }

    /// Stores a signed channel text event (kind 9) plus its inbox row, routes
    /// it to Cem through the production routing commit, and returns the
    /// decision id.
    pub(super) async fn route(&self, content: &str) -> Uuid {
        self.route_kind(KIND_STREAM_MESSAGE, Some(Uuid::new_v4()), content)
            .await
    }

    /// Like [`Self::route`] for an arbitrary event kind and channel scope:
    /// the shape a stale or hand-seeded dispatch for a non-channel event
    /// would leave behind.
    pub(super) async fn route_kind(
        &self,
        kind: i32,
        channel_id: Option<Uuid>,
        content: &str,
    ) -> Uuid {
        self.route_kind_with_reply(kind, channel_id, content, None)
            .await
    }

    pub(super) async fn route_kind_with_reply(
        &self,
        kind: i32,
        channel_id: Option<Uuid>,
        content: &str,
        reply: Option<(
            MessageId,
            chrono::DateTime<Utc>,
            MessageId,
            chrono::DateTime<Utc>,
        )>,
    ) -> Uuid {
        let id = message_id();
        if let Some(channel_id) = channel_id {
            sqlx::query(
                "INSERT INTO channels (community_id, id, name, created_by) VALUES ($1, $2, $3, $4) ON CONFLICT (community_id,id) DO NOTHING",
            )
            .bind(self.community_id)
            .bind(channel_id)
            .bind(format!("test-{channel_id}"))
            .bind([7u8; 32].as_slice())
            .execute(&self.pool)
            .await
            .expect("channel");
            for key in [
                [7u8; 32].to_vec(),
                hex::decode(&self.employee.office.public_key).expect("office key"),
            ] {
                sqlx::query("INSERT INTO channel_members (community_id, channel_id, pubkey) VALUES ($1, $2, $3) ON CONFLICT (community_id,channel_id,pubkey) DO NOTHING")
                    .bind(self.community_id).bind(channel_id).bind(key).execute(&self.pool).await.expect("member");
            }
            let mut channels = self
                .control
                .routing_cohort(&self.scope)
                .await
                .expect("cohort")
                .expect("explicit fixture cohort")
                .channel_ids;
            if !channels.contains(&channel_id) {
                channels.push(channel_id);
                cohort_support::select_and_reconcile(
                    &self.control,
                    &self.scope,
                    &channels,
                    std::slice::from_ref(&self.employee.id),
                )
                .await;
            }
        }
        let created_at = Utc::now();
        sqlx::query(
            "INSERT INTO events
                 (community_id, id, pubkey, created_at, kind, tags, content, sig, channel_id)
             VALUES ($1, $2, $3, $4, $5, '[]'::jsonb, $6, $7, $8)",
        )
        .bind(self.community_id)
        .bind(id.as_bytes().as_slice())
        .bind([7u8; 32].as_slice())
        .bind(created_at)
        .bind(kind)
        .bind(content)
        .bind([9u8; 64].as_slice())
        .bind(channel_id)
        .execute(&self.pool)
        .await
        .expect("insert event");
        if let Some((parent, parent_at, root, root_at)) = reply {
            sqlx::query("INSERT INTO thread_metadata(community_id,event_created_at,event_id,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,$5,$6,$7,$8,1)")
                .bind(self.community_id).bind(created_at).bind(id.as_bytes().as_slice()).bind(channel_id)
                .bind(parent.as_bytes().as_slice()).bind(parent_at).bind(root.as_bytes().as_slice()).bind(root_at)
                .execute(&self.pool).await.expect("canonical reply metadata before routing");
        }
        self.control
            .insert_accepted_event(
                self.community_id,
                &InboxEvent {
                    event_id: id,
                    event_created_at: created_at,
                    event_kind: kind,
                    author_pubkey: [7; 32],
                    channel_id,
                },
            )
            .await
            .expect("insert inbox row");
        let claim = self
            .control
            .claim_message(&self.scope, id, "router", Duration::from_secs(60), 5)
            .await
            .expect("claim")
            .expect("claimable");
        let snapshot = self
            .control
            .routing_snapshot(&self.scope, id)
            .await
            .expect("snapshot")
            .expect("inbox");
        let office_input_hash = match ortak_office::PgChannelNormalizer::new(self.pool.clone())
            .normalize(&self.scope, &snapshot.inbox)
            .await
        {
            Ok(Normalization::Message(message)) => ortak_control::service::office_input_hash(
                &message.envelope,
                message.root_message_id,
                &message.eligible_employee_ids,
            ),
            _ => [0; 32], // Deliberately invalid fixtures must fail admission before runtime I/O.
        };
        let proposal = RoutingProposal {
            office_input_hash,
            office_authority: snapshot.office_authority,
            company_id: self.scope.company_id(),
            message_id: id,
            root_message_id: id,
            claim_generation: claim.claim_generation,
            origin: MessageOrigin::Human("sefa".to_owned()),
            input_hash: [3; 32],
            candidates: vec![CandidateRevision {
                employee_id: self.employee.id.clone(),
                revision_id: self.revision_id,
            }],
            roster_scope: RosterScope::Targets,
            eligible_employee_ids: std::iter::once(self.employee.id.clone()).collect(),
            decision: RoutingDecision {
                message_id: id.to_hex(),
                mode: RoutingMode::Deterministic,
                summary_reason: RoutingReason::StructuredDispatch,
                policy_version: self.policy.version.clone(),
                policy_fingerprint: self.policy.fingerprint(),
                recipients: vec![RecipientDecision {
                    employee_id: self.employee.id.clone(),
                    action: RecipientAction::Wake,
                    reason: RoutingReason::StructuredDispatch,
                    score: None,
                    evidence: Vec::new(),
                }],
            },
            scorer: None,
        };
        match self
            .control
            .commit_routing(&self.scope, &proposal)
            .await
            .expect("commit routing")
        {
            RoutingCommitOutcome::Committed(decision) => {
                assert_eq!(decision.dispatches.len(), 1);
                decision.decision_id
            }
            other => panic!("expected a committed decision, got {other:?}"),
        }
    }

    pub(super) async fn lease(&self, lease: Duration) -> OutboxLease {
        let mut leases = self
            .control
            .claim_due(
                &self.scope,
                Some(OutboxKind::RunDispatch),
                "dispatcher",
                lease,
                10,
            )
            .await
            .expect("claim due");
        assert_eq!(leases.len(), 1, "expected exactly one due run_dispatch row");
        leases.remove(0)
    }

    pub(super) async fn run_rows(&self) -> i64 {
        sqlx::query("SELECT count(*) FROM runs WHERE company_id = $1")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("count column")
    }

    pub(super) async fn run(&self, run_id: Uuid) -> RunRow {
        let row = sqlx::query(
            "SELECT status, runtime_run_ref, employee_revision_id, message_id, delivery_intent,
                    cancel_reason, error_code, finished_at
               FROM runs WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .fetch_one(&self.pool)
        .await
        .expect("run row");
        RunRow {
            status: row.try_get("status").expect("status"),
            runtime_run_ref: row.try_get("runtime_run_ref").expect("ref"),
            employee_revision_id: row.try_get("employee_revision_id").expect("revision"),
            message_id: row.try_get("message_id").expect("message"),
            delivery_intent: row.try_get("delivery_intent").expect("intent"),
            cancel_reason: row.try_get("cancel_reason").expect("cancel reason"),
            error_code: row.try_get("error_code").expect("error code"),
            finished_at: row.try_get("finished_at").expect("finished"),
        }
    }

    pub(super) async fn outbox(&self, outbox_id: Uuid) -> OutboxRow {
        let row = sqlx::query(
            "SELECT state, attempt_count, lease_token, run_id, last_error
               FROM outbox WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(outbox_id)
        .fetch_one(&self.pool)
        .await
        .expect("outbox row");
        OutboxRow {
            state: row.try_get("state").expect("state"),
            attempt_count: row.try_get("attempt_count").expect("attempts"),
            lease_token: row.try_get("lease_token").expect("token"),
            run_id: row.try_get("run_id").expect("run id"),
            last_error: row.try_get("last_error").expect("error"),
        }
    }

    /// `(sequence, event_type, runtime_cursor, payload)` in order.
    pub(super) async fn events(
        &self,
        run_id: Uuid,
    ) -> Vec<(i64, String, Option<String>, serde_json::Value)> {
        sqlx::query(
            "SELECT sequence, event_type, runtime_cursor, payload FROM run_events
              WHERE company_id = $1 AND run_id = $2 ORDER BY sequence",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .expect("events")
        .iter()
        .map(|row| {
            (
                row.try_get("sequence").expect("sequence"),
                row.try_get("event_type").expect("type"),
                row.try_get("runtime_cursor").expect("cursor"),
                row.try_get("payload").expect("payload"),
            )
        })
        .collect()
    }

    pub(super) async fn started(&self) -> (Uuid, RuntimeRunRef, Uuid) {
        self.route("Cem, selam nasılsın?").await;
        let lease = self.lease(Duration::from_secs(60)).await;
        match self
            .supervisor(self.config())
            .dispatch(&self.scope, &lease)
            .await
            .expect("dispatch")
        {
            DispatchOutcome::Started {
                run_id,
                runtime_run_ref,
            } => (run_id, runtime_run_ref, lease.id),
            other => panic!("expected a started run, got {other:?}"),
        }
    }
}

pub(super) async fn create_company(pool: &PgPool, policy: &RoutingPolicy) -> (Uuid, Uuid) {
    let community_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(community_id)
        .bind(format!("ortak-runtime-{}.example", community_id.simple()))
        .execute(pool)
        .await
        .expect("insert community");
    let company_id: Uuid = sqlx::query(
        "INSERT INTO companies (slug, display_name, routing_policy)
         VALUES ($1, 'Ortak runtime test', $2) RETURNING id",
    )
    .bind(format!("co-{}", Uuid::new_v4().simple()))
    .bind(serde_json::to_value(policy).expect("policy json"))
    .fetch_one(pool)
    .await
    .expect("insert company")
    .try_get("id")
    .expect("company id");
    sqlx::query("INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)")
        .bind(community_id)
        .bind(company_id)
        .execute(pool)
        .await
        .expect("insert binding");
    (community_id, company_id)
}

/// Employee, immutable revision, validated runtime binding row, activation:
/// the shape the provisioning saga leaves behind.
pub(super) async fn activate_employee(
    pool: &PgPool,
    company_id: Uuid,
    employee: &Employee,
    validated: bool,
) -> Uuid {
    sqlx::query(
        "INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')
         ON CONFLICT DO NOTHING",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .execute(pool)
    .await
    .expect("insert employee");
    let manifest = serde_json::to_value(employee).expect("manifest json");
    let revision_id: Uuid = sqlx::query(
        "INSERT INTO employee_revisions
             (company_id, employee_id, revision_number, manifest, manifest_fingerprint, provisioning_mode)
         VALUES ($1, $2,
                 (SELECT coalesce(max(revision_number), 0) + 1 FROM employee_revisions
                   WHERE company_id = $1 AND employee_id = $2),
                 $3, $4, 'adopt')
         RETURNING id",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(&manifest)
    .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
    .fetch_one(pool)
    .await
    .expect("insert revision")
    .try_get("id")
    .expect("revision id");
    sqlx::query(
        "INSERT INTO employee_runtime_bindings
             (company_id, revision_id, employee_id, adapter, provisioning_mode, profile_ref,
              model, workspace_ref, credential_refs, options, validated_at)
         VALUES ($1, $2, $3, $4, 'adopt', $5, $6, $7, $8, $9, $10)",
    )
    .bind(company_id)
    .bind(revision_id)
    .bind(employee.id.as_str())
    .bind(&employee.runtime.adapter)
    .bind(employee.runtime.profile_ref.as_deref())
    .bind(&employee.runtime.model)
    .bind(&employee.runtime.workspace_ref)
    .bind(serde_json::to_value(&employee.runtime.credential_refs).expect("refs"))
    .bind(serde_json::to_value(&employee.runtime.options).expect("options"))
    .bind(validated.then(Utc::now))
    .execute(pool)
    .await
    .expect("insert runtime binding");
    if let Some(memory) = &employee.memory {
        sqlx::query("INSERT INTO employee_memory_bindings (company_id,revision_id,employee_id,adapter,provisioning_mode,endpoint_ref,workspace,user_peer,employee_peer,options,validated_at) VALUES ($1,$2,$3,$4,'adopt',$5,$6,$7,$8,$9,$10)")
            .bind(company_id).bind(revision_id).bind(employee.id.as_str())
            .bind(&memory.adapter).bind(&memory.endpoint_ref).bind(&memory.workspace)
            .bind(&memory.user_peer).bind(&memory.employee_peer)
            .bind(serde_json::to_value(&memory.options).expect("memory options"))
            .bind(validated.then(Utc::now)).execute(pool).await.expect("memory binding");
    }
    sqlx::query(
        "UPDATE employees SET active_revision_id = $3, status = 'active', updated_at = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .execute(pool)
    .await
    .expect("activate");
    sqlx::query("INSERT INTO employee_office_bindings (company_id, employee_id, revision_id, provisioning_mode, public_key, signer_ref, verified_at) VALUES ($1, $2, $3, 'adopt', $4, $5, now()) ON CONFLICT DO NOTHING")
        .bind(company_id).bind(employee.id.as_str()).bind(revision_id)
        .bind(hex::decode(&employee.office.public_key).expect("office key"))
        .bind(employee.office.signer_ref.as_str()).execute(pool).await.expect("office binding");
    revision_id
}

pub(super) fn authorized(authorization: DispatchAuthorization) -> ortak_runtime::DispatchAuthority {
    match authorization {
        DispatchAuthorization::Authorized(authority) => *authority,
        other => panic!("expected an authorized dispatch, got {other:?}"),
    }
}
