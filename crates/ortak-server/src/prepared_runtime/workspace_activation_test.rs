//! Final activation uses current workspace eligibility after fresh adapter gates.
use super::*;
use ortak_control::fakes::{
    FakeCredentialResolver, FakeMemoryAdapter, FakeOfficeIdentityAdapter, FakeRuntimeAdapter,
};
use ortak_control::provisioning::{ProvisioningSaga, SagaConfig, SagaOutcome};

#[path = "workspace_activation_ports.rs"]
mod ports;
use ports::{SelectedMemory, SelectedRuntime};

async fn resume_saga(f: &Fixture) -> Result<SagaOutcome, ortak_control::ControlError> {
    let config: ProvisioningConfig = serde_json::from_value(f.config.clone()).unwrap();
    let employee = &config.manifest.employee;
    let runtime = SelectedRuntime(
        FakeRuntimeAdapter::new()
            .with_existing_profile(employee.runtime.profile_ref.as_deref().unwrap(), true),
    );
    let memory = SelectedMemory(
        FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap()),
    );
    let office = FakeOfficeIdentityAdapter::new()
        .with_signer(
            employee.office.signer_ref.as_str(),
            &employee.office.public_key,
        )
        .with_existing_member(&employee.office.public_key);
    let credentials = FakeCredentialResolver::new().with_references(
        employee
            .runtime
            .credential_refs
            .iter()
            .map(|r| r.as_str().to_owned())
            .chain(std::iter::once(
                employee.office.signer_ref.as_str().to_owned(),
            )),
    );
    let saga = ProvisioningSaga::new(
        &f.control,
        &runtime,
        &memory,
        &office,
        &credentials,
        SagaConfig::default(),
    );
    saga.resume(&f.scope, f.operation).await
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_activation_rechecks_registry_after_profile_preparation_before_new_target() {
    for withdraw in [false, true] {
        let f = Fixture::new_with_workspace(true).await;
        let grant = current_registry(&f).await;
        let mut running = f.start();
        f.wait_started(&mut running).await;
        f.bridge.known.lock().unwrap().as_mut().unwrap().1 = "completed".into();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), running)
                .await
                .unwrap()
                .unwrap(),
            Ok(())
        );
        if withdraw {
            ortak_runtime::postgres::workspace_tools::revoke(&f.control, &f.scope, grant.revision)
                .await
                .unwrap();
        }
        let result = resume_saga(&f).await;
        if withdraw {
            let error = match result {
                Err(error) => error.to_string(),
                Ok(SagaOutcome::Failed { error, .. }) => error,
                other => panic!("withdrawn Files profile activated: {other:?}"),
            };
            assert!(
                error.contains("selected workspace registry is not current at activation"),
                "{error}"
            );
        } else {
            assert!(
                matches!(result, Ok(SagaOutcome::Succeeded(_))),
                "{result:?}"
            );
        }
        let active: bool = sqlx::query_scalar(
            "SELECT status='active' FROM employees WHERE company_id=$1 AND id='prepared-fixture'",
        )
        .bind(f.scope.company_id())
        .fetch_one(f.control.pool())
        .await
        .unwrap();
        assert_eq!(active, !withdraw);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_expiry_after_activation_validation_rolls_back_at_commit() {
    let f = Fixture::new_with_workspace(true).await;
    let company = f.scope.company_id();
    let name = format!("workspace_expiry_{}", Uuid::new_v4().simple());
    // This fixture delays the real activation UPDATE after its repository
    // validation. It neither changes immutable expiry nor disables any guard.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$
        DECLARE remaining DOUBLE PRECISION;
        BEGIN
            SELECT extract(epoch FROM max(expires_at)-clock_timestamp()) INTO remaining
                FROM workspace_bindings WHERE company_id=NEW.company_id AND employee_id=NEW.id;
            IF remaining IS NULL OR remaining<=0 OR remaining>1.6 THEN
                RAISE EXCEPTION 'controlled expiry fixture did not reach a live activation';
            END IF;
            PERFORM pg_sleep(remaining+0.025);
            RETURN NEW;
        END $$;
        CREATE TRIGGER {name} AFTER UPDATE ON employees FOR EACH ROW
            WHEN(NEW.company_id='{company}'::uuid AND NEW.status='active') EXECUTE FUNCTION {name}();"
    )))
    .execute(f.control.pool())
    .await
    .unwrap();
    let grant = retained_registry_for(&f, 1500).await;
    bind_project(&f, &grant).await;
    let result = resume_saga(&f).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON employees; DROP FUNCTION {name}();"
    )))
    .execute(f.control.pool())
    .await
    .unwrap();
    let error = match result {
        Err(error) => error.to_string(),
        Ok(SagaOutcome::Failed { error, .. }) => error,
        other => panic!("expired Files activation committed: {other:?}"),
    };
    assert!(
        error.contains("Files profile requires a current selected workspace at activation"),
        "{error}"
    );
    let unchanged: bool = sqlx::query_scalar(
        "SELECT status<>'active' AND active_revision_id IS NULL FROM employees WHERE company_id=$1 AND id='prepared-fixture'",
    )
    .bind(company)
    .fetch_one(f.control.pool())
    .await
    .unwrap();
    assert!(unchanged);
}
