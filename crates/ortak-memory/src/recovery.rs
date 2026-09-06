//! Durable extension ownership, independent from saga acquisition ownership.

use std::collections::BTreeMap;

use ortak_control::memory::{MemoryResourceOutcome, MemoryResourceRequest};
use ortak_domain::{EmployeeId, MemoryBinding, ProvisioningMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{config, invalid, rejected, resources, unavailable, HonchoMemoryAdapter, MemoryError};

/// Immutable native resource identities from the extension's original create receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HonchoNativeResourceIds {
    /// Native workspace identity, independent from its public name.
    pub workspace: String,
    /// Exact public peer names and their distinct native identities.
    pub peers: BTreeMap<String, String>,
}

impl HonchoNativeResourceIds {
    pub(crate) fn matches_binding(&self, binding: &MemoryBinding) -> bool {
        config::name(&self.workspace)
            && self.peers.len() == 2
            && [&binding.user_peer, &binding.employee_peer]
                .iter()
                .all(|name| self.peers.get(*name).is_some_and(|id| config::name(id)))
            && self.peers.get(&binding.user_peer) != self.peers.get(&binding.employee_peer)
    }
}

/// Journalable original extension creation evidence; never an execution witness.
///
/// Persist this with the original bootstrap intent. Deserialization grants no
/// authority: recovery checks the server-authorized binding and compares every
/// identity against the selected extension's current read-only inspection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HonchoCreatedResourcesReceipt {
    /// Original server-resolved company.
    pub company_id: Uuid,
    /// Selected deployment that created these resources.
    pub deployment_id: Uuid,
    /// Original stable employee identity.
    pub employee_id: EmployeeId,
    /// Complete immutable memory binding.
    pub binding: MemoryBinding,
    /// Original durable bootstrap create key; never the later saga Adopt key.
    pub creation_key: String,
    /// Extension-issued hash of the canonical original create request.
    pub request_hash: String,
    /// Original frozen native workspace and peer identities.
    pub native_ids: HonchoNativeResourceIds,
    /// Normalized original created resource receipt, independent from adoption.
    pub resources: MemoryResourceOutcome,
}

impl HonchoMemoryAdapter {
    /// Exports retained original creation evidence after read-only live verification.
    ///
    /// Call after successful explicit creation and persist the result before
    /// handing the bundle to an adopting saga or worker. This cannot discover or
    /// adopt arbitrary native resources, and never grants an I/O witness.
    pub async fn created_resources_receipt(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<HonchoCreatedResourcesReceipt, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            if allowed.mode != ProvisioningMode::Create
                || request.mode != ProvisioningMode::Create
                || !config::key(&request.idempotency_key)
            {
                return Err(invalid("invalid original memory receipt request"));
            }
            let identity = self
                .creation_receipts
                .lock()
                .map_err(|_| unavailable("memory creation receipt state unavailable"))?
                .get(&allowed.employee_id)
                .cloned()
                .ok_or_else(|| rejected("original memory creation receipt is unavailable"))?;
            let body = self.creation_body(allowed, &request.idempotency_key);
            if identity.request_hash != crate::wire::fingerprint(&body)? {
                return Err(rejected(
                    "memory ownership receipt differs from create request",
                ));
            }
            self.protocol().await?;
            if self.inspect_owned_identity(allowed).await? != identity {
                return Err(rejected("memory native resource identity changed"));
            }
            Ok(HonchoCreatedResourcesReceipt {
                company_id: self.company_id,
                deployment_id: self.config.deployment.deployment_id,
                employee_id: allowed.employee_id.clone(),
                binding: allowed.binding.clone(),
                creation_key: request.idempotency_key.clone(),
                request_hash: identity.request_hash,
                native_ids: identity.native_ids,
                resources: resources::outcome(allowed, true),
            })
        })
        .await
    }

    /// Recovers a journaled extension-owned bundle without creating or writing it.
    ///
    /// The caller must authorize this operation and supply the exact original
    /// durable bootstrap receipt. Company, deployment, binding, create hash and
    /// immutable native IDs must match the selected live extension. A configured
    /// Adopt acquisition returns only adopted outcomes, so its compensation has
    /// no created resources. Explicit roundtrip validation remains a separate
    /// required operation, including after every process restart.
    pub async fn recover_created_resources(
        &self,
        receipt: &HonchoCreatedResourcesReceipt,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&receipt.employee_id), &receipt.binding)?;
            if receipt.company_id != self.company_id
                || receipt.deployment_id != self.config.deployment.deployment_id
                || !config::key(&receipt.creation_key)
                || receipt.resources != resources::outcome(allowed, true)
                || !receipt.native_ids.matches_binding(&allowed.binding)
                || receipt.request_hash
                    != crate::wire::fingerprint(
                        &self.creation_body(allowed, &receipt.creation_key),
                    )?
            {
                return Err(invalid(
                    "memory creation receipt differs from authorized selection",
                ));
            }
            let expected = resources::ResourceIdentity {
                request_hash: receipt.request_hash.clone(),
                native_ids: receipt.native_ids.clone(),
            };
            self.protocol().await?;
            if self.inspect_owned_identity(allowed).await? != expected {
                return Err(rejected("memory original creation identity changed"));
            }
            self.retain_verified_identity(allowed, expected)?;
            Ok(resources::outcome(
                allowed,
                allowed.mode == ProvisioningMode::Create,
            ))
        })
        .await
    }
}
