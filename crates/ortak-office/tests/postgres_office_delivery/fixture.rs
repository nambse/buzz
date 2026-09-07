use super::*;

pub(super) const SIGNER_REF: &str = "credential://office/cem";

pub(super) fn database_url() -> String {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL")
        .expect("explicit disposable ORTAK_TEST_DATABASE_URL required");
    let options: sqlx::postgres::PgConnectOptions = url.parse().expect("valid URL");
    assert!(
        matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432,
        "Office PG tests require disposable localhost:55432"
    );
    url
}
static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
pub(super) async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url()).await.expect("connect");
    MIGRATED
        .get_or_init(|| async {
            buzz_db::migration::run_migrations(&pool)
                .await
                .expect("migrate once");
        })
        .await;
    pool
}

pub(super) struct RowState {
    pub(super) state: String,
    pub(super) attempt_count: i32,
    pub(super) signed_event_id: Option<Vec<u8>>,
    pub(super) signed_event_bytes: Option<Vec<u8>>,
    pub(super) last_error: Option<String>,
}

/// How a test binding row is shaped.
#[derive(Clone, Copy)]
pub(super) enum BindingShape {
    Verified,
    Unverified,
    Retired,
}

pub(super) struct Fixture {
    pub(super) pool: PgPool,
    pub(super) control: PgControlPlane,
    pub(super) scope: CompanyScope,
    pub(super) run_id: Uuid,
    pub(super) revision_id: Uuid,
    pub(super) binding_id: Uuid,
    pub(super) signer: FakeOfficeSigner,
    pub(super) publisher: FakeOfficePublisher,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let community_id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(community_id)
            .bind(format!("ortak-office-{}.example", community_id.simple()))
            .execute(&pool)
            .await
            .expect("insert community");
        let company_id: Uuid = sqlx::query(
            "INSERT INTO companies (slug, display_name, routing_policy)
             VALUES ($1, 'Ortak office test', $2) RETURNING id",
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
        let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER_REF);
        let public_key = signer.public_key(SIGNER_REF).expect("registered signer");
        insert_employee(&pool, company_id, "cem").await;
        let revision_id =
            insert_revision(&pool, company_id, "cem", 1, &public_key, SIGNER_REF).await;
        let binding_id = insert_binding(
            &pool,
            company_id,
            "cem",
            revision_id,
            &public_key,
            SIGNER_REF,
            BindingShape::Verified,
        )
        .await;
        sqlx::query("UPDATE employees SET active_revision_id=$3,status='active' WHERE company_id=$1 AND id=$2")
            .bind(company_id).bind("cem").bind(revision_id).execute(&pool).await.expect("activate canonical author");
        let run_id = insert_canonical_run(&pool, company_id, revision_id, "Merhaba from Cem").await;
        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve scope");
        Self {
            pool,
            control,
            scope,
            run_id,
            revision_id,
            binding_id,
            signer,
            publisher: FakeOfficePublisher::new(),
        }
    }

    pub(super) fn public_key(&self) -> OfficePublicKey {
        self.signer
            .public_key(SIGNER_REF)
            .expect("registered signer")
    }

    pub(super) async fn draft(&self) -> OfficePublishDraft {
        self.draft_for(self.run_id).await
    }

    pub(super) async fn draft_for(&self, run_id: Uuid) -> OfficePublishDraft {
        let query="SELECT draft_kind,draft_tags,draft_content FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2 AND draft_kind IS NOT NULL";
        let row = sqlx::query(query)
            .bind(self.scope.company_id())
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
            .expect("requested canonical draft");
        let row = match row {
            Some(row) => row,
            None => sqlx::query(query)
                .bind(self.scope.company_id())
                .bind(self.run_id)
                .fetch_one(&self.pool)
                .await
                .expect("fallback canonical draft for refused run"),
        };
        OfficePublishDraft {
            company_id: self.scope.company_id(),
            run_id,
            kind: row.get::<i32, _>("draft_kind") as u16,
            tags: serde_json::from_value(row.get("draft_tags")).expect("canonical tags"),
            content: row.get("draft_content"),
        }
    }

    pub(super) fn service(
        &self,
        retry_backoff: Duration,
    ) -> OfficeDeliveryService<PgControlPlane, &FakeOfficeSigner, &FakeOfficePublisher> {
        OfficeDeliveryService::new(
            self.control.clone(),
            &self.signer,
            &self.publisher,
            DeliveryConfig { retry_backoff },
        )
    }

    /// Enqueues the fixture draft and returns the canonical authorized publish.
    pub(super) async fn enqueue(&self) -> AuthorizedOfficePublish {
        match self
            .control
            .enqueue_office_publish(&self.scope, &self.draft().await)
            .await
            .expect("enqueue")
        {
            EnqueueOutcome::Enqueued(authorized) => authorized,
            other => panic!("expected a fresh row, got {other:?}"),
        }
    }

    /// Replays the fixture draft, as a retry in a fresh process would, and
    /// requires the same authorized publish back.
    pub(super) async fn replay(
        &self,
        expected: &AuthorizedOfficePublish,
    ) -> AuthorizedOfficePublish {
        let outcome = self
            .control
            .enqueue_office_publish(&self.scope, &self.draft().await)
            .await
            .expect("replay enqueue");
        assert_eq!(outcome, EnqueueOutcome::Existing(expected.clone()));
        outcome.into_authorized()
    }

    pub(super) async fn claim(&self, lease: Duration) -> OutboxLease {
        let mut leases = self
            .control
            .claim_due(
                &self.scope,
                Some(OutboxKind::OfficePublish),
                "office-worker",
                lease,
                10,
            )
            .await
            .expect("claim");
        assert_eq!(
            leases.len(),
            1,
            "expected exactly one due office_publish row"
        );
        leases.remove(0)
    }

    pub(super) async fn nothing_due(&self) {
        assert!(self
            .control
            .claim_due(
                &self.scope,
                Some(OutboxKind::OfficePublish),
                "office-worker",
                Duration::from_secs(30),
                10
            )
            .await
            .expect("claim")
            .is_empty());
    }

    pub(super) async fn row(&self, outbox_id: Uuid) -> RowState {
        let row = sqlx::query(
            "SELECT state, attempt_count, signed_event_id, signed_event_bytes, last_error
               FROM outbox WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(outbox_id)
        .fetch_one(&self.pool)
        .await
        .expect("read row");
        RowState {
            state: row.try_get("state").expect("state"),
            attempt_count: row.try_get("attempt_count").expect("attempts"),
            signed_event_id: row.try_get("signed_event_id").expect("id"),
            signed_event_bytes: row.try_get("signed_event_bytes").expect("bytes"),
            last_error: row.try_get("last_error").expect("error"),
        }
    }

    pub(super) async fn assert_untouched(&self, outbox_id: Uuid) {
        let row = self.row(outbox_id).await;
        assert_eq!(row.state, "pending");
        assert!(row.signed_event_id.is_none());
        assert!(row.signed_event_bytes.is_none());
        assert_eq!(self.signer.sign_calls(), 0);
        assert!(self.publisher.published().is_empty());
    }

    pub(super) async fn outbox_rows(&self) -> i64 {
        sqlx::query("SELECT count(*) FROM outbox WHERE company_id = $1 AND kind='office_publish'")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("count column")
    }
}

pub(super) async fn insert_employee(pool: &PgPool, company_id: Uuid, employee_id: &str) {
    sqlx::query("INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')")
        .bind(company_id)
        .bind(employee_id)
        .execute(pool)
        .await
        .expect("insert employee");
}

/// Inserts a revision whose manifest declares the Office key and signer the
/// way provisioning stores a serialized `Employee` (only the `office` object
/// is consulted by delivery).
pub(super) async fn insert_revision(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: &str,
    revision_number: i32,
    public_key: &OfficePublicKey,
    signer_ref: &str,
) -> Uuid {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/cem.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("employee fixture");
    let manifest: ortak_domain::EmployeeManifest = serde_yaml::from_str(&yaml).expect("manifest");
    let mut employee = manifest.employee;
    employee.id = ortak_domain::EmployeeId::parse(employee_id).expect("employee id");
    employee.status = ortak_domain::EmployeeStatus::Active;
    employee.office.public_key = public_key.to_hex();
    employee.office.signer_ref =
        ortak_domain::CredentialRef::parse(signer_ref).expect("signer ref");
    let manifest = serde_json::to_value(employee).expect("full canonical employee manifest");
    sqlx::query(
        "INSERT INTO employee_revisions
             (company_id, employee_id, revision_number, manifest, manifest_fingerprint, provisioning_mode)
         VALUES ($1, $2, $3, $4, $5, 'adopt') RETURNING id",
    )
    .bind(company_id)
    .bind(employee_id)
    .bind(revision_number)
    .bind(&manifest)
    .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
    .fetch_one(pool)
    .await
    .expect("insert revision")
    .try_get("id")
    .expect("revision id")
}

pub(super) async fn insert_binding(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: &str,
    revision_id: Uuid,
    public_key: &OfficePublicKey,
    signer_ref: &str,
    shape: BindingShape,
) -> Uuid {
    let now = Utc::now();
    let hour = chrono::Duration::hours(1);
    let (verified_at, valid_from, valid_until): (
        Option<chrono::DateTime<Utc>>,
        chrono::DateTime<Utc>,
        Option<chrono::DateTime<Utc>>,
    ) = match shape {
        BindingShape::Verified => (Some(now), now - hour, None),
        BindingShape::Unverified => (None, now - hour, None),
        BindingShape::Retired => (Some(now - hour * 2), now - hour * 2, Some(now - hour)),
    };
    sqlx::query(
        "INSERT INTO employee_office_bindings
             (company_id, employee_id, revision_id, provisioning_mode, public_key, signer_ref,
              verified_at, valid_from, valid_until)
         VALUES ($1, $2, $3, 'adopt', $4, $5, $6, $7, $8)
         RETURNING id",
    )
    .bind(company_id)
    .bind(employee_id)
    .bind(revision_id)
    .bind(public_key.as_bytes().to_vec())
    .bind(signer_ref)
    .bind(verified_at)
    .bind(valid_from)
    .bind(valid_until)
    .fetch_one(pool)
    .await
    .expect("insert office binding")
    .try_get("id")
    .expect("binding id")
}

pub(super) async fn insert_run(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: &str,
    revision_id: Uuid,
    status: &str,
    delivery_intent: Option<&str>,
) -> Uuid {
    let finished_at = matches!(status, "completed" | "failed" | "cancelled").then(Utc::now);
    sqlx::query(
        "INSERT INTO runs
             (company_id, employee_id, employee_revision_id, runtime_adapter, status,
              delivery_intent, started_at, finished_at)
         VALUES ($1, $2, $3, 'fake-runtime', $4, $5, now(), $6) RETURNING id",
    )
    .bind(company_id)
    .bind(employee_id)
    .bind(revision_id)
    .bind(status)
    .bind(delivery_intent)
    .bind(finished_at)
    .fetch_one(pool)
    .await
    .expect("insert run")
    .try_get("id")
    .expect("run id")
}

/// A fresh employee with its own generated key, revision, binding, and
/// completed `reply` run in the fixture company.
pub(super) async fn employee_with_binding(
    fixture: &Fixture,
    employee_id: &str,
    shape: BindingShape,
) -> (Uuid, Uuid) {
    let signer_ref = format!("credential://office/{employee_id}");
    let signer = FakeOfficeSigner::new().with_generated_signer(&signer_ref);
    let key = signer.public_key(&signer_ref).expect("generated");
    let company_id = fixture.scope.company_id();
    insert_employee(&fixture.pool, company_id, employee_id).await;
    let revision_id =
        insert_revision(&fixture.pool, company_id, employee_id, 1, &key, &signer_ref).await;
    insert_binding(
        &fixture.pool,
        company_id,
        employee_id,
        revision_id,
        &key,
        &signer_ref,
        shape,
    )
    .await;
    let run_id = insert_run(
        &fixture.pool,
        company_id,
        employee_id,
        revision_id,
        "completed",
        Some("reply"),
    )
    .await;
    (revision_id, run_id)
}

pub(super) fn binding_rejection(error: OfficeDeliveryError) -> BindingRejection {
    match error {
        OfficeDeliveryError::BindingUnauthorized { reason, .. } => reason,
        other => panic!("expected a binding rejection, got {other:?}"),
    }
}
