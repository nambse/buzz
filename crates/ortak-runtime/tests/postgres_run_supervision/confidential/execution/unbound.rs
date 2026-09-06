use super::*;
use ortak_control::ports::CompanyDirectory;

#[tokio::test]
#[ignore = "disposable77 + explicit synthetic key env; loopback start then keyless suspended stop"]
async fn encrypted_suspended_recovery_uses_retained_scope_and_denies_all_keys() {
    let x = EncryptedFixture::new().await;
    let run = admitted(&x).await;
    let reference = format!("ortak:{}:{run}", x.f.scope.company_id());
    let (origin, http) = server(vec![
        ("POST /v1/confidential/runs/lookup ", Reply::Absent),
        (
            "POST /v1/confidential/runs ",
            Reply::Json(
                json!({"runtime_run_ref":reference,"started_at":Utc::now(),"status":"running"}),
            ),
        ),
        (
            "POST /v1/confidential/runs/cancel ",
            Reply::Json(json!({"runtime_run_ref":reference,"outcome":"cancelled"})),
        ),
    ])
    .await;
    let adapter = HermesAdapter::new(x.f.scope.company_id(), &origin, "synthetic-bearer").unwrap();
    let repo = PgConfidentialExecution::new(x.f.pool.clone());
    let keys = provider(&x);
    let execute = EncryptedExecution::new(&x.f.scope, &repo, &adapter, &keys);
    assert_eq!(
        execute.dispatch_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    // The new stop-only port cannot claim this ordinary observing state or make
    // an event request. The synthetic server accepts only the later cancel.
    assert_eq!(
        execute.recover_stop_once().await.unwrap(),
        ExecutionProgress::Idle
    );
    let slug: String = sqlx::query_scalar("SELECT slug FROM companies WHERE id=$1")
        .bind(x.f.scope.company_id())
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    // Retained cohorts prohibit removing their Office binding. Suspension is
    // the real revocation seam; preserve that FK and all historical authority.
    sqlx::query("UPDATE companies SET status='suspended' WHERE id=$1")
        .bind(x.f.scope.company_id())
        .execute(&x.f.pool)
        .await
        .unwrap();
    let suspended = x.f.control.resolve_company_by_slug(&slug).await.unwrap();
    assert_eq!(suspended.community_id(), x.f.scope.community_id());
    // The retained Office binding is metadata, not fresh admission authority.
    assert_eq!(
        x.f.control
            .resolve_current_encrypted_scope(&suspended)
            .await
            .unwrap(),
        Some(suspended.clone())
    );
    let current: bool = sqlx::query_scalar("SELECT ortak_confidential_dm_current($1,$2)")
        .bind(suspended.company_id())
        .bind(run)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    assert!(!current);
    let scopes =
        x.f.control
            .confidential_recovery_scopes(&suspended)
            .await
            .unwrap();
    assert_eq!(scopes, vec![x.f.scope.clone()]);
    assert!(x.protected.cancel(&scopes[0], run).await.unwrap());
    let denied = EnvDmKeyProvider::denied();
    let recovery = EncryptedExecution::new(&scopes[0], &repo, &adapter, &denied);
    assert_eq!(
        recovery.recover_stop_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    assert_eq!(http.await.unwrap().len(), 3);
    let states:(String,String)=sqlx::query_as("SELECT c.state,x.state FROM runtime_cancellations c JOIN confidential_execution_leases x USING(company_id,run_id) WHERE c.company_id=$1 AND c.run_id=$2")
        .bind(suspended.company_id()).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(states, ("acknowledged".into(), "stopped".into()));
    assert!(x
        .f
        .control
        .confidential_recovery_scopes(&suspended)
        .await
        .unwrap()
        .is_empty());
}
