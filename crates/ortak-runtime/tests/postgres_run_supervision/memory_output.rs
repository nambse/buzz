use super::*;

use ortak_control::memory::{MemoryAdapter, MemoryBudget, MemoryRecallRequest, MemoryScope};
use ortak_domain::CredentialRef;
use ortak_office::fakes::{FakeOfficePublisher, FakeOfficeSigner};
use ortak_office::{DeliveryConfig, OfficeDeliveryService};
use ortak_runtime::memory_output::schedule_memory_output;
use ortak_runtime::office_delivery::deliver_one_office_output;
use ortak_runtime::office_output::schedule_office_outputs;

const SIGNER: &str = "credential://office/memory-test";

async fn signed_fixture() -> (Fixture, FakeOfficeSigner) {
    let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER);
    let mut employee = fixture_employee();
    employee.office.public_key = signer.public_key(SIGNER).expect("fresh signer").to_hex();
    employee.office.signer_ref = CredentialRef::parse(SIGNER).expect("opaque signer ref");
    employee
        .memory
        .as_mut()
        .expect("fixture memory binding")
        .adapter = "fake-memory".to_owned();
    (Fixture::new_for_employee(employee).await, signer)
}

async fn completed_output(fixture: &Fixture) -> Uuid {
    let (run_id, reference, _) = fixture.started().await;
    super::office_output::complete(
        fixture,
        run_id,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("The actual published answer.\n")],
    )
    .await;
    let report = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("freeze output");
    assert_eq!(report.enqueued, 1);
    run_id
}

async fn recall(fixture: &Fixture, run_id: Uuid) -> ortak_control::memory::MemoryRecall {
    fixture
        .memory
        .recall(&MemoryRecallRequest {
            employee_id: fixture.employee.id.clone(),
            binding: fixture
                .employee
                .memory
                .clone()
                .expect("fixture memory binding"),
            scope: MemoryScope::RunScratch { run_id },
            query: "published answer".into(),
            budget: MemoryBudget::default(),
        })
        .await
        .expect("inspect scoped fake memory")
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn memory_output_waits_for_real_signed_delivery_and_retries_the_frozen_answer() {
    let (fixture, signer) = signed_fixture().await;
    let run_id = completed_output(&fixture).await;
    assert_eq!(
        schedule_memory_output(&fixture.control, &fixture.memory, &fixture.scope)
            .await
            .expect("before acknowledgement")
            .attempted,
        0
    );
    let publisher = FakeOfficePublisher::new();
    let delivery = OfficeDeliveryService::new(
        fixture.control.clone(),
        &signer,
        &publisher,
        DeliveryConfig::default(),
    );
    assert!(
        deliver_one_office_output(&fixture.control, &fixture.scope, "memory-test", &delivery)
            .await
            .expect("actual signed Office delivery")
    );
    assert_eq!(signer.sign_calls(), 1);
    fixture.memory.set_unavailable(true);
    let failed = schedule_memory_output(&fixture.control, &fixture.memory, &fixture.scope)
        .await
        .expect("durable service retry");
    assert_eq!(
        (
            failed.attempted,
            failed.acknowledged,
            failed.failed_attempts
        ),
        (1, 0, 1)
    );
    let state: (String, String) = sqlx::query_as(
        "SELECT state,last_error_code FROM runtime_memory_writes WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("retry remains visible");
    assert_eq!(
        state,
        ("pending".into(), "memory_output_service_retry".into())
    );
    sqlx::query("UPDATE runtime_memory_writes SET next_attempt_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect("make retry due");
    fixture.memory.set_unavailable(false);
    let written = schedule_memory_output(&fixture.control, &fixture.memory, &fixture.scope)
        .await
        .expect("retry same frozen request");
    assert_eq!((written.attempted, written.acknowledged), (1, 1));
    let records = recall(&fixture, run_id).await.records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].content, "The actual published answer.\n");
    assert_eq!(records[0].provenance.run_id, Some(run_id));
    assert!(records[0].provenance.source.starts_with("office:"));
    assert_eq!(
        schedule_memory_output(&fixture.control, &fixture.memory, &fixture.scope)
            .await
            .expect("completed replay")
            .attempted,
        0
    );
    assert_eq!(recall(&fixture, run_id).await.records, records);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn memory_output_revalidates_source_membership_after_office_acknowledgement() {
    let (fixture, signer) = signed_fixture().await;
    let run_id = completed_output(&fixture).await;
    let publisher = FakeOfficePublisher::new();
    let delivery = OfficeDeliveryService::new(
        fixture.control.clone(),
        &signer,
        &publisher,
        DeliveryConfig::default(),
    );
    deliver_one_office_output(&fixture.control, &fixture.scope, "memory-test", &delivery)
        .await
        .expect("signed acknowledgement");
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND pubkey=$2")
        .bind(fixture.community_id).bind(hex::decode(&fixture.employee.office.public_key).expect("public key"))
        .execute(&fixture.pool).await.expect("revoke original channel membership");
    let report = schedule_memory_output(&fixture.control, &fixture.memory, &fixture.scope)
        .await
        .expect("refuse changed authority");
    assert_eq!((report.acknowledged, report.failed_attempts), (0, 1));
    let state: String = sqlx::query_scalar(
        "SELECT state FROM runtime_memory_writes WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("visible refusal");
    assert_eq!(state, "failed");
    assert!(recall(&fixture, run_id).await.records.is_empty());
}
