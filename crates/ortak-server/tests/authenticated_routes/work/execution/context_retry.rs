use super::*;
use ortak_control::run_event::RedactionPolicy;
use ortak_runtime::memory_context::{
    FreezeSnapshotOutcome, FrozenRunSnapshot, RunContextRepository, RunMemory,
};
use ortak_runtime::{
    DispatchAuthority, DispatchAuthorization, DispatchRefusal, RunDispatchRepository,
};

struct NoSecondRecall;
impl RunMemory for NoSecondRecall {
    async fn check(&self, _: &DispatchAuthority) -> Result<(), DispatchRefusal> {
        Ok(())
    }
    async fn snapshot(
        &self,
        _: &DispatchAuthority,
        _: Uuid,
        _: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        panic!("a frozen retry cannot recall or reselect context")
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn frozen_work_retry_keeps_bytes_and_rechecks_history_project_and_employee_before_start() {
    for revoke in ["none", "history", "project", "employee"] {
        let f = Fixture::new().await;
        employee(&f).await;
        let app = work_app(&f, true, Role::Reader, vec![f.channel]);
        let (project, item) = ready(&f, &app).await;
        let source = item["source_message_id"].as_str().unwrap();
        let selected = reply(&f, source, "Reference selected before start").await;
        let (run, _) = queue(&f, &app, &item).await;
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        let lease = f
            .control
            .claim_runtime_dispatches(
                &scope,
                "fake-runtime",
                "work-context-retry",
                Duration::from_secs(60),
                1,
            )
            .await
            .unwrap()
            .remove(0);
        let DispatchAuthorization::Authorized(authority) =
            f.control.authorize_dispatch(&scope, &lease).await.unwrap()
        else {
            panic!("current request must authorize")
        };
        // Empty scratch recall is a controlled adapter result. The production
        // repository must select and validate the actual Work references.
        let candidate = json!({"version":2,"company_id":f.company,"event_kind":authority.input().event_kind,
            "input_truncated":authority.input().truncated,"memory_binding":authority.memory_binding(),
            "recall":ortak_control::memory::MemoryRecall::default(),"spec":authority.run_spec(run).unwrap(),
            "work_origin":authority.work_origin()});
        let candidate =
            FrozenRunSnapshot::decode(&serde_json::to_vec(&candidate).unwrap(), &authority, run)
                .unwrap();
        let mut injected: Value = serde_json::from_slice(&candidate.encode().unwrap()).unwrap();
        let mut supplied: Value = serde_json::from_str(include_str!(
            "../../../../../ortak-control/src/work_context/test_vector.json"
        ))
        .unwrap();
        supplied["snapshot_id"] = json!(run);
        supplied["work_item_id"] = json!(id(&item));
        supplied["project_id"] = json!(project);
        supplied["requested_version"] = json!(version(&item));
        supplied["execution_version"] = json!(version(&item) + 1);
        supplied["employee"]["employee_id"] = json!(authority.employee_id());
        supplied["employee"]["revision_id"] = json!(authority.employee_revision_id());
        supplied["prior_artifact"] = Value::Null;
        injected["spec"]["context"]["work_context"] = supplied;
        let injected =
            FrozenRunSnapshot::decode(&serde_json::to_vec(&injected).unwrap(), &authority, run)
                .unwrap();
        assert!(
            matches!(
                f.control
                    .freeze_run_snapshot(&scope, &lease, &authority, run, &injected)
                    .await
                    .unwrap(),
                FreezeSnapshotOutcome::Refused(DispatchRefusal::MemoryContextRejected)
            ),
            "an adapter cannot substitute its own Work sources or employee facts"
        );
        let FreezeSnapshotOutcome::Ready(winner) = f
            .control
            .freeze_run_snapshot(&scope, &lease, &authority, run, &candidate)
            .await
            .unwrap()
        else {
            panic!("first snapshot must freeze")
        };
        assert_eq!(
            winner
                .spec()
                .context
                .work_context
                .as_ref()
                .unwrap()
                .messages
                .len(),
            2
        );
        let bytes = winner.encode().unwrap();
        reply(&f, source, "Newer reference after the snapshot").await;
        match revoke {
            "history" => {
                sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=decode($2,'hex')")
                .bind(f.community).bind(&selected).execute(&f.pool).await.unwrap();
            }
            "project" => {
                sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
                .bind(f.company).bind(project).bind(f.operator.public_key().to_hex()).execute(&f.pool).await.unwrap();
            }
            "employee" => {
                sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND role='bot'")
                .bind(f.community).bind(f.channel).execute(&f.pool).await.unwrap();
            }
            _ => {}
        }
        let runtime = FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
        let outcome = RunSupervisor::new(f.control.clone(), &runtime, SupervisorConfig::default())
            .with_run_memory(NoSecondRecall)
            .dispatch(&scope, &lease)
            .await;
        if revoke == "none" {
            assert!(matches!(outcome.unwrap(), DispatchOutcome::Started { .. }));
            assert_eq!(runtime.start_specs(), vec![winner.spec().clone()]);
        } else {
            assert!(
                !matches!(outcome, Ok(DispatchOutcome::Started { .. })),
                "{revoke}"
            );
            assert!(runtime.start_specs().is_empty(), "{revoke}");
        }
        let stored: Vec<u8> = sqlx::query_scalar(
            "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
        )
        .bind(f.company)
        .bind(run)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(
            stored, bytes,
            "{revoke}: authorization cannot rewrite evidence"
        );
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn unlinked_work_does_not_receive_nearby_office_history_or_another_items_artifact() {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (project, other) = ready(&f, &app).await;
    let (first, _) = queue(&f, &app, &other).await;
    let (runtime, memory, reference) = start(&f, &employee, first).await;
    complete(
        &f,
        &runtime,
        &memory,
        first,
        &reference,
        BoundedText::raw("Different Work output"),
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    assert_eq!(
        schedule_work_outputs(&f.control, &scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    let unlinked = item(&f, &app, project).await;
    let (status, assigned) = post(&app, &f.operator, &format!("/api/v1/work-items/{}/assignments", id(&unlinked)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&unlinked),"employee_id":"cem","role":"owner"})).await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    let ready = transition(&f, &app, assigned["work_item"].clone(), "ready").await;
    let (run, _) = queue(&f, &app, &ready).await;
    let (runtime, _, _) = start(&f, &employee, run).await;
    let specs = runtime.start_specs();
    let context = specs[0].context.work_context.as_ref().unwrap();
    assert!(context.source_message_id.is_none());
    assert!(context.thread_root_message_id.is_none());
    assert!(context.messages.is_empty());
    assert!(!context.omitted_history);
    assert!(context.prior_artifact.is_none());
    assert_eq!(context.employee.employee_id, employee.id);
}
