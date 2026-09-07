//! Real signed routes expose persisted progress, never a runner or private config.
use super::*;
use ortak_control::provisioning::ProvisioningStep;

fn admin(f: &Fixture) -> Router {
    let mut config = config(f.community, &f.operator, f.channel);
    config.humans[0].can_manage_employees = true;
    config.humans[0]
        .employee_ids
        .push(EmployeeId::parse("missing").unwrap());
    product_router(f.control.clone(), config, Arc::new(Replay::default())).unwrap()
}
async fn operation(f: &Fixture, employee: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO provisioning_operations(company_id,id,employee_id,mode,dry_run,idempotency_key,manifest,manifest_fingerprint,status,current_step,error_message,finished_at) VALUES ($1,$2,$3,'adopt',false,$4,$5,$6,'failed','ensure_runtime_profile','PRIVATE_ADAPTER_ERROR_CANARY',now())")
        .bind(f.company).bind(id).bind(employee).bind(format!("PRIVATE_OPERATION_KEY_CANARY:{id}"))
        .bind(json!({"private_manifest":"PRIVATE_MANIFEST_CANARY","credential_ref":"credential://private/canary"})).bind([0u8;32].as_slice()).execute(&f.pool).await.unwrap();
    for step in ProvisioningStep::ALL {
        let failed = step == ProvisioningStep::EnsureRuntimeProfile;
        sqlx::query("INSERT INTO provisioning_operation_steps(company_id,operation_id,step_index,step_name,state,idempotency_key,attempt_count,adopted_existing,result,error_message,finished_at) VALUES($1,$2,$3,$4,$5,$6,$7,true,$8,'PRIVATE_STEP_ERROR_CANARY',CASE WHEN $9 THEN now() END)")
            .bind(f.company).bind(id).bind(step.index()).bind(step.name()).bind(if failed {"failed"} else {"pending"}).bind(format!("PRIVATE_STEP_KEY_CANARY:{id}:{}",step.name())).bind(if failed {3} else {0}).bind(json!({"native_profile":"PRIVATE_RECEIPT_CANARY"})).bind(failed).execute(&f.pool).await.unwrap();
    }
    id
}

#[test]
fn provisioning_read_privilege_is_explicit_and_never_implied_by_operator() {
    let raw = json!({"origin":"http://localhost:8787","community_id":Uuid::new_v4(),"humans":[{"public_key":Keys::generate().public_key().to_hex(),"role":"operator","channel_ids":[Uuid::new_v4()],"employee_ids":["cem"]}]});
    let config: ApiConfig = serde_json::from_value(raw.clone()).unwrap();
    assert!(!config.humans[0].can_manage_employees);
    let mut invalid = raw;
    invalid["humans"][0]["role"] = json!("reader");
    invalid["humans"][0]["can_manage_employees"] = json!(true);
    let invalid: ApiConfig = serde_json::from_value(invalid).unwrap();
    assert!(invalid.validate().is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn provisioning_progress_requires_current_explicit_employee_management_authority() {
    let f = Fixture::new().await;
    let app = admin(&f);
    let id = operation(&f, "cem").await;
    let path = format!("/api/v1/employees/cem/provisioning/{id}");
    // Existing cancel operators and ordinary readers gain no new privilege.
    for key in [&f.operator, &f.reader] {
        assert_eq!(
            response(&f.app, signed(key, "GET", &path, "", false))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, directory) = response(
        &app,
        signed(&f.operator, "GET", "/api/v1/employees", "", false),
    )
    .await;
    assert_eq!(directory["can_view_provisioning"], true);
    let (_, ordinary) = response(
        &f.app,
        signed(&f.operator, "GET", "/api/v1/employees", "", false),
    )
    .await;
    assert_eq!(ordinary["can_view_provisioning"], false);
    let foreign = Fixture::new().await;
    let foreign_id = operation(&foreign, "cem").await;
    let foreign_path = format!("/api/v1/employees/cem/provisioning/{foreign_id}");
    assert_eq!(
        response(&app, signed(&f.operator, "GET", &foreign_path, "", false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,'ungranted')")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    let ungranted = operation(&f, "ungranted").await;
    let other = format!("/api/v1/employees/ungranted/provisioning/{ungranted}");
    assert_eq!(
        response(&app, signed(&f.operator, "GET", &other, "", false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let missing = "/api/v1/employees/missing/provisioning";
    assert_eq!(
        response(&app, signed(&f.operator, "GET", missing, "", false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let denials:i64=sqlx::query_scalar("SELECT count(*) FROM ortak_api_audit WHERE company_id=$1 AND action='read_employee' AND outcome IN ('denied','not_found')")
        .bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        denials, 4,
        "both privilege denials plus ungranted/missing employee are durably audited"
    );
    // A verified human that becomes an employee/bot is immediately refused.
    sqlx::query("INSERT INTO users(community_id,pubkey,deactivated_at) VALUES($1,$2,now()) ON CONFLICT(community_id,pubkey) DO UPDATE SET deactivated_at=now()")
        .bind(f.community).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    assert_eq!(
        response(&app, signed(&f.operator, "GET", &path, "", false))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn provisioning_projection_reads_ten_real_steps_without_private_receipts_or_external_execution(
) {
    let f = Fixture::new().await;
    let app = admin(&f);
    let id = operation(&f, "cem").await;
    let path = format!("/api/v1/employees/cem/provisioning/{id}");
    let (status, body) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["read_only"], true);
    assert_eq!(body["operation"]["status"], "failed");
    assert_eq!(body["operation"]["result_revision_id"], Value::Null);
    let steps = body["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 10);
    for (entry, step) in steps.iter().zip(ProvisioningStep::ALL) {
        assert_eq!(entry["name"], step.name());
        assert_eq!(entry["adopted_existing"], true);
    }
    assert_eq!(steps[3]["attempt_count"], 3);
    let text = body.to_string();
    fn assert_private_fields_absent(value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    assert!(![
                        "manifest",
                        "result",
                        "error_message",
                        "idempotency_key",
                        "native_profile",
                        "credential_ref"
                    ]
                    .contains(&key.as_str()));
                    assert_private_fields_absent(value);
                }
            }
            Value::Array(values) => values.iter().for_each(assert_private_fields_absent),
            _ => {}
        }
    }
    assert_private_fields_absent(&body);
    for private in [
        "PRIVATE_MANIFEST_CANARY",
        "PRIVATE_ADAPTER_ERROR_CANARY",
        "PRIVATE_RECEIPT_CANARY",
        "PRIVATE_OPERATION_KEY_CANARY",
        "PRIVATE_STEP_KEY_CANARY",
        "PRIVATE_STEP_ERROR_CANARY",
        "credential://",
    ] {
        assert!(!text.contains(private));
    }
    // The malformed private manifest above could not be decoded as a runner
    // request. Successful reads therefore falsify broad repository serialization.
    let (status, _) = response(&app, signed(&f.operator, "POST", &path, "{}", true)).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM provisioning_operations WHERE company_id=$1),(SELECT count(*) FROM office_identity_profiles WHERE company_id=$1)")
        .bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1, 0));
    sqlx::query("UPDATE provisioning_operations SET status='running',current_step='validate_runtime_profile',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    let (_, current) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(current["operation"]["status"], "running");
    assert!(current.get("runtime_health").is_none());
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn provisioning_operation_pages_are_bounded_and_cursor_is_exclusive() {
    let f = Fixture::new().await;
    let app = admin(&f);
    let first = operation(&f, "cem").await;
    let second = operation(&f, "cem").await;
    let path = "/api/v1/employees/cem/provisioning?limit=1";
    let (status, page) = response(&app, signed(&f.operator, "GET", path, "", false)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["operations"].as_array().unwrap().len(), 1);
    assert_eq!(page["operations"][0]["operation_id"], second.to_string());
    assert_eq!(page["has_more"], true);
    let path = format!("{path}&cursor={}", page["next_cursor"].as_str().unwrap());
    let (_, next) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(next["operations"][0]["operation_id"], first.to_string());
    assert_eq!(next["has_more"], false);
    assert_eq!(next["next_cursor"], Value::Null);
    assert!(!page.to_string().contains("PRIVATE_"));
    let malformed = "/api/v1/employees/cem/provisioning?cursor=not-json";
    assert_eq!(
        response(&app, signed(&f.operator, "GET", malformed, "", false))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}
