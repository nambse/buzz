use super::*;

#[test]
fn conversation_v4_rejects_cross_audience_even_with_recomputed_valid_hashes() {
    let f = Fixture::new(false);
    let parsed = f.context.origin.parsed_provenance().unwrap();
    let a = parsed.audience();
    for n in 0..7 {
        let different = Uuid::from_u128(999);
        let root = if n == 5 {
            event(99)
        } else if n == 6 {
            ortak_control::memory::conversation::ConversationEventIdentity::new(
                event(8).event_id(),
                timestamp() + chrono::Duration::microseconds(1),
            )
            .unwrap()
        } else {
            event(8)
        };
        let audience = ConversationAudienceV1::thread(
            if n == 0 { different } else { a.company_id() },
            if n == 1 { different } else { a.community_id() },
            if n == 2 { different } else { a.project_id() },
            if n == 3 {
                ortak_domain::EmployeeId::parse("other").unwrap()
            } else {
                a.employee_id().clone()
            },
            if n == 4 { different } else { a.channel_id() },
            root,
        )
        .unwrap();
        let mut context = f.context.clone();
        set_provenance(conversation_record(&mut context), provenance(audience, 10));
        assert!(
            f.base().with_conversation(&f.authority, context).is_err(),
            "audience axis {n}"
        );
    }
    let mut channel = f.context.clone();
    set_provenance(
        conversation_record(&mut channel),
        provenance(f.channel_audience(), 10),
    );
    assert!(f.base().with_conversation(&f.authority, channel).is_ok());
}

#[test]
fn conversation_v4_origin_is_sealed_structural_data_not_fabricated_dispatch() {
    let f = Fixture::new(false);
    let observed = f.context.origin.parsed_provenance().unwrap();
    let bytes = observed.canonical_bytes().unwrap();
    for requester in [&[9; 31][..], &[9; 33][..]] {
        assert!(ConversationMemoryOrigin::from_observation(requester, &bytes).is_err());
    }
    let channel = provenance(f.channel_audience(), 4)
        .canonical_bytes()
        .unwrap();
    assert!(ConversationMemoryOrigin::from_observation(&[9; 32], &channel).is_err());
    let mut noncanonical = bytes.clone();
    noncanonical.push(b' ');
    assert!(ConversationMemoryOrigin::from_observation(&[9; 32], &noncanonical).is_err());

    let mut context = f.context.clone();
    let other_source = provenance(observed.audience().clone(), 99)
        .canonical_bytes()
        .unwrap();
    context.origin = ConversationMemoryOrigin::from_observation(&[9; 32], &other_source).unwrap();
    assert!(
        f.base().with_conversation(&f.authority, context).is_err(),
        "Office source must equal dispatch input"
    );
    let other_company = crate::memory_context::tests::authority(
        Uuid::from_u128(999),
        Uuid::from_u128(11),
        "question",
    );
    assert!(f.context.validate_for(&other_company).is_err());
    let f = Fixture::new(true);
    let mut work = f.authority.work_origin().unwrap().clone();
    work.project_id = Uuid::from_u128(999);
    let foreign = crate::memory_context::tests::authority_for(
        f.authority.company_id(),
        Uuid::from_u128(11),
        "question",
        Some(work),
    );
    assert!(f.context.validate_for(&foreign).is_err());
    // Frozen historical structure deliberately does not make an expiry/ACL
    // decision; the repository must apply the current SQL checks separately.
    let mut historical = f.context.clone();
    conversation_record(&mut historical).pin.expires_at = timestamp() - chrono::Duration::days(1);
    assert!(f.base().with_conversation(&f.authority, historical).is_ok());
}

#[test]
fn conversation_v4_rejects_pin_forgery_and_content_mutation() {
    let f = Fixture::new(false);
    for n in 0..13 {
        let mut context = f.context.clone();
        let r = conversation_record(&mut context);
        match n {
            0 => r.pin.consumption_epoch = 1,
            1 => r.pin.conversation_authority_epoch = -1,
            2 => r.pin.conversation_consumption_epoch = -1,
            3 => r.pin.conversation_audience_hash = "f".repeat(64),
            4 => r.pin.source_hash = "f".repeat(64),
            5 => r.pin.fact_version = 2,
            6 => r.pin.fact_id = Uuid::nil(),
            7 => r.pin.target_id = Uuid::nil(),
            8 => r.pin.approval_id = Uuid::nil(),
            9 => r.pin.approved_by = "D".repeat(64),
            10 => r.pin.binding_hash.clear(),
            11 => r.content.push_str(" forged"),
            _ => content(r, "bad\0text"),
        }
        assert!(
            f.base().with_conversation(&f.authority, context).is_err(),
            "pin/content {n}"
        );
    }
    let mut duplicate = f.context.clone();
    duplicate.records.push(duplicate.records[0].clone());
    assert!(f.base().with_conversation(&f.authority, duplicate).is_err());
    let mut invalid = f.context.clone();
    conversation_record(&mut invalid).provenance.push(' ');
    assert!(f.base().with_conversation(&f.authority, invalid).is_err());
}

#[test]
fn conversation_v4_decode_rechecks_combined_bytes_rendering_and_dispatch_pins() {
    let f = Fixture::new(false);
    let snapshot = f.snapshot();
    let bytes = snapshot.encode().unwrap();
    let changed = crate::memory_context::tests::authority(
        f.authority.company_id(),
        Uuid::from_u128(99),
        "changed",
    );
    assert!(FrozenRunSnapshot::decode(&bytes, &changed, f.run).is_err());
    assert!(FrozenRunSnapshot::decode(&bytes, &f.authority, Uuid::from_u128(999)).is_err());
    let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    tampered["spec"]["context"]["memory_context"][1] = "raw unframed content".into();
    assert!(FrozenRunSnapshot::decode(
        &serde_json::to_vec(&tampered).unwrap(),
        &f.authority,
        f.run
    )
    .is_err());
    // Bypass the truncating constructor deliberately to exercise decode's own
    // count fence; the attacker also regenerates otherwise-consistent rendering.
    let mut forged = snapshot.wire.clone();
    let original = forged.recall.records[0].clone();
    for n in 1..8 {
        let mut next = original.clone();
        next.record_ref = format!("extra-{n}");
        forged.recall.records.push(next);
    }
    forged.spec.context.memory_context = rendered(&forged.recall).unwrap();
    forged
        .spec
        .context
        .memory_context
        .extend(forged.conversation.as_ref().unwrap().rendered().unwrap());
    assert!(
        FrozenRunSnapshot::decode(&serde_json::to_vec(&forged).unwrap(), &f.authority, f.run)
            .is_err()
    );
}
