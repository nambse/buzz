//! Explicit selected-profile diagnostics; ordinary health remains read-only.
use super::*;

/// Closed durable bridge status for an explicitly admitted connection check.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileProbeStatus {
    /// Reserved before external execution.
    Accepted,
    /// The selected profile is executing its bounded diagnostic.
    Running,
    /// Execution completed; containment and current health still require checks.
    Completed,
    /// Execution failed; containment still requires a check.
    Failed,
    /// Cancellation is pending containment.
    Cancelling,
    /// Cancellation completed.
    Cancelled,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeReceipt {
    runtime_run_ref: RuntimeRunRef,
    started_at: DateTime<Utc>,
    status: ProfileProbeStatus,
}

impl HermesAdapter {
    /// Explicitly admits one diagnostic using an identity already persisted by
    /// the caller. Retrying this exact ID never starts a second bridge child.
    /// No ordinary health, credential-read or capability call invokes this path.
    pub async fn start_profile_probe(
        &self,
        binding: &RuntimeBinding,
        probe_id: Uuid,
    ) -> Result<ProfileProbeStatus, RuntimeError> {
        if probe_id.is_nil() || binding.adapter != "hermes" || binding.profile_ref.is_none() {
            return Err(invalid());
        }
        let receipt: ProbeReceipt = self
            .request(
                Method::POST,
                "/v1/profiles/probe",
                Some(json!({"company_id":self.company_id,"binding":binding,"probe_id":probe_id})),
            )
            .await?
            .ok_or_else(unavailable)?;
        self.probe_receipt(probe_id, receipt)
    }

    /// Reads only the exact diagnostic identity; missing means no admission was
    /// observed, never permission to replace an uncertain child with a new ID.
    pub async fn profile_probe_status(
        &self,
        probe_id: Uuid,
    ) -> Result<Option<ProfileProbeStatus>, RuntimeError> {
        if probe_id.is_nil() {
            return Err(invalid());
        }
        self.request(
            Method::POST,
            "/v1/runs/lookup",
            Some(json!({
                "company_id":self.company_id,"run_id":probe_id,
                "idempotency_key":crate::run_idempotency_key(self.company_id,probe_id)
            })),
        )
        .await?
        .map(|receipt| self.probe_receipt(probe_id, receipt))
        .transpose()
    }

    /// Stops the exact diagnostic, including a lost admission acknowledgment.
    /// The bridge acknowledges only after proving child containment, even when
    /// its execution journal already reports a terminal result.
    pub async fn stop_profile_probe(&self, probe_id: Uuid) -> Result<(), RuntimeError> {
        if probe_id.is_nil() {
            return Err(invalid());
        }
        let receipt = self
            .cancel_start(
                &crate::run_idempotency_key(self.company_id, probe_id),
                "provisioning profile diagnostic containment",
            )
            .await?;
        if receipt.runtime_run_ref != Some(self.reference(probe_id)) {
            return Err(invalid());
        }
        Ok(())
    }

    fn probe_receipt(
        &self,
        probe_id: Uuid,
        receipt: ProbeReceipt,
    ) -> Result<ProfileProbeStatus, RuntimeError> {
        if receipt.runtime_run_ref != self.reference(probe_id)
            || receipt.started_at > Utc::now() + chrono::Duration::seconds(30)
        {
            return Err(invalid());
        }
        Ok(receipt.status)
    }
}
