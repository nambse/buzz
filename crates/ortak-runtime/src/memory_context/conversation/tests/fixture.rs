use super::*;
use crate::memory_context::tests::{authority_for, recall};
use chrono::{DateTime, Utc};
use ortak_control::MessageId;
use ortak_control::memory::conversation::{ConversationEventIdentity, ConversationMemoryDigest};
use sha2::{Digest, Sha256};

pub(super) struct Fixture {
    pub authority: DispatchAuthority,
    pub run: Uuid,
    pub context: ReviewedConversationContext,
}

pub(super) fn timestamp() -> DateTime<Utc> {
    "2026-09-06T00:00:00Z".parse().unwrap()
}

pub(super) fn event(value: u8) -> ConversationEventIdentity {
    ConversationEventIdentity::new(MessageId::from_bytes([value; 32]), timestamp()).unwrap()
}

pub(super) fn provenance(audience: ConversationAudienceV1, source: u8) -> ConversationProvenanceV1 {
    ConversationProvenanceV1::new(
        audience,
        event(source),
        ConversationMemoryDigest::from_bytes([13; 32]),
    )
    .unwrap()
}

pub(super) fn content(record: &mut ReviewedConversationRecord, text: &str) {
    record.content = text.to_owned();
    record.pin.content_hash = hex::encode(Sha256::digest(text.as_bytes()));
}

pub(super) fn set_provenance(record: &mut ReviewedConversationRecord, p: ConversationProvenanceV1) {
    record.pin.conversation_audience_hash = p.audience().audience_hash().unwrap().to_hex();
    record.pin.source_hash = p.source_hash().unwrap().to_hex();
    record.provenance = String::from_utf8(p.canonical_bytes().unwrap()).unwrap();
}

pub(super) fn conversation_record(
    context: &mut ReviewedConversationContext,
) -> &mut ReviewedConversationRecord {
    match &mut context.records[0] {
        ReviewedContextRecord::Conversation { record } => record,
        _ => panic!("fixture conversation record"),
    }
}

pub(super) fn project_record(id: u128) -> ReviewedMemoryRecord {
    let text = "Unchanged approved project fact".to_owned();
    ReviewedMemoryRecord {
        pin: ReviewedMemoryPin {
            fact_id: Uuid::from_u128(id),
            target_id: Uuid::from_u128(101),
            fact_version: 1,
            consumption_epoch: 9,
            content_hash: hex::encode(Sha256::digest(text.as_bytes())),
            source_hash: "b".repeat(64),
            binding_hash: "c".repeat(64),
            approval_id: Uuid::from_u128(102),
            approved_by: "d".repeat(64),
            expires_at: timestamp() + chrono::Duration::days(1),
        },
        content: text,
    }
}

impl Fixture {
    pub(super) fn new(work: bool) -> Self {
        let run = Uuid::from_u128(20);
        let project = Uuid::from_u128(12);
        let origin = work.then_some(crate::authority::WorkRunOrigin {
            run_id: run,
            work_item_id: Uuid::from_u128(21),
            project_id: project,
            execution_version: 2,
            definition_hash: "a".repeat(64),
        });
        let authority = authority_for(Uuid::from_u128(10), Uuid::from_u128(11), "question", origin);
        let audience = ConversationAudienceV1::thread(
            authority.company_id(),
            Uuid::from_u128(7),
            project,
            authority.employee_id().clone(),
            Uuid::from_u128(6),
            event(8),
        )
        .unwrap();
        let observed = provenance(audience.clone(), 4);
        let origin = ConversationMemoryOrigin::from_observation(
            &[9; 32],
            &observed.canonical_bytes().unwrap(),
        )
        .unwrap();
        let p = provenance(audience, 10);
        let mut record = ReviewedConversationRecord {
            pin: ReviewedConversationPin {
                fact_id: Uuid::from_u128(100),
                target_id: Uuid::from_u128(101),
                fact_version: 1,
                consumption_epoch: 0,
                content_hash: String::new(),
                source_hash: String::new(),
                binding_hash: "c".repeat(64),
                approval_id: Uuid::from_u128(102),
                approved_by: "d".repeat(64),
                expires_at: timestamp() + chrono::Duration::days(1),
                conversation_audience_hash: String::new(),
                conversation_authority_epoch: 3,
                conversation_consumption_epoch: 4,
            },
            content: String::new(),
            provenance: String::new(),
        };
        content(&mut record, "Reviewed conversation fact");
        set_provenance(&mut record, p);
        Self {
            authority,
            run,
            context: ReviewedConversationContext {
                origin,
                records: vec![ReviewedContextRecord::Conversation { record }],
                truncated: false,
            },
        }
    }

    pub(super) fn base(&self) -> FrozenRunSnapshot {
        FrozenRunSnapshot::from_recall(&self.authority, self.run, recall(&self.authority, self.run))
            .unwrap()
    }

    pub(super) fn empty_base(&self) -> FrozenRunSnapshot {
        FrozenRunSnapshot::from_recall(
            &self.authority,
            self.run,
            MemoryRecall {
                records: vec![],
                truncated: false,
            },
        )
        .unwrap()
    }

    pub(super) fn snapshot(&self) -> FrozenRunSnapshot {
        self.base()
            .with_conversation(&self.authority, self.context.clone())
            .unwrap()
    }

    pub(super) fn channel_audience(&self) -> ConversationAudienceV1 {
        let origin = self.context.origin.parsed_provenance().unwrap();
        let a = origin.audience();
        ConversationAudienceV1::channel(
            a.company_id(),
            a.community_id(),
            a.project_id(),
            a.employee_id().clone(),
            a.channel_id(),
        )
        .unwrap()
    }
}
