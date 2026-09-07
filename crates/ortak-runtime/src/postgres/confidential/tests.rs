use super::*;
use ortak_domain::RuntimeBinding;
use serde_json::{json, Value};

fn identity() -> ValidatedIdentity {
    let vector: Value = serde_json::from_str(include_str!(
        "../../../../ortak-control/src/confidential/vector.json"
    ))
    .unwrap();
    ValidatedIdentity::parse(
        vector["expected"]["identity_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap()
}
fn binding() -> RuntimeBinding {
    serde_json::from_value(json!({"adapter":"hermes","profile_ref":"fixture","model":"fixture-model","workspace_ref":"none","credential_refs":[],"options":{}})).unwrap()
}

#[test]
fn confidential_inner_uses_canonical_reduced_context_and_exact_utf8() {
    let run = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let text = "exact whitespace\nİ 🧭 \\ \t\u{2028}";
    let encoded = wire::snapshot(
        &identity(),
        &binding(),
        run,
        run,
        "fixture",
        "confidential_run:fixture",
        "pair",
        None,
        text,
    )
    .unwrap();
    let wire: Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(encoded.as_slice(), serde_json::to_vec(&wire).unwrap());
    assert_eq!(wire["format"], "ortak-confidential-run/1");
    assert_eq!(wire["spec"]["input"], text);
    assert_eq!(
        wire["spec"]["context"],
        json!({"conversation_ref":"pair","reply_to_message_id":null})
    );
    assert_eq!(
        wire["spec"]["permissions"],
        json!({"allowed_networks":[],"allowed_tools":[],"allowed_workspaces":[],"approval_required":[]})
    );
    assert_eq!(wire.as_object().unwrap().len(), 3);
}

#[test]
fn confidential_inner_rejects_content_bound_and_nul_without_truncating() {
    let run = Uuid::new_v4();
    for text in [String::new(), "x".repeat(8193), "nul\0content".into()] {
        assert!(wire::snapshot(
            &identity(),
            &binding(),
            run,
            run,
            "fixture",
            "key",
            "pair",
            None,
            &text
        )
        .is_err());
    }
    let input = "İ".repeat(4096);
    let encoded = wire::snapshot(
        &identity(),
        &binding(),
        run,
        run,
        "fixture",
        "key",
        "pair",
        None,
        &input,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&encoded).unwrap()["spec"]["input"],
        input
    );
}
