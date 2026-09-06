use super::*;
use crate::authority::{
    RunInput, StoredMemoryBinding, StoredRuntimeBinding, validate_pinned_revision,
};
use ortak_control::memory::{MemoryProvenance, MemoryRecord};
use ortak_domain::{EmployeeManifest, EmployeeStatus};

pub(super) fn authority(company: Uuid, lease: Uuid, input: &str) -> DispatchAuthority {
    authority_for(company, lease, input, None)
}

pub(super) fn authority_for(
    company: Uuid,
    lease: Uuid,
    input: &str,
    work: Option<crate::authority::WorkRunOrigin>,
) -> DispatchAuthority {
    let manifest: EmployeeManifest =
        serde_yaml::from_str(include_str!("../../../../config/employees/cem.yaml"))
            .expect("manifest");
    let mut employee = manifest.employee;
    employee.status = EmployeeStatus::Active;
    let runtime = &employee.runtime;
    let stored = StoredRuntimeBinding {
        adapter: runtime.adapter.clone(),
        profile_ref: runtime.profile_ref.clone(),
        model: runtime.model.clone(),
        workspace_ref: runtime.workspace_ref.clone(),
        credential_refs: runtime
            .credential_refs
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect(),
        options: runtime.options.clone(),
        validated: true,
    };
    let memory = StoredMemoryBinding {
        binding: employee.memory.clone().expect("memory"),
        validated: true,
    };
    let config = validate_pinned_revision(
        &employee.id,
        employee.status,
        &serde_json::to_value(&employee).expect("json"),
        Some(&stored),
    )
    .expect("configuration")
    .with_validated_memory(Some(&memory), employee.memory.as_ref())
    .expect("memory validation");
    if let Some(work) = work {
        return DispatchAuthority::from_work(
            company,
            Uuid::from_u128(1),
            lease,
            employee.id,
            Uuid::from_u128(3),
            config,
            RunInput {
                body: input.to_owned(),
                truncated: false,
                channel_id: Some(Uuid::from_u128(6)),
                event_kind: 0,
            },
            work,
            0,
        );
    }
    DispatchAuthority::new(
        company,
        Uuid::from_u128(1),
        lease,
        Uuid::from_u128(2),
        employee.id,
        Uuid::from_u128(3),
        ortak_control::MessageId::from_bytes([4; 32]),
        ortak_control::MessageId::from_bytes([5; 32]),
        config,
        RunInput {
            body: input.to_owned(),
            truncated: false,
            channel_id: Some(Uuid::from_u128(6)),
            event_kind: 9,
        },
    )
}

pub(super) fn recall(authority: &DispatchAuthority, run_id: Uuid) -> MemoryRecall {
    MemoryRecall {
        records: vec![MemoryRecord {
            record_ref: "memory-record-1".to_owned(),
            scope: MemoryScope::RunScratch { run_id },
            content: "Previously checked this run's input".to_owned(),
            provenance: MemoryProvenance {
                employee_id: authority.employee_id().clone(),
                run_id: Some(run_id),
                source: "run_scratch".to_owned(),
                recorded_at: chrono::Utc::now(),
            },
        }],
        truncated: false,
    }
}

#[test]
fn snapshot_reuses_exact_bytes_after_lease_renewal_but_rejects_changed_source_or_company() {
    let company = Uuid::new_v4();
    let first = authority(company, Uuid::new_v4(), "question");
    let run = Uuid::new_v4();
    let snapshot =
        FrozenRunSnapshot::from_recall(&first, run, recall(&first, run)).expect("snapshot");
    let bytes = snapshot.encode().expect("bytes");
    let renewed = authority(company, Uuid::new_v4(), "question");
    let loaded = FrozenRunSnapshot::decode(&bytes, &renewed, run).expect("renewed lease");
    assert_eq!(loaded.encode().expect("same bytes"), bytes);
    assert_eq!(loaded.spec(), snapshot.spec());
    assert!(
        FrozenRunSnapshot::decode(&bytes, &authority(company, Uuid::new_v4(), "changed"), run)
            .is_err()
    );
    assert!(
        FrozenRunSnapshot::decode(
            &bytes,
            &authority(Uuid::new_v4(), Uuid::new_v4(), "question"),
            run
        )
        .is_err()
    );
    let mut injected: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    injected["spec"]["input"] = serde_json::json!("injected input");
    assert!(
        FrozenRunSnapshot::decode(&serde_json::to_vec(&injected).expect("json"), &first, run)
            .is_err()
    );
}

#[test]
fn snapshot_rejects_foreign_scope_provenance_duplicates_and_oversized_records() {
    let authority = authority(Uuid::new_v4(), Uuid::new_v4(), "question");
    let run = Uuid::new_v4();
    let valid = recall(&authority, run);
    for invalid in 0..5 {
        let mut candidate = valid.clone();
        match invalid {
            0 => candidate.records[0].scope = MemoryScope::EmployeeExperience,
            1 => candidate.records[0].provenance.run_id = Some(Uuid::new_v4()),
            2 => {
                candidate.records[0].provenance.employee_id =
                    ortak_domain::EmployeeId::parse("other").expect("id")
            }
            3 => candidate.records.push(candidate.records[0].clone()),
            _ => candidate.records[0].content = "x".repeat(4097),
        }
        assert!(
            FrozenRunSnapshot::from_recall(&authority, run, candidate).is_err(),
            "case {invalid}"
        );
    }
}

#[tokio::test]
async fn required_memory_has_no_implicit_disabled_fallback() {
    let authority = authority(Uuid::new_v4(), Uuid::new_v4(), "question");
    assert_eq!(
        NoRunMemory.check(&authority).await,
        Err(DispatchRefusal::MemoryAdapterUnavailable)
    );
}

#[test]
fn reviewed_snapshot_preserves_legacy_bytes_and_fences_typed_scope_hash_and_total_budget() {
    use sha2::{Digest, Sha256};
    let company = Uuid::new_v4();
    let run = Uuid::new_v4();
    let origin = crate::authority::WorkRunOrigin {
        run_id: run,
        work_item_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        execution_version: 2,
        definition_hash: "a".repeat(64),
    };
    let a = authority_for(company, Uuid::new_v4(), "work", Some(origin.clone()));
    let legacy = FrozenRunSnapshot::from_recall(&a, run, recall(&a, run)).unwrap();
    let bytes = legacy.encode().unwrap();
    let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["version"], 2);
    assert!(wire.get("conversation").is_none());
    assert!(wire.get("reviewed").is_none());
    let renewed = authority_for(company, Uuid::new_v4(), "work", Some(origin));
    assert_eq!(
        FrozenRunSnapshot::decode(&bytes, &renewed, run)
            .unwrap()
            .encode()
            .unwrap(),
        bytes
    );
    let content = "Approved project fact".to_owned();
    let record = ReviewedMemoryRecord {
        content: content.clone(),
        pin: ReviewedMemoryPin {
            fact_id: Uuid::new_v4(),
            target_id: Uuid::new_v4(),
            fact_version: 1,
            consumption_epoch: 0,
            content_hash: hex::encode(Sha256::digest(content.as_bytes())),
            source_hash: "b".repeat(64),
            binding_hash: "c".repeat(64),
            approval_id: Uuid::new_v4(),
            approved_by: "d".repeat(64),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
        },
    };
    let context = ReviewedMemoryContext {
        records: vec![record.clone()],
        truncated: false,
    };
    let snapshot = legacy.clone().with_reviewed(&a, context.clone()).unwrap();
    assert_eq!(snapshot.spec().context.memory_context.len(), 2);
    let encoded = snapshot.encode().unwrap();
    assert!(
        serde_json::from_slice::<serde_json::Value>(&encoded)
            .unwrap()
            .get("conversation")
            .is_none()
    );
    assert_eq!(
        FrozenRunSnapshot::decode(&encoded, &renewed, run)
            .unwrap()
            .encode()
            .unwrap(),
        encoded
    );
    assert!(
        FrozenRunSnapshot::decode(&encoded, &authority(company, Uuid::new_v4(), "work"), run)
            .is_err()
    );
    let mut changed = context.clone();
    changed.records[0].content.push_str(" tampered");
    assert!(legacy.clone().with_reviewed(&a, changed).is_err());
    let mut duplicate = context.clone();
    duplicate.records.push(record.clone());
    assert!(legacy.clone().with_reviewed(&a, duplicate).is_err());
    let mut many = context;
    many.records.clear();
    for _ in 0..8 {
        let mut item = record.clone();
        item.pin.fact_id = Uuid::new_v4();
        many.records.push(item);
    }
    let full = legacy.with_reviewed(&a, many).unwrap();
    assert_eq!(full.spec().context.memory_context.len(), 8);
    assert!(full.wire.recall.truncated && full.wire.recall.records.is_empty());
}
