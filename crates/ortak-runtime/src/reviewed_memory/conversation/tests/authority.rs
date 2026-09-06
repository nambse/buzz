use super::*;
use crate::authority::{
    RunInput, StoredMemoryBinding, StoredRuntimeBinding, validate_pinned_revision,
};
use ortak_domain::{EmployeeManifest, EmployeeStatus};

pub(super) fn authority_for(
    company: Uuid,
    lease: Uuid,
    input: &str,
    work: Option<crate::authority::WorkRunOrigin>,
) -> DispatchAuthority {
    let manifest: EmployeeManifest =
        serde_yaml::from_str(include_str!("../../../../../../config/employees/cem.yaml"))
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
