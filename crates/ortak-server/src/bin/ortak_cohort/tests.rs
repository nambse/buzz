use super::*;

fn capture_config() -> Value {
    json!({"community_id":Uuid::new_v4(),"action":{"kind":"capture",
        "relay_capture_hook_installed":true,"channel_ids":[Uuid::new_v4()],"employee_ids":["cem"]}})
}

#[test]
fn default_off_and_missing_capture_hook_declaration_are_rejected_before_connection() {
    let mut config = capture_config();
    for enabled in [None, Some("false"), Some("TRUE"), Some("1")] {
        assert!(parse_config(enabled, &config.to_string()).is_err());
    }
    assert!(parse_config(Some("true"), &config.to_string()).is_ok());
    config["action"]["relay_capture_hook_installed"] = json!(false);
    assert_eq!(
        parse_config(Some("true"), &config.to_string()).err(),
        Some("capture requires the deployed atomic relay ingress hook declaration")
    );
    config["action"]
        .as_object_mut()
        .unwrap()
        .remove("relay_capture_hook_installed");
    assert!(parse_config(Some("true"), &config.to_string()).is_err());
}

#[test]
fn selection_is_finite_unique_and_has_no_client_supplied_company_or_unknown_fields() {
    let valid = capture_config();
    for changed in [
        json!({"company_id":Uuid::new_v4()}),
        json!({"community_id":Uuid::nil()}),
    ] {
        let mut config = valid.clone();
        config
            .as_object_mut()
            .unwrap()
            .extend(changed.as_object().unwrap().clone());
        assert!(parse_config(Some("true"), &config.to_string()).is_err());
    }
    for (field, value) in [
        ("channel_ids", json!([])),
        ("employee_ids", json!([])),
        ("channel_ids", json!([Uuid::nil()])),
        (
            "channel_ids",
            json!([
                valid["action"]["channel_ids"][0],
                valid["action"]["channel_ids"][0]
            ]),
        ),
        ("employee_ids", json!(["cem", "cem"])),
        ("employee_ids", json!(["Invalid Employee"])),
        (
            "channel_ids",
            json!((0..65).map(|_| Uuid::new_v4()).collect::<Vec<_>>()),
        ),
        (
            "employee_ids",
            json!((0..65).map(|n| format!("employee-{n}")).collect::<Vec<_>>()),
        ),
        ("unknown", json!(true)),
    ] {
        let mut config = valid.clone();
        config["action"][field] = value;
        assert!(
            parse_config(Some("true"), &config.to_string()).is_err(),
            "{field}"
        );
    }
    assert!(parse_config(Some("true"), &" ".repeat(65_537)).is_err());
}

#[test]
fn reconcile_uses_exact_capture_and_one_bounded_page_and_other_actions_are_explicit() {
    let community = Uuid::new_v4();
    let valid = json!({"community_id":community,"action":{"kind":"reconcile",
        "capture_id":Uuid::new_v4(),"channel_id":Uuid::new_v4()}});
    let parsed = parse_config(Some("true"), &valid.to_string()).unwrap();
    assert!(matches!(
        parsed.action,
        Action::Reconcile { limit: 256, .. }
    ));
    for (field, value) in [
        ("limit", json!(0)),
        ("limit", json!(257)),
        ("capture_id", json!(Uuid::nil())),
        ("channel_id", json!(Uuid::nil())),
    ] {
        let mut config = valid.clone();
        config["action"][field] = value;
        assert!(parse_config(Some("true"), &config.to_string()).is_err());
    }
    for action in [
        json!({"kind":"status"}),
        json!({"kind":"disable"}),
        json!({"kind":"enable","capture_id":Uuid::new_v4()}),
    ] {
        assert!(parse_config(
            Some("true"),
            &json!({"community_id":community,"action":action}).to_string()
        )
        .is_ok());
    }
    for action in [
        json!({"kind":"enable"}),
        json!({"kind":"enable","capture_id":Uuid::nil()}),
        json!({"kind":"status","channel_ids":[]}),
        json!({"kind":"disable","capture_id":Uuid::new_v4()}),
        json!({"kind":"automatic"}),
    ] {
        assert!(
            parse_config(
                Some("true"),
                &json!({"community_id":community,"action":action}).to_string()
            )
            .is_err(),
            "unexpected action acceptance: {action}"
        );
    }
}
