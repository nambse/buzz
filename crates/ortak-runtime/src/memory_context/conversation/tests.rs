use super::*;
use ortak_control::memory::conversation::{ConversationAudienceV1, ConversationProvenanceV1};

mod fixture;
mod validation;
mod wire;
use fixture::*;

#[test]
fn conversation_v4_preserves_order_and_exact_legacy_project_rendering() {
    let f = Fixture::new(true);
    let project = project_record(200);
    let legacy = ReviewedMemoryContext {
        records: vec![project.clone()],
        truncated: false,
    };
    let expected = legacy.rendered().unwrap().remove(0);
    let mut context = f.context.clone();
    context.records.insert(
        0,
        ReviewedContextRecord::Project {
            record: project.clone(),
        },
    );
    let snapshot = f
        .base()
        .with_conversation(&f.authority, context.clone())
        .unwrap();
    assert_eq!(snapshot.spec().context.memory_context[1], expected);
    assert_eq!(
        snapshot.conversation().unwrap().records[0].fact_id(),
        project.pin.fact_id
    );
    assert!(snapshot.reviewed().is_none());
    assert!(snapshot
        .clone()
        .with_reviewed(&f.authority, legacy)
        .is_err());
    assert!(snapshot
        .with_conversation(&f.authority, context.clone())
        .is_err());
    let office = Fixture::new(false);
    assert!(office
        .base()
        .with_conversation(&office.authority, context.clone())
        .is_err());
    context.records.pop();
    assert!(
        f.base().with_conversation(&f.authority, context).is_err(),
        "project-only must retain v3"
    );
    let mut duplicate = f.context.clone();
    let mut project = project;
    project.pin.fact_id = duplicate.records[0].fact_id();
    duplicate
        .records
        .push(ReviewedContextRecord::Project { record: project });
    assert!(f.base().with_conversation(&f.authority, duplicate).is_err());
}

#[test]
fn conversation_v4_prioritizes_reviewed_records_within_combined_budget() {
    let f = Fixture::new(false);
    let mut scratch = crate::memory_context::tests::recall(&f.authority, f.run);
    scratch.records[0].content = "s".repeat(4096);
    for n in 1..4 {
        let mut record = scratch.records[0].clone();
        record.record_ref = format!("scratch-{n}");
        scratch.records.push(record);
    }
    let base = FrozenRunSnapshot::from_recall(&f.authority, f.run, scratch).unwrap();
    let mut context = f.context.clone();
    content(conversation_record(&mut context), &"c".repeat(4096));
    let mut second = context.records[0].clone();
    if let ReviewedContextRecord::Conversation { record } = &mut second {
        record.pin.fact_id = Uuid::from_u128(201);
    }
    context.records.push(second);
    let snapshot = base
        .with_conversation(&f.authority, context.clone())
        .unwrap();
    assert_eq!(snapshot.wire.recall.records.len(), 2);
    assert!(snapshot.wire.recall.truncated);
    assert_eq!(snapshot.spec().context.memory_context.len(), 4);
    content(conversation_record(&mut context), &"c".repeat(4095));
    let mut third = context.records[0].clone();
    if let ReviewedContextRecord::Conversation { record } = &mut third {
        record.pin.fact_id = Uuid::from_u128(202);
        content(record, "xx");
    }
    context.records.push(third);
    assert!(
        f.empty_base()
            .with_conversation(&f.authority, context)
            .is_err(),
        "8193 reviewed bytes"
    );

    let mut context = f.context.clone();
    for n in 1..8 {
        let mut next = context.records[0].clone();
        if let ReviewedContextRecord::Conversation { record } = &mut next {
            record.pin.fact_id = Uuid::from_u128(200 + n);
        }
        context.records.push(next);
    }
    let full = f
        .base()
        .with_conversation(&f.authority, context.clone())
        .unwrap();
    assert!(full.wire.recall.records.is_empty() && full.wire.recall.truncated);
    assert_eq!(full.spec().context.memory_context.len(), 8);
    let mut ninth = context.records[0].clone();
    if let ReviewedContextRecord::Conversation { record } = &mut ninth {
        record.pin.fact_id = Uuid::from_u128(300);
    }
    context.records.push(ninth);
    assert!(f
        .empty_base()
        .with_conversation(&f.authority, context)
        .is_err());
}

#[test]
fn conversation_v4_bounds_rendered_utf8_including_escaped_metadata() {
    let f = Fixture::new(false);
    let mut context = f.context.clone();
    content(conversation_record(&mut context), &"\"".repeat(4096));
    assert!(f
        .empty_base()
        .with_conversation(&f.authority, context.clone())
        .is_err());
    // This computes the boundary using the actual provider-facing wrapper,
    // while the independently asserted 8192-byte ceiling remains literal.
    let mut record = conversation_record(&mut context).clone();
    content(&mut record, "x");
    let wrapper = |record: &ReviewedConversationRecord| {
        serde_json::to_string(&serde_json::json!({
            "type":"reviewed_conversation_memory", "trust":"untrusted_data", "record":record
        }))
        .unwrap()
    };
    let overhead = wrapper(&record).len() - 1;
    let room = 8192 - overhead;
    let exact = format!("{}{}", "\"".repeat(room / 2), "x".repeat(room % 2));
    assert!(exact.len() < 4096);
    content(&mut record, &exact);
    assert_eq!(wrapper(&record).len(), 8192);
    context.records[0] = ReviewedContextRecord::Conversation {
        record: record.clone(),
    };
    let snapshot = f
        .empty_base()
        .with_conversation(&f.authority, context.clone())
        .unwrap();
    assert_eq!(snapshot.spec().context.memory_context[0].len(), 8192);
    content(&mut record, &format!("{exact}x"));
    assert_eq!(wrapper(&record).len(), 8193);
    context.records[0] = ReviewedContextRecord::Conversation { record };
    assert!(f
        .empty_base()
        .with_conversation(&f.authority, context)
        .is_err());
}
