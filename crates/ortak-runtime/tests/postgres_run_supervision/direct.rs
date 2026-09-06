use super::*;
use ortak_runtime::office_output::{office_output_draft, schedule_office_outputs};
use ortak_runtime::reconciliation::reconcile_runtime;

#[path = "../../../ortak-control/tests/direct_channel_support.rs"]
mod support;

async fn channel(f: &Fixture) -> Uuid {
    let key: [u8; 32] = hex::decode(&f.employee.office.public_key)
        .unwrap()
        .try_into()
        .unwrap();
    let channel = support::create(&f.pool, f.community_id, &[[7; 32], key]).await;
    support::select(&f.control, &f.scope, channel, &f.employee.id).await;
    channel
}
async fn start(f: &Fixture, channel: Uuid) -> (Uuid, RuntimeRunRef) {
    f.route_kind(
        9,
        Some(channel),
        "A private request without an explicit employee mention",
    )
    .await;
    let lease = f.lease(Duration::from_secs(60)).await;
    match f
        .supervisor(f.config())
        .dispatch(&f.scope, &lease)
        .await
        .unwrap()
    {
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref,
        } => (run_id, runtime_run_ref),
        other => panic!("private DM dispatch refused: {other:?}"),
    }
}
async fn remove_human(f: &Fixture, channel: Uuid) {
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community_id).bind(channel).bind([7u8;32].as_slice()).execute(&f.pool).await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_runtime_admission_and_completed_output_keep_the_canonical_channel() {
    let f = Fixture::new().await;
    let channel = channel(&f).await;
    let (run, reference) = start(&f, channel).await;
    assert_eq!(f.adapter.start_specs().len(), 1);
    office_output::complete(
        &f,
        run,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("Private answer")],
    )
    .await;
    let report = schedule_office_outputs(&f.control, &f.scope, 64)
        .await
        .unwrap();
    assert_eq!((report.enqueued, report.failed), (1, 0));
    let draft = office_output_draft(&f.control, &f.scope, run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.tags[0], vec!["h".to_owned(), channel.to_string()]);
    assert_eq!(draft.content, "Private answer");
    assert_eq!(
        schedule_office_outputs(&f.control, &f.scope, 64)
            .await
            .unwrap()
            .enqueued,
        0
    );
    assert_eq!(office_output::output_count(&f, run).await, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_removed_participant_fences_queued_active_and_late_output() {
    for phase in 0..3 {
        let f = Fixture::new().await;
        let channel = channel(&f).await;
        if phase == 0 {
            f.route_kind(9, Some(channel), "Private input").await;
            let lease = f.lease(Duration::from_secs(60)).await;
            let admission = authorized(
                f.control
                    .authorize_dispatch(&f.scope, &lease)
                    .await
                    .unwrap(),
            );
            remove_human(&f, channel).await;
            assert_eq!(
                f.control.prepare_run(&f.scope, &admission).await.unwrap(),
                PrepareOutcome::Refused(DispatchRefusal::OfficeAuthorityChanged)
            );
            assert!(f.adapter.start_specs().is_empty());
        } else {
            let (run, reference) = start(&f, channel).await;
            if phase == 2 {
                office_output::complete(
                    &f,
                    run,
                    &reference,
                    DeliveryIntentKind::Reply,
                    vec![BoundedText::raw("Late private answer")],
                )
                .await;
            }
            remove_human(&f, channel).await;
            if phase == 1 {
                let stopped = reconcile_runtime(&f.control, &f.adapter, &f.scope, &f.config(), 64)
                    .await
                    .unwrap();
                assert_eq!((stopped.revocations, stopped.stop_attempts), (1, 1));
                assert_eq!(f.run(run).await.status, "cancelled");
                assert_eq!(
                    reconcile_runtime(&f.control, &f.adapter, &f.scope, &f.config(), 64)
                        .await
                        .unwrap()
                        .stop_attempts,
                    0
                );
            } else {
                let report = schedule_office_outputs(&f.control, &f.scope, 64)
                    .await
                    .unwrap();
                assert_eq!((report.enqueued, report.failed), (0, 1));
            }
            assert_eq!(office_output::output_count(&f, run).await, 0);
        }
    }
}
