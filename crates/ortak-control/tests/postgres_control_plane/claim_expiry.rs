//! Production routing must not turn an expired same-generation claim into a wake.
use super::*;
use ortak_control::inbox::InboxClaim;

async fn fixture() -> Fixture {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL").expect("explicit disposable database URL");
    let options: sqlx::postgres::PgConnectOptions = url.parse().expect("database URL");
    assert!(matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432);
    Fixture::new(RoutingPolicy::default(), vec![fixture_employee("cem")]).await
}

fn normalizer(id: MessageId) -> FakeNormalizer {
    FakeNormalizer {
        messages: HashMap::from([(
            id,
            NormalizedMessage {
                envelope: MessageEnvelope::human_channel(
                    id.to_hex(),
                    "human",
                    "office",
                    "A general question for the team",
                ),
                root_message_id: id,
                eligible_employee_ids: [employee_id("cem")].into_iter().collect(),
            },
        )]),
    }
}

struct Scorer {
    calls: Arc<AtomicU32>,
    shorten: Option<(PgPool, Uuid, bool)>,
}
impl SemanticScorer for Scorer {
    fn metadata(&self) -> ScorerMetadata {
        ScorerMetadata {
            adapter: "claim-expiry-fixture".to_owned(),
            model: None,
            prompt_version: None,
            version: "v1".to_owned(),
            latency_ms: None,
            usage: None,
        }
    }

    async fn score(&self, input: &SemanticScoringInput) -> ScoringOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((pool, company, hang)) = &self.shorten {
            sqlx::query("UPDATE office_inbox SET claim_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND event_id=$2")
                .bind(company).bind(input.message_id().as_bytes().as_slice())
                .execute(pool).await.expect("shorten live lease without reclaiming");
            if *hang {
                return std::future::pending().await;
            }
        }
        ScoringOutcome {
            result: Ok(input
                .request()
                .candidates()
                .iter()
                .map(|candidate| SemanticScore {
                    employee_id: candidate.employee_id().clone(),
                    score: 0.99,
                    evidence: vec![EvidenceLabel::parse("role_match").expect("fixed evidence")],
                })
                .collect()),
            metadata: self.metadata(),
        }
    }
}

async fn claim(f: &Fixture, id: MessageId, duration: Duration) -> InboxClaim {
    f.control
        .claim_message(&f.scope, id, "claim-expiry-fixture", duration, 5)
        .await
        .expect("claim")
        .expect("new accepted message")
}

async fn wait_until_blocked(pool: &PgPool, blocker_pid: i32) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_stat_activity
                  WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(blocker_pid)
            .fetch_one(pool)
            .await
            .expect("observe production lock wait");
            if blocked {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("production routing must reach the held lock while its claim is live");
}

async fn wait_until_expired(pool: &PgPool, claim: &InboxClaim) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                .bind(claim.claim_expires_at)
                .fetch_one(pool)
                .await
                .expect("observe database lease clock");
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("short claim must expire within the bounded test");
}

async fn no_wake(f: &Fixture) {
    for query in [
        "SELECT count(*) FROM outbox WHERE company_id=$1 AND $2::bytea IS NULL",
        "SELECT count(*) FROM delivery_chain_visits WHERE company_id=$1 AND $2::bytea IS NULL",
    ] {
        assert_eq!(f.count(query, None).await, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL on localhost:55432"]
async fn score_waiting_for_root_cannot_wake_after_claim_expiry_without_reclaim() {
    let f = fixture().await;
    let id = message_id();
    f.accept(id).await;
    let policy = RoutingPolicy::default();
    sqlx::query("INSERT INTO delivery_chains(company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(f.scope.company_id()).bind(id.as_bytes().as_slice()).bind(&policy.version)
        .bind(policy.fingerprint()).bind(i16::from(policy.max_hops)).bind(policy.max_chain_wakes as i32)
        .execute(&f.pool).await.expect("existing root");
    let mut blocker = f.pool.begin().await.expect("root blocker");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("blocker pid");
    sqlx::query("SELECT root_message_id FROM delivery_chains WHERE company_id=$1 AND root_message_id=$2 FOR UPDATE")
        .bind(f.scope.company_id()).bind(id.as_bytes().as_slice())
        .fetch_one(&mut *blocker).await.expect("hold root");
    let claim = claim(&f, id, Duration::from_millis(500)).await;
    let calls = Arc::new(AtomicU32::new(0));
    let service = InboxRoutingService::new(
        f.control.clone(),
        normalizer(id),
        Scorer {
            calls: calls.clone(),
            shorten: None,
        },
        RoutingWorkerConfig::default(),
    );
    let scope = f.scope.clone();
    let held_claim = claim.clone();
    let task = tokio::spawn(async move { service.route_claim(&scope, &held_claim).await });
    wait_until_blocked(&f.pool, pid).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "score must precede the lock wait"
    );
    wait_until_expired(&f.pool, &claim).await;
    blocker
        .rollback()
        .await
        .expect("release root after lease expiry");
    let outcome = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("prompt release")
        .expect("routing task")
        .expect("stale claim outcome");
    assert!(matches!(outcome, ServiceOutcome::StaleClaim));
    let inbox = f
        .control
        .inbox_row(&f.scope, id)
        .await
        .expect("inbox")
        .expect("row");
    assert_eq!(
        inbox.claim_generation, claim.claim_generation,
        "no competing reclaim"
    );
    assert_eq!(inbox.state, InboxState::Claimed);
    assert_eq!(
        f.count(
            "SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND $2::bytea IS NULL",
            None
        )
        .await,
        0
    );
    no_wake(&f).await;
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL on localhost:55432"]
async fn shortened_live_lease_refuses_scores_but_preserves_timeout_silence() {
    for hang in [false, true] {
        let f = fixture().await;
        let id = message_id();
        f.accept(id).await;
        let claim = claim(&f, id, Duration::from_secs(60)).await;
        let calls = Arc::new(AtomicU32::new(0));
        let service = InboxRoutingService::new(
            f.control.clone(),
            normalizer(id),
            Scorer {
                calls: calls.clone(),
                shorten: Some((f.pool.clone(), f.scope.company_id(), hang)),
            },
            RoutingWorkerConfig {
                semantic_timeout: Duration::from_millis(100),
                ..RoutingWorkerConfig::default()
            },
        );
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            service.route_claim(&f.scope, &claim),
        )
        .await
        .expect("bounded routing")
        .expect("routing outcome");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if hang {
            let ServiceOutcome::Committed(decision) = outcome else {
                panic!("expired same-generation timeout must finalize silence")
            };
            assert_eq!(decision.wake_count, 0);
            assert_eq!(
                decision.summary_reason,
                RoutingReason::SemanticScorerTimedOut
            );
        } else {
            assert!(matches!(outcome, ServiceOutcome::StaleClaim));
            assert_eq!(f.count("SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND $2::bytea IS NULL", None).await, 0);
        }
        let inbox = f
            .control
            .inbox_row(&f.scope, id)
            .await
            .expect("inbox")
            .expect("row");
        assert_eq!(inbox.claim_generation, claim.claim_generation);
        no_wake(&f).await;
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL on localhost:55432"]
async fn expiry_after_production_decision_insert_is_rejected_at_deferred_commit() {
    let f = fixture().await;
    let id = message_id();
    f.accept(id).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let trigger = format!("test_routing_wait_{suffix}");
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into().expect("8 bytes"));
    // A fixture-scoped AFTER INSERT barrier runs inside the actual repository
    // transaction, after its live pre-write check and before deferred COMMIT.
    // Dynamic SQL contains only generated typed UUIDs and an integer lock key.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {trigger}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
           IF NEW.company_id='{}'::uuid THEN
             PERFORM set_config('lock_timeout','2s',true);
             PERFORM pg_advisory_xact_lock({key}::bigint);
           END IF; RETURN NEW; END $$;
         CREATE TRIGGER {trigger} AFTER INSERT ON routing_decisions
         FOR EACH ROW EXECUTE FUNCTION {trigger}();",
        f.scope.company_id()
    )))
    .execute(&f.pool)
    .await
    .expect("bounded decision barrier");
    let mut blocker = f.pool.begin().await.expect("decision barrier holder");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *blocker)
        .await
        .expect("hold barrier");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("blocker pid");
    let claim = claim(&f, id, Duration::from_millis(500)).await;
    let service = InboxRoutingService::new(
        f.control.clone(),
        normalizer(id),
        Scorer {
            calls: Arc::new(AtomicU32::new(0)),
            shorten: None,
        },
        RoutingWorkerConfig::default(),
    );
    let scope = f.scope.clone();
    let held_claim = claim.clone();
    let task = tokio::spawn(async move { service.route_claim(&scope, &held_claim).await });
    wait_until_blocked(&f.pool, pid).await;
    wait_until_expired(&f.pool, &claim).await;
    blocker
        .rollback()
        .await
        .expect("release after decision's lease expires");
    let result = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("bounded deferred commit")
        .expect("routing task");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {trigger} ON routing_decisions; DROP FUNCTION {trigger}();"
    )))
    .execute(&f.pool)
    .await
    .expect("remove only the owned fixture barrier");
    let ControlError::Database(error) =
        result.expect_err("deferred constraint must reject waking decision")
    else {
        panic!("expected database serialization failure")
    };
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("40001")
    );
    assert_eq!(
        f.count(
            "SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND $2::bytea IS NULL",
            None
        )
        .await,
        0
    );
    assert_eq!(
        f.count(
            "SELECT count(*) FROM delivery_chains WHERE company_id=$1 AND $2::bytea IS NULL",
            None
        )
        .await,
        0
    );
    no_wake(&f).await;
}
