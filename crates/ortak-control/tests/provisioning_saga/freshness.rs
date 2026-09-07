//! Resumed activation must use fresh adapter observations, not cached successes.
use super::*;
use ortak_control::office_identity::{
    OfficeIdentityAdapter, OfficeIdentityError, OfficeMembershipRequest, ProfilePublication,
};
use std::time::Duration;

#[tokio::test]
async fn resumed_cached_success_reprobes_and_recovers_without_recreating_resources() {
    let manifest = disposable_adopt();
    let harness = Harness::adoptable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "fresh-memory-retry")
        .await;
    mark_ready_for_activation(
        &harness,
        &operation,
        &healthy_evidence(&manifest.employee.office.public_key, true),
    )
    .await;
    harness.memory.set_unavailable(true);
    let outcome = harness
        .saga()
        .resume(&harness.repo.scope(), operation.id)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SagaOutcome::Failed {
            step: ProvisioningStep::ActivateRevision,
            ..
        }
    ));
    assert_eq!(harness.repo.activations(), 0);
    harness.memory.set_unavailable(false);
    let outcome = harness
        .saga()
        .resume(&harness.repo.scope(), operation.id)
        .await
        .unwrap();
    let operation = succeeded(outcome);
    assert_eq!(harness.repo.activations(), 1);
    assert!(
        harness.office.published_profiles().is_empty(),
        "finished publication must not replay on a freshness retry"
    );
    assert!(harness.memory.created_resources().is_empty());
    let result = &step(&operation, ProvisioningStep::ActivateRevision).result;
    assert_eq!(result["admission"]["attempt_count"], 2);
    assert_eq!(result["admission"]["format"], "ortak.activation/v1");
    assert_eq!(
        result["result_revision_id"],
        serde_json::json!(operation.result_revision_id)
    );
}

struct ProbeOffice<'a> {
    inner: &'a FakeOfficeIdentityAdapter,
    wrong_key: bool,
    delay: Duration,
}
impl OfficeIdentityAdapter for ProbeOffice<'_> {
    async fn verify_signer(
        &self,
        r: &CredentialRef,
        e: &OfficePublicKey,
    ) -> Result<SignerVerification, OfficeIdentityError> {
        std::thread::sleep(self.delay);
        let mut proof = self.inner.verify_signer(r, e).await?;
        if self.wrong_key {
            proof.produced_public_key = OfficePublicKey::parse_hex(&"de".repeat(32))?;
            proof.matches_expected = true;
        }
        Ok(proof)
    }
    async fn ensure_membership(
        &self,
        r: &OfficeMembershipRequest,
    ) -> Result<ResourceOutcome, OfficeIdentityError> {
        self.inner.ensure_membership(r).await
    }
    async fn remove_created_membership(&self, r: &str, k: &str) -> Result<(), OfficeIdentityError> {
        self.inner.remove_created_membership(r, k).await
    }
    async fn membership_health(
        &self,
        k: &OfficePublicKey,
    ) -> Result<HealthReport, OfficeIdentityError> {
        self.inner.membership_health(k).await
    }
    async fn publish_profile(
        &self,
        e: &EmployeeId,
        b: &ortak_domain::OfficeBinding,
        n: &str,
        k: &str,
    ) -> Result<ProfilePublication, OfficeIdentityError> {
        self.inner.publish_profile(e, b, n, k).await
    }
}

#[tokio::test]
async fn fresh_signer_must_produce_exact_key_even_when_adapter_claims_it_matches() {
    let manifest = disposable_adopt();
    let harness = Harness::adoptable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "fresh-wrong-key")
        .await;
    mark_ready_for_activation(
        &harness,
        &operation,
        &healthy_evidence(&manifest.employee.office.public_key, true),
    )
    .await;
    let office = ProbeOffice {
        inner: &harness.office,
        wrong_key: true,
        delay: Duration::ZERO,
    };
    let saga = ProvisioningSaga::new(
        &harness.repo,
        &harness.runtime,
        &harness.memory,
        &office,
        &harness.credentials,
        SagaConfig::default(),
    );
    assert!(matches!(
        saga.resume(&harness.repo.scope(), operation.id)
            .await
            .unwrap(),
        SagaOutcome::Failed {
            step: ProvisioningStep::ActivateRevision,
            ..
        }
    ));
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn synchronous_probe_overshoot_never_reaches_the_activation_repository() {
    let manifest = disposable_adopt();
    let harness = Harness::adoptable(&manifest);
    let operation = harness
        .begin(
            &manifest,
            OperationMode::Adopt,
            false,
            "fresh-blocking-probe",
        )
        .await;
    mark_ready_for_activation(
        &harness,
        &operation,
        &healthy_evidence(&manifest.employee.office.public_key, true),
    )
    .await;
    let office = ProbeOffice {
        inner: &harness.office,
        wrong_key: false,
        delay: Duration::from_millis(120),
    };
    let capture = support::Capture::new(&harness.repo);
    let config = SagaConfig {
        activation_lifetime: Duration::from_millis(80),
        ..SagaConfig::default()
    };
    let saga = ProvisioningSaga::new(
        &capture,
        &harness.runtime,
        &harness.memory,
        &office,
        &harness.credentials,
        config,
    );
    assert!(matches!(
        saga.resume(&harness.repo.scope(), operation.id)
            .await
            .unwrap(),
        SagaOutcome::Failed {
            step: ProvisioningStep::ActivateRevision,
            ..
        }
    ));
    assert!(
        !capture.has_candidate(),
        "an expired synchronous Ready result must be dropped before the repository is called"
    );
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn final_activation_refuses_changed_operation_or_compensating_prerequisite() {
    for prior_step in [false, true] {
        let manifest = disposable_adopt();
        let harness = Harness::adoptable(&manifest);
        let operation = harness
            .begin(&manifest, OperationMode::Adopt, false, "fresh-state-change")
            .await;
        let capture = support::Capture::new(&harness.repo);
        let saga = ProvisioningSaga::new(
            &capture,
            &harness.runtime,
            &harness.memory,
            &harness.office,
            &harness.credentials,
            SagaConfig::default(),
        );
        assert!(saga
            .resume(&harness.repo.scope(), operation.id)
            .await
            .is_err());
        let candidate = capture.take();
        if prior_step {
            let current = harness
                .repo
                .load_operation(&harness.repo.scope(), operation.id)
                .await
                .unwrap()
                .unwrap();
            let mut changed = step(&current, ProvisioningStep::EnsureMemoryResources).clone();
            changed.state = StepState::Compensating;
            harness
                .repo
                .record_step(&harness.repo.scope(), operation.id, &changed)
                .await
                .unwrap();
        } else {
            harness
                .repo
                .update_operation(
                    &harness.repo.scope(),
                    operation.id,
                    &OperationUpdate {
                        status: OperationStatus::Failed,
                        current_step: Some(ProvisioningStep::ActivateRevision),
                        error_message: Some("operator stopped activation".into()),
                    },
                )
                .await
                .unwrap();
        }
        assert!(harness
            .repo
            .activate_revision(&harness.repo.scope(), operation.id, &candidate)
            .await
            .is_err());
        assert_eq!(harness.repo.activations(), 0);
    }
}

#[tokio::test]
async fn healthy_capability_reports_must_name_the_prepared_adapter() {
    for memory in [false, true] {
        let mut manifest = disposable_adopt();
        if memory {
            manifest.employee.memory.as_mut().unwrap().adapter = "unselected-memory".into();
        } else {
            manifest.employee.runtime.adapter = "unselected-runtime".into();
        }
        let harness = Harness::adoptable(&manifest);
        let operation = harness
            .begin(&manifest, OperationMode::Adopt, false, "fresh-adapter-name")
            .await;
        mark_ready_for_activation(
            &harness,
            &operation,
            &healthy_evidence(&manifest.employee.office.public_key, true),
        )
        .await;
        assert!(matches!(
            harness
                .saga()
                .resume(&harness.repo.scope(), operation.id)
                .await
                .unwrap(),
            SagaOutcome::Failed {
                step: ProvisioningStep::ActivateRevision,
                ..
            }
        ));
        assert_eq!(harness.repo.activations(), 0);
    }
}
