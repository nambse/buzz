//! Production-seam Postgres tests for the Office delivery outbox slice.
//!
//! Run with a local database that can receive the embedded migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-office -- --ignored`

use std::time::Duration;

use chrono::Utc;
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::ports::{CompanyDirectory, OutboxRepository};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::RoutingPolicy;
use ortak_office::fakes::{FakeOfficePublisher, FakeOfficeSigner};
use ortak_office::{
    AuthorizedOfficePublish, BindingRejection, DeliveryConfig, DeliveryOutcome, EnqueueOutcome,
    FreezeOutcome, OfficeDeliveryError, OfficeDeliveryRepository, OfficeDeliveryService,
    OfficeEventError, OfficePublishDraft, OfficeSigner, PublishReceipt, KIND_STREAM_MESSAGE,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1 -- local test-only credentials
const SIGNER_REF: &str = "credential://office/cem";

fn database_url() -> String {
    std::env::var("ORTAK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("BUZZ_TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
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

struct RowState {
    state: String,
    attempt_count: i32,
    signed_event_id: Option<Vec<u8>>,
    signed_event_bytes: Option<Vec<u8>>,
    last_error: Option<String>,
}

/// How a test binding row is shaped.
#[derive(Clone, Copy)]
enum BindingShape {
    Verified,
    Unverified,
    Retired,
}

struct Fixture {
    pool: PgPool,
    control: PgControlPlane,
    scope: CompanyScope,
    run_id: Uuid,
    revision_id: Uuid,
    binding_id: Uuid,
    signer: FakeOfficeSigner,
    publisher: FakeOfficePublisher,
}

impl Fixture {
    async fn new() -> Self {
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
        let run_id = insert_run(
            &pool,
            company_id,
            "cem",
            revision_id,
            "completed",
            Some("reply"),
        )
        .await;
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

    fn public_key(&self) -> OfficePublicKey {
        self.signer
            .public_key(SIGNER_REF)
            .expect("registered signer")
    }

    fn draft(&self) -> OfficePublishDraft {
        self.draft_for(self.run_id)
    }

    fn draft_for(&self, run_id: Uuid) -> OfficePublishDraft {
        OfficePublishDraft {
            company_id: self.scope.company_id(),
            run_id,
            kind: KIND_STREAM_MESSAGE,
            tags: vec![vec!["h".to_owned(), "general".to_owned()]],
            content: "Merhaba from Cem".to_owned(),
        }
    }

    fn service(
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
    async fn enqueue(&self) -> AuthorizedOfficePublish {
        match self
            .control
            .enqueue_office_publish(&self.scope, &self.draft())
            .await
            .expect("enqueue")
        {
            EnqueueOutcome::Enqueued(authorized) => authorized,
            other => panic!("expected a fresh row, got {other:?}"),
        }
    }

    /// Replays the fixture draft, as a retry in a fresh process would, and
    /// requires the same authorized publish back.
    async fn replay(&self, expected: &AuthorizedOfficePublish) -> AuthorizedOfficePublish {
        let outcome = self
            .control
            .enqueue_office_publish(&self.scope, &self.draft())
            .await
            .expect("replay enqueue");
        assert_eq!(outcome, EnqueueOutcome::Existing(expected.clone()));
        outcome.into_authorized()
    }

    async fn claim(&self, lease: Duration) -> OutboxLease {
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

    async fn nothing_due(&self) {
        assert!(self
            .control
            .claim_due(
                &self.scope,
                None,
                "office-worker",
                Duration::from_secs(30),
                10
            )
            .await
            .expect("claim")
            .is_empty());
    }

    async fn row(&self, outbox_id: Uuid) -> RowState {
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

    async fn assert_untouched(&self, outbox_id: Uuid) {
        let row = self.row(outbox_id).await;
        assert_eq!(row.state, "pending");
        assert!(row.signed_event_id.is_none());
        assert!(row.signed_event_bytes.is_none());
        assert_eq!(self.signer.sign_calls(), 0);
        assert!(self.publisher.published().is_empty());
    }

    async fn outbox_rows(&self) -> i64 {
        sqlx::query("SELECT count(*) FROM outbox WHERE company_id = $1")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("count column")
    }
}

async fn insert_employee(pool: &PgPool, company_id: Uuid, employee_id: &str) {
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
async fn insert_revision(
    pool: &PgPool,
    company_id: Uuid,
    employee_id: &str,
    revision_number: i32,
    public_key: &OfficePublicKey,
    signer_ref: &str,
) -> Uuid {
    let manifest = serde_json::json!({
        "id": employee_id,
        "office": {
            "public_key": public_key.to_hex(),
            "signer_ref": signer_ref,
            "home_channel_ref": null,
        }
    });
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

async fn insert_binding(
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

async fn insert_run(
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
async fn employee_with_binding(
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

fn binding_rejection(error: OfficeDeliveryError) -> BindingRejection {
    match error {
        OfficeDeliveryError::BindingUnauthorized { reason, .. } => reason,
        other => panic!("expected a binding rejection, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn identity_and_signer_come_from_the_run_and_its_verified_binding() {
    let fixture = Fixture::new().await;
    let company_id = fixture.scope.company_id();

    // The caller's draft names only company, run, and message; the authorized
    // publish carries the employee and revision of the run row and the signer
    // and key of that revision's verified binding.
    let authorized = fixture.enqueue().await;
    assert_eq!(authorized.employee_id().as_str(), "cem");
    assert_eq!(authorized.employee_revision_id(), fixture.revision_id);
    assert_eq!(authorized.binding_id(), fixture.binding_id);
    assert_eq!(authorized.signer_ref().as_str(), SIGNER_REF);
    assert_eq!(*authorized.public_key(), fixture.public_key());
    assert_eq!(authorized.run_id(), fixture.run_id);
    assert_eq!(authorized.intent().kind, KIND_STREAM_MESSAGE);
    assert_eq!(authorized.intent().content, "Merhaba from Cem");
    let payload = authorized.payload();
    assert_eq!(payload.employee_id.as_str(), "cem");
    assert_eq!(payload.employee_revision_id, fixture.revision_id);
    assert_eq!(payload.public_key, fixture.public_key());
    // Replay is idempotent and yields the identical canonical object.
    fixture.replay(&authorized).await;
    assert_eq!(fixture.outbox_rows().await, 1);

    // A run that is not completed, or completed silently, cannot publish.
    for (status, intent) in [
        ("running", None),
        ("waiting", None),
        ("completed", Some("silent")),
        ("failed", None),
    ] {
        let run_id = insert_run(
            &fixture.pool,
            company_id,
            "cem",
            fixture.revision_id,
            status,
            intent,
        )
        .await;
        let error = fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(run_id))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, OfficeDeliveryError::RunNotPublishable { run_id: found, .. } if *found == run_id),
            "{status} / {intent:?}: {error:?}"
        );
    }

    // An unknown run and a run of another company are refused.
    let unknown = Uuid::new_v4();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unknown))
            .await
            .unwrap_err(),
        OfficeDeliveryError::UnknownRun { run_id } if run_id == unknown
    ));
    let other = Fixture::new().await;
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(other.run_id))
            .await
            .unwrap_err(),
        OfficeDeliveryError::UnknownRun { run_id } if run_id == other.run_id
    ));

    // Unverified and retired bindings fail closed.
    let (_, unverified_run) =
        employee_with_binding(&fixture, "zeynep", BindingShape::Unverified).await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unverified_run))
                .await
                .unwrap_err()
        ),
        BindingRejection::Unverified
    );
    let (_, retired_run) = employee_with_binding(&fixture, "ali", BindingShape::Retired).await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(retired_run))
                .await
                .unwrap_err()
        ),
        BindingRejection::Retired
    );

    // A run pinned to a revision whose key has no binding, and one pinned to
    // a revision that names another employee's key, are refused.
    let unbound_key = FakeOfficeSigner::new()
        .with_generated_signer("credential://office/unbound")
        .public_key("credential://office/unbound")
        .expect("generated");
    let unbound_revision = insert_revision(
        &fixture.pool,
        company_id,
        "cem",
        2,
        &unbound_key,
        SIGNER_REF,
    )
    .await;
    let unbound_run = insert_run(
        &fixture.pool,
        company_id,
        "cem",
        unbound_revision,
        "completed",
        Some("channel"),
    )
    .await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unbound_run))
                .await
                .unwrap_err()
        ),
        BindingRejection::Missing
    );
    let zeynep_key: Vec<u8> = sqlx::query(
        "SELECT public_key FROM employee_office_bindings WHERE company_id = $1 AND employee_id = 'zeynep'",
    )
    .bind(company_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("zeynep binding")
    .try_get("public_key")
    .expect("key");
    let zeynep_key = OfficePublicKey::parse_hex(&hex::encode(zeynep_key)).expect("key");
    let borrowed_revision =
        insert_revision(&fixture.pool, company_id, "cem", 3, &zeynep_key, SIGNER_REF).await;
    let borrowed_run = insert_run(
        &fixture.pool,
        company_id,
        "cem",
        borrowed_revision,
        "completed",
        Some("reply"),
    )
    .await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(borrowed_run))
                .await
                .unwrap_err()
        ),
        BindingRejection::WrongEmployee
    );

    // Kind policy: a profile event never reaches the outbox.
    let mut profile = fixture.draft_for(unbound_run);
    profile.kind = 0;
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &profile)
            .await
            .unwrap_err(),
        OfficeDeliveryError::Event(OfficeEventError::KindNotAllowed { kind: 0 })
    ));

    // None of the refusals created rows, signed, or published.
    assert_eq!(fixture.outbox_rows().await, 1);
    fixture.assert_untouched(authorized.outbox_id()).await;

    // A binding retired after enqueue is refused at delivery, before signing,
    // even though the authorized object was valid when it was issued.
    sqlx::query(
        "UPDATE employee_office_bindings SET valid_until = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(fixture.binding_id)
    .execute(&fixture.pool)
    .await
    .expect("retire binding");
    let lease = fixture.claim(Duration::from_secs(30)).await;
    assert_eq!(
        binding_rejection(
            fixture
                .service(Duration::from_secs(30))
                .deliver(&fixture.scope, &lease, &authorized)
                .await
                .unwrap_err()
        ),
        BindingRejection::Retired
    );
    fixture.assert_untouched(authorized.outbox_id()).await;
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn publish_failure_after_freeze_retries_identical_bytes_without_resigning() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::ZERO);

    // First attempt: signed and frozen, then the Office is unreachable.
    fixture.publisher.fail_next(1);
    let first = fixture.claim(Duration::from_secs(30)).await;
    let outcome = service
        .deliver(&fixture.scope, &first, &authorized)
        .await
        .expect("deliver");
    let DeliveryOutcome::Retrying {
        event_id: Some(event_id),
        ..
    } = outcome
    else {
        panic!("expected a retrying outcome with a frozen id, got {outcome:?}");
    };
    assert_eq!(fixture.signer.sign_calls(), 1);
    assert!(fixture.publisher.published().is_empty());
    let frozen = fixture.row(outbox_id).await;
    assert_eq!(frozen.state, "pending");
    assert_eq!(frozen.attempt_count, 1);
    assert_eq!(
        frozen.signed_event_id.as_deref(),
        Some(event_id.as_bytes().as_slice())
    );
    let frozen_bytes = frozen
        .signed_event_bytes
        .expect("bytes frozen before first publish");
    assert!(frozen.last_error.is_some());

    // Retry under a new lease from a replayed enqueue (a fresh process):
    // same bytes, same id, signer not invoked.
    let replayed = fixture.replay(&authorized).await;
    let second = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(second.lease_token, first.lease_token);
    assert_eq!(
        service
            .deliver(&fixture.scope, &second, &replayed)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            event_id,
            signed_now: false,
            receipt: PublishReceipt::Accepted,
        }
    );
    assert_eq!(fixture.signer.sign_calls(), 1);
    let published = fixture.publisher.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].event_id, event_id);
    assert_eq!(published[0].signed_bytes, frozen_bytes);
    let delivered = fixture.row(outbox_id).await;
    assert_eq!(delivered.state, "delivered");
    assert_eq!(
        delivered.signed_event_bytes.as_deref(),
        Some(frozen_bytes.as_slice())
    );
    fixture.nothing_due().await;
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn crash_after_freeze_is_recovered_by_the_next_lease_holder_without_resigning() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();

    // Worker A signs and freezes, then crashes before publishing. Its lease
    // is zero-length so the row is immediately reclaimable.
    let crashed = fixture.claim(Duration::ZERO).await;
    let signing = authorized
        .signing_request(Utc::now())
        .expect("signing request");
    let signed = fixture.signer.sign(&signing).await.expect("sign");
    let FreezeOutcome::Frozen(frozen) = fixture
        .control
        .freeze_signed_event(&fixture.scope, &crashed, &signed)
        .await
        .expect("freeze")
    else {
        panic!("lease is current, freeze must succeed");
    };
    assert_eq!(*frozen, signed);
    assert!(fixture.publisher.published().is_empty());

    // Worker B reclaims and publishes exactly the frozen event.
    let service = fixture.service(Duration::from_secs(30));
    let recovered = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(recovered.lease_token, crashed.lease_token);
    assert_eq!(
        service
            .deliver(&fixture.scope, &recovered, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            event_id: signed.event_id(),
            signed_now: false,
            receipt: PublishReceipt::Accepted,
        }
    );
    assert_eq!(fixture.signer.sign_calls(), 1);
    let published = fixture.publisher.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].signed_bytes, signed.signed_bytes());
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");

    // The crashed worker can no longer freeze anything into the row.
    assert_eq!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &crashed, &signed)
            .await
            .expect("stale freeze"),
        FreezeOutcome::StaleLease
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn stale_lease_cannot_freeze_or_publish() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::from_secs(30));

    let stale = fixture.claim(Duration::ZERO).await;
    let current = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(stale.lease_token, current.lease_token);

    assert_eq!(
        service
            .deliver(&fixture.scope, &stale, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::StaleLease
    );
    fixture.assert_untouched(outbox_id).await;

    // Even a correctly signed event cannot be frozen under the stale token.
    let signing = authorized
        .signing_request(Utc::now())
        .expect("signing request");
    let signed = fixture.signer.sign(&signing).await.expect("sign");
    assert_eq!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &stale, &signed)
            .await
            .expect("stale freeze"),
        FreezeOutcome::StaleLease
    );
    let row = fixture.row(outbox_id).await;
    assert!(row.signed_event_bytes.is_none());
    assert!(fixture.publisher.published().is_empty());

    // The current holder delivers normally and signs exactly once more.
    let outcome = service
        .deliver(&fixture.scope, &current, &authorized)
        .await
        .expect("deliver");
    assert!(
        matches!(
            outcome,
            DeliveryOutcome::Delivered {
                signed_now: true,
                receipt: PublishReceipt::Accepted,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(fixture.signer.sign_calls(), 2);
    assert_eq!(fixture.publisher.published().len(), 1);
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn cross_company_wrong_run_wrong_kind_and_mismatched_intent_fail_closed() {
    let fixture = Fixture::new().await;
    let other = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::from_secs(30));
    let lease = fixture.claim(Duration::from_secs(30)).await;

    // A draft scoped to another company is refused before any lookup.
    let mut foreign = fixture.draft();
    foreign.company_id = other.scope.company_id();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &foreign)
            .await
            .unwrap_err(),
        OfficeDeliveryError::CompanyMismatch { .. }
    ));

    // Another company's authorized publish cannot use this lease: it names
    // another run, and even a lease edited to name that run is a different
    // row than the one the publish was issued for. This company's publish
    // is refused under the other scope before any lookup.
    let through_other = other.enqueue().await;
    assert!(matches!(
        other
            .service(Duration::from_secs(30))
            .deliver(&other.scope, &lease, &through_other)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));
    let mut renamed = lease.clone();
    renamed.run_id = Some(other.run_id);
    assert!(matches!(
        other
            .control
            .frozen_event(&other.scope, &renamed, &through_other)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRow { expected, found }
            if expected == through_other.outbox_id() && found == lease.id
    ));
    assert!(matches!(
        other
            .service(Duration::from_secs(30))
            .deliver(&other.scope, &lease, &authorized)
            .await
            .unwrap_err(),
        OfficeDeliveryError::CompanyMismatch { .. }
    ));

    // Wrong run: another completed run of the same employee has its own row.
    let second_run = insert_run(
        &fixture.pool,
        fixture.scope.company_id(),
        "cem",
        fixture.revision_id,
        "completed",
        Some("reply"),
    )
    .await;
    let mut second_draft = fixture.draft_for(second_run);
    second_draft.content = "Something else".to_owned();
    let second = fixture
        .control
        .enqueue_office_publish(&fixture.scope, &second_draft)
        .await
        .expect("enqueue second run")
        .into_authorized();
    assert!(matches!(
        service
            .deliver(&fixture.scope, &lease, &second)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));

    // Wrong outbox kind.
    let mut wrong_kind = lease.clone();
    wrong_kind.kind = OutboxKind::RunDispatch;
    assert!(matches!(
        service
            .deliver(&fixture.scope, &wrong_kind, &authorized)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongKind { .. }
    ));

    // Different content for the pinned run is refused at enqueue, so no
    // authorized publish with a different intent can exist for this row.
    let mut different = fixture.draft();
    different.content = "Something else".to_owned();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &different)
            .await
            .unwrap_err(),
        OfficeDeliveryError::IntentMismatch { .. }
    ));

    // A signed event for another run cannot be frozen into this row.
    let signing = second.signing_request(Utc::now()).expect("signing request");
    let foreign_event = fixture.signer.sign(&signing).await.expect("sign");
    assert!(matches!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &lease, &foreign_event)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));

    let row = fixture.row(outbox_id).await;
    assert_eq!(row.state, "pending");
    assert!(row.signed_event_bytes.is_none());
    assert!(fixture.publisher.published().is_empty());
    assert!(other.publisher.published().is_empty());

    // The matching authorized publish still delivers under the same lease.
    assert!(matches!(
        service
            .deliver(&fixture.scope, &lease, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            signed_now: true,
            ..
        }
    ));
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");
}
