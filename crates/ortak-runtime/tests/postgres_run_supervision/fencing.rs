use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn revoked_office_membership_prevents_queued_runtime_admission() {
    for mutate_after_authorization in [false, true] {
        let fixture = Fixture::new().await;
        fixture.route("Cem, selam").await;
        let lease = fixture.lease(Duration::from_secs(60)).await;
        let authority = if mutate_after_authorization {
            Some(authorized(
                fixture
                    .control
                    .authorize_dispatch(&fixture.scope, &lease)
                    .await
                    .expect("authorize"),
            ))
        } else {
            None
        };
        sqlx::query(
            "UPDATE channel_members SET removed_at = now() WHERE community_id = $1 AND pubkey = $2",
        )
        .bind(fixture.community_id)
        .bind(hex::decode(fixture_employee().office.public_key).expect("employee key"))
        .execute(&fixture.pool)
        .await
        .expect("revoke membership");
        if let Some(authority) = authority {
            assert_eq!(
                fixture
                    .control
                    .prepare_run(&fixture.scope, &authority)
                    .await
                    .expect("prepare"),
                PrepareOutcome::Refused(DispatchRefusal::OfficeAuthorityChanged)
            );
        } else {
            assert!(matches!(
                fixture
                    .supervisor(fixture.config())
                    .dispatch(&fixture.scope, &lease)
                    .await
                    .expect("dispatch"),
                DispatchOutcome::Refused {
                    refusal: DispatchRefusal::OfficeAuthorityChanged,
                    ..
                }
            ));
        }
        assert_eq!(fixture.run_rows().await, 0);
        assert!(fixture.adapter.start_specs().is_empty());
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn cached_company_scope_cannot_admit_work_after_suspension() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    sqlx::query("UPDATE companies SET status = 'suspended' WHERE id = $1")
        .bind(fixture.scope.company_id())
        .execute(&fixture.pool)
        .await
        .expect("suspend company after scope and decision exist");
    assert_eq!(
        fixture
            .supervisor(fixture.config())
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("durable refusal"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::CompanyNotActive,
            retry: OutboxFailOutcome::Retrying,
        }
    );
    assert_eq!(fixture.run_rows().await, 0);
    assert!(fixture.adapter.start_specs().is_empty());
    assert_eq!(fixture.outbox(lease.id).await.attempt_count, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn office_key_and_signer_rotation_refuse_the_old_pinned_run() {
    for rotate_key in [true, false] {
        let fixture = Fixture::new().await;
        let decision_id = fixture.route("Cem, selam").await;
        let lease = fixture.lease(Duration::from_secs(60)).await;
        let mut newer = fixture_employee();
        let previous_key = hex::decode(&newer.office.public_key).expect("old key");
        newer.permissions = PermissionPolicy::default();
        if rotate_key {
            newer.office.public_key = message_id().to_hex();
            sqlx::query("UPDATE employee_office_bindings SET valid_until = clock_timestamp() WHERE company_id = $1")
                .bind(fixture.scope.company_id()).execute(&fixture.pool).await.expect("retire old key");
        } else {
            newer.office.signer_ref =
                ortak_domain::CredentialRef::parse("credential://test/rotated-signer")
                    .expect("signer reference");
        }
        let revision =
            activate_employee(&fixture.pool, fixture.scope.company_id(), &newer, true).await;
        assert_ne!(revision, fixture.revision_id);
        if rotate_key {
            sqlx::query("INSERT INTO channel_members (community_id, channel_id, pubkey) SELECT community_id, channel_id, $3 FROM channel_members WHERE community_id = $1 AND pubkey = $2")
                .bind(fixture.community_id).bind(&previous_key)
                .bind(hex::decode(&newer.office.public_key).expect("new key"))
                .execute(&fixture.pool).await.expect("new key joins same channel");
            sqlx::query("UPDATE channel_members SET removed_at = clock_timestamp() WHERE community_id = $1 AND pubkey = $2")
                .bind(fixture.community_id).bind(&previous_key).execute(&fixture.pool).await.expect("remove old key");
        } else {
            sqlx::query(
                "UPDATE employee_office_bindings SET signer_ref = $2 WHERE company_id = $1",
            )
            .bind(fixture.scope.company_id())
            .bind(newer.office.signer_ref.as_str())
            .execute(&fixture.pool)
            .await
            .expect("new signer matches active manifest");
        }
        // Both rotations preserve the normalized eligible employee set. This
        // proves the new identity guard is required independently of the hash.
        let decision = sqlx::query("SELECT message_id, office_input_hash FROM routing_decisions WHERE company_id = $1 AND id = $2")
            .bind(fixture.scope.company_id()).bind(decision_id).fetch_one(&fixture.pool).await.expect("decision");
        let message =
            MessageId::try_from_slice(&decision.get::<Vec<u8>, _>("message_id")).expect("message");
        let snapshot = fixture
            .control
            .routing_snapshot(&fixture.scope, message)
            .await
            .expect("snapshot")
            .expect("inbox");
        let Normalization::Message(normalized) =
            ortak_office::PgChannelNormalizer::new(fixture.pool.clone())
                .normalize(&fixture.scope, &snapshot.inbox)
                .await
                .expect("normalize rotated identity")
        else {
            panic!("rotation remains channel-eligible");
        };
        let current_hash = ortak_control::service::office_input_hash(
            &normalized.envelope,
            normalized.root_message_id,
            &normalized.eligible_employee_ids,
        );
        assert_eq!(
            decision.get::<Vec<u8>, _>("office_input_hash"),
            current_hash
        );
        assert!(matches!(
            fixture
                .supervisor(fixture.config())
                .dispatch(&fixture.scope, &lease)
                .await
                .expect("dispatch refusal"),
            DispatchOutcome::Refused {
                refusal: DispatchRefusal::OfficeAuthorityChanged,
                ..
            }
        ));
        assert!(fixture.adapter.start_specs().is_empty());
        assert_eq!(fixture.run_rows().await, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn repeated_admission_waiting_past_the_same_witness_expiry_cannot_start_again() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let deadline: chrono::DateTime<Utc> = sqlx::query_scalar(
        "UPDATE employee_office_bindings SET valid_until = clock_timestamp() + interval '2 seconds' WHERE company_id = $1 RETURNING valid_until",
    ).bind(fixture.scope.company_id()).fetch_one(&fixture.pool).await.expect("bounded validity window");
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease)
            .await
            .expect("first authorization"),
    );
    let PrepareOutcome::Prepared(prepared) = fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("initial admission")
    else {
        panic!("initial admission should prepare a run");
    };
    fixture
        .adapter
        .start_run(&authority.run_spec(prepared.run_id).expect("spec"))
        .await
        .expect("start with lost acknowledgement");
    let previous_token: Uuid = sqlx::query_scalar(
        "SELECT office_admission_token FROM runs WHERE company_id = $1 AND id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(prepared.run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("first token");
    let mut blocker = fixture.pool.begin().await.expect("row blocker");
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("backend pid");
    sqlx::query("SELECT id FROM runs WHERE company_id = $1 AND id = $2 FOR UPDATE")
        .bind(fixture.scope.company_id())
        .bind(prepared.run_id)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold existing run row");
    let supervisor = fixture.supervisor(fixture.config());
    let release_after_expiry = async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid)))",
                ).bind(blocker_pid).fetch_one(&fixture.pool).await.expect("observe blocked preparation");
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("second prepare must actually block on the existing row");
        // Wait against the authoritative database deadline after observing
        // the lock wait, rather than guessing when the dispatcher got there.
        sqlx::query("SELECT pg_sleep(GREATEST(0, EXTRACT(EPOCH FROM ($1::timestamptz - clock_timestamp()))) + 0.01)")
            .bind(deadline).execute(&fixture.pool).await.expect("wait through known binding expiry");
        blocker.rollback().await.expect("release row");
    };
    let (outcome, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            supervisor.dispatch(&fixture.scope, &lease),
            release_after_expiry
        )
    })
    .await
    .expect("bounded admission race");
    let Err(RunSupervisionError::Control(ortak_control::ControlError::Database(
        sqlx::Error::Database(error),
    ))) = outcome
    else {
        panic!("expired repeated admission must fail the deferred database guard: {outcome:?}");
    };
    assert_eq!(error.code().as_deref(), Some("40001"));
    assert_eq!(
        fixture.adapter.start_specs().len(),
        1,
        "no second runtime start after expiry"
    );
    assert_eq!(fixture.run(prepared.run_id).await.status, "queued");
    let durable_token: Uuid = sqlx::query_scalar(
        "SELECT office_admission_token FROM runs WHERE company_id = $1 AND id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(prepared.run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("unchanged token after rollback");
    assert_eq!(durable_token, previous_token);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn canonical_hash_without_original_office_witness_cannot_start_runtime() {
    let fixture = Fixture::new().await;
    let name = format!("test_missing_witness_{}", Uuid::new_v4().simple());
    // Preserve all canonical routing/recipient/visit facts while recreating the
    // supported legacy absence of a routing-time witness. Only this generated
    // company is affected; immutable production rows are never updated.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
           IF NEW.company_id='{}'::uuid THEN
             NEW.office_authority_generation := NULL;
             NEW.office_authority_valid_before := NULL;
           END IF; RETURN NEW; END $$;
         CREATE TRIGGER {name} BEFORE INSERT ON routing_decisions
         FOR EACH ROW EXECUTE FUNCTION {name}();",
        fixture.scope.company_id()
    )))
    .execute(&fixture.pool)
    .await
    .expect("scoped legacy fixture");
    let decision_id = fixture.route("Cem, selam").await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON routing_decisions; DROP FUNCTION {name}();"
    )))
    .execute(&fixture.pool)
    .await
    .expect("remove owned fixture trigger");
    let decision = sqlx::query("SELECT message_id,office_input_hash,office_authority_generation FROM routing_decisions WHERE company_id=$1 AND id=$2")
        .bind(fixture.scope.company_id()).bind(decision_id)
        .fetch_one(&fixture.pool).await.expect("original canonical decision");
    assert_eq!(
        decision.get::<Option<i64>, _>("office_authority_generation"),
        None
    );
    let message = MessageId::try_from_slice(&decision.get::<Vec<u8>, _>("message_id"))
        .expect("canonical message");
    let snapshot = fixture
        .control
        .routing_snapshot(&fixture.scope, message)
        .await
        .expect("current snapshot")
        .expect("inbox");
    let Normalization::Message(normalized) =
        ortak_office::PgChannelNormalizer::new(fixture.pool.clone())
            .normalize(&fixture.scope, &snapshot.inbox)
            .await
            .expect("canonical normalization")
    else {
        panic!("fixture must remain authorized by canonical Office facts")
    };
    let current_hash = ortak_control::service::office_input_hash(
        &normalized.envelope,
        normalized.root_message_id,
        &normalized.eligible_employee_ids,
    );
    assert_eq!(
        decision.get::<Vec<u8>, _>("office_input_hash"),
        current_hash
    );
    let lease = fixture.lease(Duration::from_secs(60)).await;
    assert!(matches!(
        fixture
            .supervisor(fixture.config())
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("durable legacy refusal"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::OfficeAuthorityChanged,
            ..
        }
    ));
    assert_eq!(fixture.run_rows().await, 0);
    assert!(fixture.adapter.start_specs().is_empty());
}
