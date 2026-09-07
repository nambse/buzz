use super::*;

fn fixture() -> ConversationContext {
    serde_json::from_str(include_str!("test_vector.json")).expect("shared wire vector")
}

fn valid(context: &ConversationContext) -> bool {
    context.valid_for(
        context.snapshot_id,
        &context.employee.employee_id,
        context.employee.revision_id,
    )
}

#[test]
fn shared_wire_retains_distinct_authors_and_trigger() {
    let context = fixture();
    assert!(valid(&context));
    assert_eq!(context.employee.name, "Bora");
    assert_eq!(
        context.messages[1]
            .author_employee_id
            .as_ref()
            .map(EmployeeId::as_str),
        Some("ada")
    );
    assert_ne!(context.messages[1].message_id, context.trigger_message_id);
    assert!(context.messages[1].content.contains("Ürün hedefini"));
    let encoded = serde_json::to_vec(&context).expect("encode");
    assert_eq!(
        serde_json::from_slice::<ConversationContext>(&encoded).expect("decode"),
        context
    );
}

#[test]
fn rejects_duplicate_trigger_cross_thread_and_order() {
    let original = fixture();
    let mut changed = original.clone();
    changed.messages.push(changed.messages[0].clone());
    assert!(!valid(&changed));
    changed = original.clone();
    changed.messages[0].message_id = changed.trigger_message_id.clone();
    assert!(!valid(&changed));
    changed = original.clone();
    changed.messages.reverse();
    assert!(!valid(&changed));
    changed = original.clone();
    changed.thread_root_message_id = Some("d".repeat(64));
    changed.messages[0].selection = ContextSelection::ThreadRecent;
    assert!(!valid(&changed));
    assert!(!original.valid_for(
        Uuid::new_v4(),
        &original.employee.employee_id,
        original.employee.revision_id
    ));
    changed = original.clone();
    changed.teammates.push(changed.employee.clone());
    assert!(!valid(&changed));
}

#[test]
fn rejects_byte_budget_unknown_fields_and_forged_actor_roles() {
    let mut context = fixture();
    context.messages[0].content = "ü".repeat(MAX_MESSAGE_BYTES / 2 + 1);
    assert!(!valid(&context));
    context.messages[0].content = "a\0b".into();
    assert!(!valid(&context));
    let mut wire = serde_json::to_value(fixture()).expect("wire");
    wire["messages"][0]["role"] = "system".into();
    assert!(serde_json::from_value::<ConversationContext>(wire).is_err());
    let mut context = fixture();
    context.messages.clear();
    for ordinal in 0..7 {
        let mut message = fixture().messages[0].clone();
        message.message_id = format!("{ordinal:064x}");
        message.content = "x".repeat(MAX_MESSAGE_BYTES);
        context.messages.push(message);
    }
    assert!(!valid(&context));
}
