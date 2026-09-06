use super::config::Selection;
use nostr::{Keys, SecretKey};
use serde_json::{json, Value};
use uuid::Uuid;

#[test]
fn encrypted_worker_config_binds_explicit_company_and_all_purposes_without_key_io() {
    let company = Uuid::from_u128(1);
    let pair = Uuid::from_u128(2);
    let key = Keys::new(SecretKey::from_slice(&[0x31; 32]).unwrap());
    let good = json!({"format":"ortak-encrypted-worker/1","pair_ids":[pair],
        "relay_origin":"ws://127.0.0.1:9333/","key_bindings":[{
            "signer":{"company_id":company,"employee_id":"synthetic-employee",
                "signer_ref":"secret://synthetic/office","public_key":key.public_key().to_hex(),
                "secret_env":"ORTAK_TEST_ENCRYPTED_CONFIG_NO_KEY_READ"},
            "office_binding_id":Uuid::from_u128(3),"key_version":0,
            "purposes":["dm_decrypt","confidential_wrap","confidential_unwrap","dm_seal"]}]});
    let parsed = Selection::parse(company, good.clone()).unwrap();
    assert_eq!(parsed.pairs, vec![pair]);
    // Only the exact public selection is parsed; this test installs no env key.
    let reject = |value: Value| assert!(Selection::parse(company, value).is_err());
    let mut value = good.clone();
    value["pair_ids"] = json!([pair, pair]);
    reject(value);
    let mut value = good.clone();
    value["pair_ids"] = json!([]);
    reject(value);
    let mut value = good.clone();
    value["pair_ids"] = json!([Uuid::nil()]);
    reject(value);
    let mut value = good.clone();
    value["pair_ids"] = json!((10..27).map(Uuid::from_u128).collect::<Vec<_>>());
    reject(value);
    let mut value = good.clone();
    value["key_bindings"][0]["signer"]["company_id"] = json!(Uuid::from_u128(4));
    reject(value);
    let mut value = good.clone();
    value["key_bindings"][0]["purposes"] =
        json!(["dm_decrypt", "confidential_wrap", "confidential_unwrap"]);
    reject(value);
    let mut value = good.clone();
    value["key_bindings"][0]["purposes"] = json!([
        "dm_decrypt",
        "confidential_wrap",
        "confidential_unwrap",
        "shell"
    ]);
    reject(value);
    let mut value = good.clone();
    value["relay_origin"] = json!("ws://example.invalid/");
    reject(value);
    let mut value = good.clone();
    value["relay_origin"] = json!("wss://example.invalid/?token=forbidden");
    reject(value);
    let mut value = good.clone();
    value["auto_register"] = json!(true);
    reject(value);
    let mut value = good;
    value["key_bindings"][0]["secret_key"] = json!("forbidden");
    reject(value);
}
