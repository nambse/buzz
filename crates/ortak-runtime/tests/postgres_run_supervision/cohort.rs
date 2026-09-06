use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn cohort_removal_fences_queued_and_previously_authorized_runtime_dispatch() {
    for after_authorization in [false, true] {
        let f = Fixture::new().await;
        f.route("Cem, selam").await;
        let lease = f.lease(Duration::from_secs(60)).await;
        let authority = if after_authorization {
            Some(authorized(
                f.control
                    .authorize_dispatch(&f.scope, &lease)
                    .await
                    .expect("authorize"),
            ))
        } else {
            None
        };
        sqlx::query(
            "DELETE FROM office_routing_employees WHERE company_id=$1 AND employee_id='cem'",
        )
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("remove selected employee");
        if let Some(authority) = authority {
            assert_eq!(
                f.control
                    .prepare_run(&f.scope, &authority)
                    .await
                    .expect("prepare"),
                PrepareOutcome::Refused(DispatchRefusal::OfficeAuthorityChanged)
            );
        } else {
            assert!(matches!(
                f.supervisor(f.config())
                    .dispatch(&f.scope, &lease)
                    .await
                    .expect("dispatch"),
                DispatchOutcome::Refused {
                    refusal: DispatchRefusal::OfficeAuthorityChanged,
                    ..
                }
            ));
        }
        assert_eq!(f.run_rows().await, 0);
        assert!(f.adapter.start_specs().is_empty());
    }
}
