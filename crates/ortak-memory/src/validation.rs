use crate::{
    gate::IoGate, invalid, rejected, unavailable, unsupported, HonchoMemoryAdapter, MemoryError,
};
use chrono::{DateTime, Utc};
use ortak_control::memory::{
    MemoryBudget, MemoryCapability, MemoryFact, MemoryProvenance, MemoryRecallRequest, MemoryScope,
    MemoryWriteReceipt, MemoryWriteRequest,
};
use ortak_domain::{EmployeeId, MemoryBinding, ProvisioningMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Explicit, journalable memory-roundtrip request. Persist run/time before retrying.
#[derive(Clone)]
pub struct MemoryRoundtripRequest {
    /// Allowed employee to validate.
    pub employee_id: EmployeeId,
    /// Exact server-selected binding.
    pub binding: MemoryBinding,
    /// Fresh diagnostic run namespace; never reuse a work run.
    pub run_id: Uuid,
    /// Stable provenance timestamp, preserved on retry.
    pub recorded_at: DateTime<Utc>,
}

/// Evidence of actual memory I/O, not external provider requests or model quality.
#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryRoundtripReceipt {
    /// Server-resolved company whose memory was exercised.
    pub company_id: Uuid,
    /// Exact secret-free binding exercised by this observation.
    pub binding: MemoryBinding,
    /// Explicit deployment whose binding was validated.
    pub deployment_id: Uuid,
    /// Validated employee identity.
    pub employee_id: EmployeeId,
    /// Diagnostic scratch scope.
    pub scope: MemoryScope,
    /// Canonical durable write acknowledgement.
    pub write_receipt: MemoryWriteReceipt,
    /// Time the matching scoped recall completed.
    pub validated_at: DateTime<Utc>,
    /// Advisory wall-clock expiration; execution uses a monotonic deadline.
    pub expires_at: DateTime<Utc>,
}

impl HonchoMemoryAdapter {
    /// Writes one diagnostic fact and validates an exact, nonempty scoped recall.
    ///
    /// This explicit operation is allowed only for fresh extension-owned bundles.
    /// It is never called by health, capability probing or adoption. Persist the
    /// supplied diagnostic run/time before calling; retries use the same receipt.
    pub async fn validate_memory_roundtrip(
        &self,
        request: &MemoryRoundtripRequest,
    ) -> Result<MemoryRoundtripReceipt, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            if allowed.mode != ProvisioningMode::Create
                || request.run_id.is_nil()
                || !self
                    .creation_receipts
                    .lock()
                    .map_err(|_| unavailable("memory creation receipt state unavailable"))?
                    .contains_key(&allowed.employee_id)
            {
                return Err(unsupported(MemoryCapability::Remember));
            }
            let generation = self.begin_validation(allowed)?;
            let gate = IoGate::Validation(generation);
            self.protocol().await?;
            self.require_owned(allowed).await?;
            let scope = MemoryScope::RunScratch {
                run_id: request.run_id,
            };
            let text = format!(
                "Ortak memory roundtrip {} {}",
                self.config.deployment.deployment_id, request.run_id
            );
            let write = MemoryWriteRequest {
                employee_id: allowed.employee_id.clone(),
                binding: allowed.binding.clone(),
                scope: scope.clone(),
                facts: vec![MemoryFact {
                    content: text.clone(),
                    provenance: MemoryProvenance {
                        employee_id: allowed.employee_id.clone(),
                        run_id: Some(request.run_id),
                        source: "ortak_memory_roundtrip".into(),
                        recorded_at: request.recorded_at,
                    },
                }],
                idempotency_key: format!(
                    "roundtrip:{}:{}",
                    self.config.deployment.deployment_id, request.run_id
                ),
            };
            let (write_receipt, records) = self.write_on(allowed, &write, gate).await?;
            let recalled = self
                .recall_on(
                    allowed,
                    &MemoryRecallRequest {
                        employee_id: allowed.employee_id.clone(),
                        binding: allowed.binding.clone(),
                        scope: scope.clone(),
                        query: text,
                        budget: MemoryBudget {
                            max_records: 1,
                            max_bytes: 4096,
                        },
                    },
                    gate,
                )
                .await?;
            if recalled.records != records {
                return Err(rejected(
                    "memory roundtrip did not recall the canonical written record",
                ));
            }
            self.require_owned(allowed).await?;
            let validated_at = Utc::now();
            let expiry = chrono::Duration::from_std(self.config.witness_lifetime)
                .map_err(|_| invalid("invalid validation lifetime"))?;
            self.publish_validation(allowed, generation)?;
            Ok(MemoryRoundtripReceipt {
                company_id: self.company_id,
                binding: allowed.binding.clone(),
                deployment_id: self.config.deployment.deployment_id,
                employee_id: allowed.employee_id.clone(),
                scope,
                write_receipt,
                validated_at,
                expires_at: validated_at + expiry,
            })
        })
        .await
    }
}
