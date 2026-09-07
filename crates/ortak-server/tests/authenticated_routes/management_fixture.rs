//! Exact valid prepared selection with deliberately absent credentials.
use super::*;

pub(super) fn prepared(scope: &ortak_control::CompanyScope, channel: Uuid) -> Value {
    let public_key = Keys::generate().public_key().to_hex();
    let memory = json!({"adapter":"honcho","endpoint_ref":"service://fixture/honcho",
        "workspace":"fixture-workspace","user_peer":"fixture-human","employee_peer":"fixture-employee"});
    let office = json!({"public_key":public_key,"signer_ref":"secret://fixture/office","home_channel_ref":channel.to_string()});
    let missing = format!("ORTAK_MISSING_{}", Uuid::new_v4().simple());
    assert!(std::env::var_os(&missing).is_none());
    json!({
        "community_id":scope.community_id().unwrap(),"operation_key":Uuid::new_v4().to_string(),"mode":"adopt","dry_run":false,
        "manifest":{"schema_version":"ortak.employee/v0","provisioning":"adopt","employee":{
            "id":"prepared-fixture","name":"Prepared Fixture","title":"Assistant","biography":"Isolated test",
            "status":"draft","aliases":[],"responsibilities":[],"domains":[],
            "runtime":{"adapter":"hermes","profile_ref":"fixture-profile","model":"fixture-model","workspace_ref":"/fixture",
                "credential_refs":["secret://fixture/oauth"]},
            "memory":memory,"office":office,"permissions":{},"routing":{"enabled":false,"semantic_min_score":null}}},
        "bridge_origin":"http://127.0.0.1:1","bridge_token_env":missing,
        "runtime_credentials":{"source":"environment","bindings":[{"credential_ref":"secret://fixture/oauth","environment_variable":"ORTAK_FIXTURE_OAUTH"}]},
        "office_signer":{"company_id":scope.company_id(),"employee_id":"prepared-fixture","signer_ref":"secret://fixture/office",
            "public_key":public_key,"secret_env":"ORTAK_FIXTURE_OFFICE"},
        "office":{"company_id":scope.company_id(),"community_id":scope.community_id().unwrap(),"origin":"http://127.0.0.1:1",
            "employees":[{"employee_id":"prepared-fixture","office":office,"channels":[channel]}]},
        "memory":{"origin":"http://127.0.0.1:1","token_ref":"secret://fixture/honcho","token_env":"ORTAK_FIXTURE_HONCHO",
            "validate_memory_io":true,"validation_run_id":Uuid::new_v4(),"validation_recorded_at":"2026-09-05T12:00:00Z",
            "creation_receipt":{"company_id":scope.company_id(),"deployment_id":Uuid::new_v4(),"employee_id":"prepared-fixture",
                "binding":memory,"creation_key":"original-create-key","request_hash":"0".repeat(64),
                "native_ids":{"workspace":"native-workspace","peers":{"fixture-human":"native-human","fixture-employee":"native-employee"}},
                "resources":{"workspace":{"resource_ref":"fixture-workspace","ownership":"created"},
                    "user_peer":{"resource_ref":"fixture-human","ownership":"created"},
                    "employee_peer":{"resource_ref":"fixture-employee","ownership":"created"}}}}
    })
}
