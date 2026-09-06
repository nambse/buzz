use super::*;
use chrono::{Duration, SecondsFormat, Utc};
use ortak_control::memory::employee::{
    EmployeeMemoryAudienceV1, EmployeeMemoryDigest, EmployeeMemoryProvenanceV1,
    EmployeeMemorySourceV1, EmployeeSharingApprovalV1,
};
use ortak_control::{MessageId, office_identity::OfficePublicKey};

struct Fixture {
    authority: DispatchAuthority,
    run: Uuid,
    origin: EmployeeMemoryOrigin,
}
impl Fixture {
    fn new(work: bool) -> Self {
        let run = Uuid::from_u128(20);
        let work = work.then_some(crate::authority::WorkRunOrigin {
            run_id: run,
            work_item_id: Uuid::from_u128(21),
            project_id: Uuid::from_u128(12),
            execution_version: 2,
            definition_hash: "a".repeat(64),
        });
        let authority = crate::memory_context::tests::authority_for(
            Uuid::from_u128(10),
            Uuid::from_u128(11),
            "question",
            work,
        );
        let value = serde_json::json!({"company_id":authority.company_id(),"destination_authority_epoch":3,
            "destination_channel_id":authority.input().channel_id,"employee_id":authority.employee_id(),
            "format":"ortak-reviewed-employee-run-origin/1","requester_public_key":"09".repeat(32),
            "source":{"author_public_key":"09".repeat(32),"channel_id":Uuid::from_u128(6),"community_id":Uuid::from_u128(7),
                "event_created_at":"2026-09-06T00:00:00.000000Z","event_id":"04".repeat(32),"evidence_hash":"0d".repeat(32)},
            "source_authority_epoch":3});
        Self {
            authority,
            run,
            origin: EmployeeMemoryOrigin::from_observation(&serde_json::to_vec(&value).unwrap())
                .unwrap(),
        }
    }
    fn record(&self, id: u128, relationship: bool, content: &str) -> ReviewedEmployeeRecord {
        let human = OfficePublicKey::parse_hex(&"09".repeat(32)).unwrap();
        let a = if relationship {
            EmployeeMemoryAudienceV1::relationship(
                self.authority.company_id(),
                self.authority.employee_id().clone(),
                Uuid::from_u128(7),
                Uuid::from_u128(6),
                human,
            )
            .unwrap()
        } else {
            EmployeeMemoryAudienceV1::experience(
                self.authority.company_id(),
                self.authority.employee_id().clone(),
                Uuid::from_u128(7),
                Uuid::from_u128(6),
            )
            .unwrap()
        };
        let at = "2026-09-06T00:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        let p = EmployeeMemoryProvenanceV1::new(
            a,
            EmployeeMemorySourceV1::new(
                Uuid::from_u128(7),
                Uuid::from_u128(6),
                MessageId::from_bytes([14; 32]),
                at,
                human,
                EmployeeMemoryDigest::from_bytes([15; 32]),
            )
            .unwrap(),
            EmployeeSharingApprovalV1::new(
                Uuid::from_u128(id + 1000),
                human,
                EmployeeMemoryDigest::from_bytes(Sha256::digest(content.as_bytes()).into()),
                at + Duration::days(1),
            )
            .unwrap(),
        )
        .unwrap();
        let namespace=serde_json::to_vec(&serde_json::json!({"company_id":self.authority.company_id(),"employee_id":self.authority.employee_id(),
            "format":"ortak-reviewed-employee-namespace/1"})).unwrap();
        ReviewedEmployeeRecord {
            pin: ReviewedEmployeePin {
                fact_id: Uuid::from_u128(id),
                target_id: Uuid::from_u128(500),
                fact_version: 1,
                content_hash: p.approval().content_hash().to_hex(),
                source_hash: p.source_hash().unwrap().to_hex(),
                sharing_hash: p.sharing_hash().unwrap().to_hex(),
                audience_hash: p.audience().audience_hash().unwrap().to_hex(),
                binding_hash: "b".repeat(64),
                namespace_hash: hex::encode(Sha256::digest(namespace)),
                approval_id: p.approval().approval_id(),
                approved_by: human.to_hex(),
                expires_at: p.approval().expires_at(),
                source_authority_epoch: 3,
                destination_authority_epoch: 3,
                consumption_epoch: 7,
            },
            content: content.into(),
            provenance: String::from_utf8(p.canonical_bytes().unwrap()).unwrap(),
        }
    }
    fn context(&self, records: Vec<ReviewedEmployeeRecord>) -> ReviewedEmployeeContext {
        ReviewedEmployeeContext {
            origin: self.origin.clone(),
            conversation_origin: None,
            records: records
                .into_iter()
                .map(|record| EmployeeContextRecord::Employee { record })
                .collect(),
            truncated: false,
        }
    }
    fn base(&self) -> FrozenRunSnapshot {
        let mut scratch = crate::memory_context::tests::recall(&self.authority, self.run);
        scratch.records[0].content = "scratch\0value\u{1}".into();
        FrozenRunSnapshot::from_recall(&self.authority, self.run, scratch).unwrap()
    }
}

#[test]
fn employee_v5_preserves_legacy_bytes_and_exact_project_rendering() {
    let f = Fixture::new(true);
    let record = f.record(100, false, "approved employee fact");
    let project = ReviewedMemoryRecord {
        pin: ReviewedMemoryPin {
            fact_id: Uuid::from_u128(200),
            target_id: Uuid::from_u128(201),
            fact_version: 1,
            consumption_epoch: 8,
            content_hash: record.pin.content_hash.clone(),
            source_hash: "c".repeat(64),
            binding_hash: "d".repeat(64),
            approval_id: Uuid::from_u128(202),
            approved_by: "e".repeat(64),
            expires_at: record.pin.expires_at,
        },
        content: record.content.clone(),
    };
    let legacy = f
        .base()
        .with_reviewed(
            &f.authority,
            ReviewedMemoryContext {
                records: vec![project],
                truncated: false,
            },
        )
        .unwrap();
    let original = legacy.encode().unwrap();
    let expected = legacy.spec().context.memory_context[1].clone();
    let v5 = legacy
        .clone()
        .with_employee(&f.authority, f.context(vec![record]))
        .unwrap();
    assert_eq!(legacy.encode().unwrap(), original);
    assert_eq!(v5.spec().context.memory_context[1], expected);
    assert!(v5.spec().context.memory_context[2].contains("scratch\\u0000value\\u0001"));
    assert!(v5.reviewed().is_none() && v5.conversation().is_none());
    let bytes = v5.encode().unwrap();
    assert_eq!(
        FrozenRunSnapshot::decode(&bytes, &f.authority, f.run)
            .unwrap()
            .encode()
            .unwrap(),
        bytes
    );
    for version in 1..=4 {
        let mut forged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        forged["version"] = version.into();
        assert!(
            FrozenRunSnapshot::decode(&serde_json::to_vec(&forged).unwrap(), &f.authority, f.run)
                .is_err()
        );
    }
}

#[test]
fn employee_v5_rejects_cross_human_destination_provenance_and_forged_content() {
    let f = Fixture::new(false);
    let good = f.context(vec![f.record(100, true, "relationship")]);
    assert!(f.base().with_employee(&f.authority, good.clone()).is_ok());
    for change in ["human", "destination", "content", "sharing", "epoch"] {
        let mut context = good.clone();
        if change == "human" || change == "destination" {
            let mut origin: serde_json::Value =
                serde_json::from_slice(context.origin.canonical_bytes()).unwrap();
            if change == "human" {
                origin["requester_public_key"] = "08".repeat(32).into();
                origin["source"]["author_public_key"] = "08".repeat(32).into();
            } else {
                origin["destination_channel_id"] = Uuid::from_u128(9).to_string().into();
            }
            context.origin =
                EmployeeMemoryOrigin::from_observation(&serde_json::to_vec(&origin).unwrap())
                    .unwrap();
        } else if let EmployeeContextRecord::Employee { record } = &mut context.records[0] {
            match change {
                "content" => record.content.push('!'),
                "sharing" => record.pin.sharing_hash = "f".repeat(64),
                _ => record.pin.destination_authority_epoch += 1,
            }
        }
        assert!(
            f.base().with_employee(&f.authority, context).is_err(),
            "{change}"
        );
    }
}

#[test]
fn employee_v5_enforces_priority_content_and_rendered_budgets() {
    let f = Fixture::new(false);
    let relationship = f.record(101, true, "relationship");
    let experience = f.record(100, false, "experience");
    assert!(
        f.base()
            .with_employee(
                &f.authority,
                f.context(vec![relationship.clone(), experience.clone()])
            )
            .is_ok()
    );
    assert!(
        f.base()
            .with_employee(&f.authority, f.context(vec![experience, relationship]))
            .is_err()
    );
    assert!(
        f.base()
            .with_employee(&f.authority, f.context(vec![]))
            .is_err()
    );
    assert!(
        f.base()
            .with_employee(
                &f.authority,
                f.context(vec![f.record(1, false, &"\"".repeat(4096))])
            )
            .is_err()
    );
    let eight = (1..=8).map(|id| f.record(id, false, "small")).collect();
    let full = f
        .base()
        .with_employee(&f.authority, f.context(eight))
        .unwrap();
    assert_eq!(full.spec().context.memory_context.len(), 8);
    assert!(full.wire.recall.records.is_empty());
    assert!(full.wire.recall.truncated);
    assert!(
        f.base()
            .with_employee(
                &f.authority,
                f.context((1..=9).map(|id| f.record(id, false, "small")).collect())
            )
            .is_err()
    );
    assert!(
        f.base()
            .with_employee(
                &f.authority,
                f.context(vec![
                    f.record(1, false, &"x".repeat(4096)),
                    f.record(2, false, &"y".repeat(4096)),
                    f.record(3, false, "z")
                ])
            )
            .is_err()
    );
}

#[test]
fn employee_run_origin_requires_canonical_partition_and_never_mints_sharing_approval() {
    let f = Fixture::new(false);
    let bytes = f.origin.canonical_bytes();
    assert!(
        !String::from_utf8(bytes.to_vec())
            .unwrap()
            .contains("approval")
    );
    let mut duplicate = String::from_utf8(bytes.to_vec()).unwrap();
    duplicate.insert_str(1, "\"format\":\"ortak-reviewed-employee-run-origin/1\",");
    assert!(EmployeeMemoryOrigin::from_observation(duplicate.as_bytes()).is_err());
    let pretty =
        serde_json::to_vec_pretty(&serde_json::from_slice::<serde_json::Value>(bytes).unwrap())
            .unwrap();
    assert!(EmployeeMemoryOrigin::from_observation(&pretty).is_err());
    for change in ["time", "unknown", "epoch"] {
        let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        match change {
            "time" => value["source"]["event_created_at"] = "2026-09-06T00:00:00.000000001Z".into(),
            "unknown" => value["project_id"] = Uuid::from_u128(12).to_string().into(),
            _ => value["source_authority_epoch"] = (-1).into(),
        }
        assert!(
            EmployeeMemoryOrigin::from_observation(&serde_json::to_vec(&value).unwrap()).is_err()
        );
    }
    assert_eq!(
        f.record(1, false, "record")
            .pin
            .expires_at
            .to_rfc3339_opts(SecondsFormat::Micros, true),
        "2026-09-07T00:00:00.000000Z"
    );
}
