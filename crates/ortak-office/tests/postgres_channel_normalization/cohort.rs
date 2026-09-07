use super::*;
use ortak_control::ports::{MessageNormalizer, Normalization};
use ortak_domain::EmployeeId;
use ortak_office::PgChannelNormalizer;

use crate::cohort_support;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn central_selection_intersects_employee_eligibility_and_refuses_known_outside_target() {
    let f = Fixture::new().await;
    cohort_support::select_and_reconcile(
        &f.control,
        &f.scope,
        &[f.channel_id],
        &[EmployeeId::parse("zeynep").expect("id")],
    )
    .await;
    let (event, outcome) = f.route_human_text("Cem, selam").await;
    let decision = committed(outcome);
    assert_silent(&decision, RoutingReason::TargetNotChannelMember);
    assert_eq!(f.run_dispatch_rows(event.id).await, 0);
    let (_, outcome) = f.route_human_text("Zeynep, selam").await;
    let decision = committed(outcome);
    assert_eq!(decision.wake_count, 1);
    assert_eq!(decision.dispatches[0].employee_id, "zeynep");
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn disabled_or_missing_cohort_refuses_canonical_normalization() {
    for remove in [false, true] {
        let f = Fixture::new().await;
        let event = f
            .store_event(EventSpec {
                kind: KIND_STREAM_MESSAGE,
                author: f.human_key,
                content: "Cem, selam",
                tags: serde_json::json!([["h", f.channel_id.to_string()]]),
                channel_id: Some(f.channel_id),
                parent: None,
            })
            .await;
        f.accept(&event, KIND_STREAM_MESSAGE, f.human_key).await;
        if remove {
            sqlx::query("DELETE FROM office_routing_cohorts WHERE company_id=$1")
                .bind(f.company_id())
                .execute(&f.pool)
                .await
                .expect("remove cohort");
        } else {
            f.control
                .disable_routing_cohort(&f.scope)
                .await
                .expect("disable");
        }
        let inbox = f
            .control
            .inbox_row(&f.scope, event.id)
            .await
            .expect("inbox")
            .expect("exists");
        assert!(
            matches!(PgChannelNormalizer::new(f.pool.clone()).normalize(&f.scope,&inbox).await.expect("normalize"),
            Normalization::Refused(ref refusal) if refusal.reason==RoutingReason::ChannelNotRoutable)
        );
        assert_eq!(f.run_dispatch_rows(event.id).await, 0);
    }
}
