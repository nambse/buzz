use super::*;
use ortak_control::memory::conversation::{
    ConversationAudienceV1, ConversationEventIdentity, ConversationMemoryDigest,
    ConversationProvenanceV1,
};
use sha2::{Digest, Sha256};
mod authority;

pub(super) fn fixture(
    work: bool,
) -> (
    DispatchAuthority,
    FrozenRunSnapshot,
    ReviewedConversationSelection,
) {
    let run = Uuid::from_u128(20);
    let project = Uuid::from_u128(12);
    let work = work.then_some(crate::authority::WorkRunOrigin {
        run_id: run,
        work_item_id: Uuid::from_u128(21),
        project_id: project,
        execution_version: 2,
        definition_hash: "a".repeat(64),
    });
    let a = authority::authority_for(Uuid::from_u128(10), Uuid::from_u128(11), "question", work);
    let timestamp = "2026-09-06T00:00:00Z".parse().unwrap();
    let event = |id| {
        ConversationEventIdentity::new(ortak_control::MessageId::from_bytes([id; 32]), timestamp)
            .unwrap()
    };
    let audience = ConversationAudienceV1::thread(
        a.company_id(),
        Uuid::from_u128(7),
        project,
        a.employee_id().clone(),
        Uuid::from_u128(6),
        event(8),
    )
    .unwrap();
    let p = ConversationProvenanceV1::new(
        audience,
        event(4),
        ConversationMemoryDigest::from_bytes([13; 32]),
    )
    .unwrap();
    let origin =
        ConversationMemoryOrigin::from_observation(&[9; 32], &p.canonical_bytes().unwrap())
            .unwrap();
    let base =
        FrozenRunSnapshot::from_recall(&a, run, ortak_control::memory::MemoryRecall::default())
            .unwrap();
    let selected = ReviewedConversationSelection {
        company_id: a.company_id(),
        project_id: project,
        employee_id: a.employee_id().clone(),
        binding: a.memory_binding().unwrap().clone(),
        origin,
        records: vec![],
        truncated: false,
    };
    (a, base, selected)
}

pub(super) fn pin(
    selected: &ReviewedConversationSelection,
    id: u128,
    priority: u8,
    text: &str,
) -> ReviewedSelectionPin {
    let original = selected.origin.parsed_provenance().unwrap();
    let expires_at = "2026-09-07T00:00:00Z".parse().unwrap();
    let common = ReviewedMemoryPin {
        fact_id: Uuid::from_u128(id),
        target_id: Uuid::from_u128(101),
        fact_version: 1,
        consumption_epoch: 0,
        content_hash: hex::encode(Sha256::digest(text.as_bytes())),
        source_hash: "b".repeat(64),
        binding_hash: "c".repeat(64),
        approval_id: Uuid::from_u128(102),
        approved_by: "d".repeat(64),
        expires_at,
    };
    if priority == 2 {
        return ReviewedSelectionPin::Project { pin: common };
    }
    let a = original.audience();
    let audience = if priority == 0 {
        a.clone()
    } else {
        ConversationAudienceV1::channel(
            a.company_id(),
            a.community_id(),
            a.project_id(),
            a.employee_id().clone(),
            a.channel_id(),
        )
        .unwrap()
    };
    let p = ConversationProvenanceV1::new(
        audience,
        original.source().clone(),
        original.source_evidence_hash(),
    )
    .unwrap();
    ReviewedSelectionPin::Conversation {
        pin: ReviewedConversationPin {
            fact_id: common.fact_id,
            target_id: common.target_id,
            fact_version: 1,
            consumption_epoch: 0,
            content_hash: common.content_hash,
            source_hash: p.source_hash().unwrap().to_hex(),
            binding_hash: common.binding_hash,
            approval_id: common.approval_id,
            approved_by: common.approved_by,
            expires_at,
            conversation_audience_hash: p.audience().audience_hash().unwrap().to_hex(),
            conversation_authority_epoch: 2,
            conversation_consumption_epoch: 3,
        },
        provenance: String::from_utf8(p.canonical_bytes().unwrap()).unwrap(),
    }
}

#[test]
fn selected_conversation_response_restores_priority_without_inventing_missing_text() {
    let (a, base, mut selected) = fixture(true);
    let text = "Approved remote fact";
    selected.records = vec![
        pin(&selected, 900, 0, text),
        pin(&selected, 500, 1, text),
        pin(&selected, 100, 2, text),
    ];
    let remote = ReviewedSelectedRecall {
        records: selected
            .records
            .iter()
            .rev()
            .map(|p| p.record(text.into()))
            .collect(),
        truncated: false,
    };
    let result =
        response::compose(base.clone(), &a, &selected, remote, &RedactionPolicy::new()).unwrap();
    assert_eq!(
        result
            .conversation()
            .unwrap()
            .records
            .iter()
            .map(ReviewedContextRecord::fact_id)
            .collect::<Vec<_>>(),
        vec![
            Uuid::from_u128(900),
            Uuid::from_u128(500),
            Uuid::from_u128(100)
        ]
    );
    let project_only = ReviewedSelectedRecall {
        records: vec![selected.records[2].record(text.into())],
        truncated: false,
    };
    let result = response::compose(
        base.clone(),
        &a,
        &selected,
        project_only,
        &RedactionPolicy::new(),
    )
    .unwrap();
    assert!(result.conversation().is_none());
    assert_eq!(result.reviewed().unwrap().records.len(), 1);
    assert_eq!(
        result.reviewed().unwrap().records[0].pin.fact_id,
        Uuid::from_u128(100)
    );
    assert!(result.reviewed().unwrap().truncated);
    let (a, base, mut selected) = fixture(false);
    selected.records.push(pin(&selected, 900, 0, text));
    let raw = base.encode().unwrap();
    let result = response::compose(
        base,
        &a,
        &selected,
        ReviewedSelectedRecall::default(),
        &RedactionPolicy::new(),
    )
    .unwrap();
    assert_eq!(
        result.encode().unwrap(),
        raw,
        "no conversation remotely returned; original scratch bytes survive"
    );
}

#[test]
fn selected_conversation_response_refuses_wrong_pin_variant_digest_duplicate_and_redaction() {
    let (a, base, mut selected) = fixture(true);
    let text = "Approved remote fact";
    selected.records.push(pin(&selected, 900, 0, text));
    for n in 0..5 {
        let mut record = selected.records[0].record(text.into());
        match n {
            0 => {
                if let ReviewedContextRecord::Conversation { record } = &mut record {
                    record.pin.conversation_consumption_epoch += 1
                }
            }
            1 => {
                if let ReviewedContextRecord::Conversation { record } = &mut record {
                    record.content.push('!')
                }
            }
            2 => record = pin(&selected, 900, 2, text).record(text.into()),
            3 => record = pin(&selected, 901, 0, text).record(text.into()),
            _ => {}
        }
        let mut remote = ReviewedSelectedRecall {
            records: vec![record.clone()],
            truncated: false,
        };
        if n == 4 {
            remote.records.push(record)
        }
        assert!(
            response::compose(base.clone(), &a, &selected, remote, &RedactionPolicy::new())
                .is_err(),
            "case {n}"
        );
    }
    let redaction = RedactionPolicy::new().with_literal_secrets([text]);
    let remote = ReviewedSelectedRecall {
        records: vec![selected.records[0].record(text.into())],
        truncated: false,
    };
    assert!(response::compose(base, &a, &selected, remote, &redaction).is_err());
}

#[tokio::test]
async fn legacy_adapter_defaults_do_not_enable_conversation_or_call_legacy_recall() {
    struct Legacy;
    impl ReviewedRunAdapter for Legacy {
        fn reviewed_enabled(&self, _: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
            Ok(false)
        }
        async fn recall_selected(
            &self,
            _: &ReviewedMemorySelection,
            _: &str,
        ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
            panic!("conversation default must not invoke legacy recall")
        }
    }
    let (a, _, selected) = fixture(false);
    assert_eq!(Legacy.conversation_project(&a).unwrap(), None);
    assert!(matches!(
        Legacy.recall_selected_conversation(&selected, "term").await,
        Err(DispatchRefusal::MemoryAdapterUnavailable)
    ));
    assert_eq!(
        office_query("Deploy DEPLOY café", &RedactionPolicy::new()),
        "deploy OR café"
    );
}
