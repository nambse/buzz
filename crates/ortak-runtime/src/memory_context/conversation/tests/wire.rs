use super::*;

#[test]
fn conversation_v4_closed_wire_retains_canonical_strings_and_original_snapshot_bytes() {
    let f = Fixture::new(false);
    let snapshot = f.snapshot();
    let wire: serde_json::Value = serde_json::from_slice(&snapshot.encode().unwrap()).unwrap();
    assert_eq!(wire["version"], 4);
    assert!(wire.get("reviewed").is_none());
    assert!(wire["conversation"]["origin"]["provenance"].is_string());
    assert_eq!(
        wire["conversation"]["origin"]["requester_public_key"],
        "09".repeat(32)
    );
    let record = &wire["conversation"]["records"][0];
    assert_eq!(record["scope"], "conversation");
    assert!(record["record"]["provenance"].is_string());
    let actual: std::collections::BTreeSet<_> = record["record"]["pin"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected = [
        "fact_id",
        "target_id",
        "fact_version",
        "consumption_epoch",
        "content_hash",
        "source_hash",
        "binding_hash",
        "approval_id",
        "approved_by",
        "expires_at",
        "conversation_audience_hash",
        "conversation_authority_epoch",
        "conversation_consumption_epoch",
    ]
    .into_iter()
    .collect();
    assert_eq!(actual, expected);
    let rendered: serde_json::Value =
        serde_json::from_str(&snapshot.spec().context.memory_context[1]).unwrap();
    assert_eq!(
        rendered,
        serde_json::json!({"type":"reviewed_conversation_memory",
        "trust":"untrusted_data", "record":record["record"]})
    );
    // Canonical thread root is intentionally different from the delivery root.
    let origin = snapshot
        .conversation()
        .unwrap()
        .origin
        .parsed_provenance()
        .unwrap();
    assert_ne!(
        Some(origin.audience().thread_root().unwrap().event_id()),
        f.authority.root_message_id()
    );
    let pretty = serde_json::to_vec_pretty(&wire).unwrap();
    let renewed = crate::memory_context::tests::authority(
        f.authority.company_id(),
        Uuid::from_u128(99),
        "question",
    );
    let decoded = FrozenRunSnapshot::decode(&pretty, &renewed, f.run).unwrap();
    assert_eq!(decoded.encode().unwrap(), pretty);
    assert_eq!(decoded.spec(), snapshot.spec());
}

#[test]
fn conversation_v4_rejects_unsupported_shapes_without_legacy_fallback() {
    let f = Fixture::new(false);
    let wire: serde_json::Value = serde_json::from_slice(&f.snapshot().encode().unwrap()).unwrap();
    for n in 0..10 {
        let mut bad = wire.clone();
        match n {
            0 => bad["version"] = 3.into(),
            1 => bad["reviewed"] = serde_json::Value::Null,
            2 => bad["conversation"] = serde_json::Value::Null,
            3 => bad["conversation"]["records"][0]["scope"] = "employee".into(),
            4 => bad["conversation"]["records"][0]["extra"] = true.into(),
            5 => bad["conversation"]["records"][0]["record"]["pin"]["extra"] = true.into(),
            6 => bad["conversation"]["origin"]["requester_public_key"] = "A".repeat(64).into(),
            7 => bad["conversation"]["origin"]["extra"] = true.into(),
            8 => {
                let p = bad["conversation"]["origin"]["provenance"]
                    .as_str()
                    .unwrap();
                bad["conversation"]["origin"]["provenance"] = format!(" {p}").into();
            }
            _ => bad["conversation"]["records"][0]["record"]["provenance"] = serde_json::json!({}),
        }
        assert!(
            FrozenRunSnapshot::decode(&serde_json::to_vec(&bad).unwrap(), &f.authority, f.run)
                .is_err(),
            "case {n}"
        );
    }
    let raw = String::from_utf8(serde_json::to_vec(&wire).unwrap()).unwrap();
    let duplicate = raw.replacen(
        "\"conversation_authority_epoch\":3",
        "\"conversation_authority_epoch\":3,\"conversation_authority_epoch\":3",
        1,
    );
    assert_ne!(raw, duplicate);
    assert!(FrozenRunSnapshot::decode(duplicate.as_bytes(), &f.authority, f.run).is_err());
}

#[test]
fn conversation_v4_does_not_change_legacy_1_2_3_fields_or_bytes() {
    for work in [false, true] {
        let f = Fixture::new(work);
        let mut snapshots = vec![f.base()];
        if work {
            snapshots.push(
                f.base()
                    .with_reviewed(
                        &f.authority,
                        ReviewedMemoryContext {
                            records: vec![project_record(200)],
                            truncated: false,
                        },
                    )
                    .unwrap(),
            );
        }
        for snapshot in snapshots {
            let bytes = snapshot.encode().unwrap();
            let mut wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(wire.get("conversation").is_none());
            assert!(snapshot.conversation().is_none());
            assert_eq!(
                FrozenRunSnapshot::decode(&bytes, &f.authority, f.run)
                    .unwrap()
                    .encode()
                    .unwrap(),
                bytes
            );
            wire["conversation"] = serde_json::Value::Null;
            assert!(
                FrozenRunSnapshot::decode(&serde_json::to_vec(&wire).unwrap(), &f.authority, f.run)
                    .is_err()
            );
        }
    }
}
