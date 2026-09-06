use super::*;
use ortak_control::confidential::ValidatedIdentity;
use ortak_domain::RuntimeBinding;
use serde_json::{json, Value};

fn identity() -> (Value, ValidatedIdentity) {
    let v: Value = serde_json::from_str(include_str!(
        "../../../ortak-control/src/confidential/vector.json"
    ))
    .unwrap();
    let id = v["identity"].clone();
    let parsed = ValidatedIdentity::parse(&serde_json::to_vec(&id).unwrap()).unwrap();
    (id, parsed)
}
fn snapshot() -> (Value, ValidatedIdentity, RuntimeBinding) {
    let (id, parsed) = identity();
    let binding = json!({"adapter":"hermes","profile_ref":"fixture","model":"fixture","workspace_ref":"none","credential_refs":[],"options":{}});
    let body = json!({"format":"ortak-confidential-run/1","identity":id,"spec":{
        "binding":binding,"context":{"conversation_ref":id["conversation_id"],"reply_to_message_id":null},
        "employee_id":id["employee_id"],"revision_id":id["employee_revision_id"],"run_id":id["run_id"],
        "idempotency_key":format!("ortak-run:{}:{}",id["company_id"].as_str().unwrap(),id["run_id"].as_str().unwrap()),
        "input":"exact private text\nİ 🧭","permissions":{"allowed_networks":[],"allowed_tools":[],"allowed_workspaces":[],"approval_required":[]}}});
    (body, parsed, serde_json::from_value(binding).unwrap())
}
#[test]
fn protected_snapshot_requires_exact_identity_binding_context_and_truly_empty_policy() {
    let (body, id, binding) = snapshot();
    assert!(inner::snapshot(&serde_json::to_vec(&body).unwrap(), &id, &binding, None).is_ok());
    for mode in 0..7 {
        let mut bad = body.clone();
        match mode {
            0 => bad["spec"]["permissions"]["allowed_tools"] = json!(["file_read"]),
            1 => bad["spec"]["context"]["memory_context"] = json!([]),
            2 => bad["spec"]["binding"]["model"] = json!("other"),
            3 => bad["spec"]["idempotency_key"] = json!("wrong-key"),
            4 => bad["spec"]["context"]["reply_to_message_id"] = json!("00".repeat(32)),
            5 => bad["spec"]["input"] = json!("x".repeat(8193)),
            _ => bad["spec"]["input"] = json!("nul\0value"),
        }
        assert!(
            inner::snapshot(&serde_json::to_vec(&bad).unwrap(), &id, &binding, None).is_err(),
            "mode {mode}"
        );
    }
}
#[test]
fn protected_inner_parser_rejects_duplicate_positional_and_noncanonical_objects() {
    let (body, id, binding) = snapshot();
    let bytes = serde_json::to_vec(&body).unwrap();
    let duplicate = String::from_utf8(bytes.clone()).unwrap().replacen(
        "\"format\":",
        "\"format\":\"ortak-confidential-run/1\",\"format\":",
        1,
    );
    assert!(inner::snapshot(duplicate.as_bytes(), &id, &binding, None).is_err());
    assert!(inner::snapshot(
        &serde_json::to_vec(&json!([body["format"], body["identity"], body["spec"]])).unwrap(),
        &id,
        &binding,
        None
    )
    .is_err());
    assert!(inner::snapshot(
        &[b" ".as_slice(), bytes.as_slice()].concat(),
        &id,
        &binding,
        None
    )
    .is_err());
    assert!(inner::snapshot(
        &serde_json::to_vec_pretty(&body).unwrap(),
        &id,
        &binding,
        None
    )
    .is_err());
}
#[test]
fn protected_completion_fold_binds_sequence_time_intent_and_final_text() {
    let (id, expected) = identity();
    let now = Utc::now();
    let reference = format!(
        "ortak:{}:{}",
        id["company_id"].as_str().unwrap(),
        id["run_id"].as_str().unwrap()
    );
    let payloads = [
        json!({"event_type":"run.started","runtime_run_ref":reference}),
        json!({"event_type":"assistant.delta","turn":0,"delta":{"text":"private exact\nİ 🧭"}}),
        json!({"event_type":"delivery.intent","intent":"reply"}),
        json!({"event_type":"run.completed","delivery_intent":"reply"}),
    ];
    let event = |ordinal: u32, payload: &Value| {
        serde_json::to_vec(&json!({"format":"ortak-confidential-event/1","identity":id,"sequence":ordinal,"occurred_at":now.to_rfc3339(),"payload":payload})).unwrap()
    };
    let mut fold = inner::Fold::new();
    for (index, payload) in payloads.iter().enumerate() {
        fold.push(
            &event(index as u32 + 1, payload),
            &expected,
            index as u32 + 1,
            now,
        )
        .unwrap();
    }
    assert_eq!(
        fold.finish().unwrap().unwrap().as_str(),
        "private exact\nİ 🧭"
    );
    for mode in 0..4 {
        let mut fold = inner::Fold::new();
        fold.push(&event(1, &payloads[0]), &expected, 1, now)
            .unwrap();
        let failed = match mode {
            0 => fold.push(&event(3, &payloads[1]), &expected, 3, now),
            1 => fold.push(
                &event(2, &payloads[1]),
                &expected,
                2,
                now + chrono::Duration::microseconds(1),
            ),
            2 => fold.push(
                &event(2, &json!({"event_type":"tool.started","tool":"shell"})),
                &expected,
                2,
                now,
            ),
            _ => fold.push(
                &event(2, &json!({"event_type":"delivery.intent","intent":"reply"})),
                &expected,
                2,
                now,
            ),
        };
        assert!(failed.is_err(), "mode {mode}");
    }
    let reply = inner::reply_bytes(&expected, "exact\nİ 🧭").unwrap();
    assert_eq!(
        inner::open_reply(&reply, &expected).unwrap().as_str(),
        "exact\nİ 🧭"
    );
}
