use super::*;
use crate::authority::{
    validate_pinned_revision, RunInput, StoredMemoryBinding, StoredRuntimeBinding,
};
use ortak_control::memory::{MemoryProvenance, MemoryRecord};
use ortak_domain::{EmployeeManifest, EmployeeStatus};

fn authority(company: Uuid, lease: Uuid, input: &str) -> DispatchAuthority {
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

fn recall(authority: &DispatchAuthority, run_id: Uuid) -> MemoryRecall {
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
    assert!(FrozenRunSnapshot::decode(
        &bytes,
        &authority(Uuid::new_v4(), Uuid::new_v4(), "question"),
        run
    )
    .is_err());
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
