use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn cohort_removal_prevents_new_signing_and_republication_of_frozen_output() {
    for already_frozen in [false, true] {
        let f = Fixture::new().await;
        let authorized = f.enqueue().await;
        let lease = f.claim(Duration::from_secs(30)).await;
        if already_frozen {
            let signing = authorized
                .signing_request(Utc::now())
                .expect("signing request");
            let signed = f.signer.sign(&signing).await.expect("initial sign");
            f.control
                .freeze_signed_event(&f.scope, &lease, &signed)
                .await
                .expect("freeze");
            assert!(f.row(lease.id).await.signed_event_bytes.is_some());
        }
        let previous_calls = f.signer.sign_calls();
        sqlx::query(
            "DELETE FROM office_routing_employees WHERE company_id=$1 AND employee_id='cem'",
        )
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("remove cohort employee");
        assert!(
            matches!(f.service(Duration::from_secs(30)).deliver(&f.scope,&lease,&authorized).await,
            Err(OfficeDeliveryError::Control(ortak_control::ControlError::InvalidData(message)))
                if message=="office delivery authority is no longer valid")
        );
        assert_eq!(f.signer.sign_calls(), previous_calls);
        assert!(f.publisher.published().is_empty());
        assert_eq!(f.row(lease.id).await.state, "pending");
    }
}
