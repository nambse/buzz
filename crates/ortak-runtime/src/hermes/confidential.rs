//! Separate protected transport. These methods grant no current source/run right
//! and never deserialize ciphertext into an ordinary runtime event or snapshot.
use super::*;
use crate::confidential::ConfidentialStartBody;
use ortak_control::confidential::{ConfidentialEnvelope, PayloadPurpose, ValidatedIdentity};
use serde_json::value::RawValue;

/// Metadata-only state, including keyless failure and containment settlement.
#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConfidentialRunStatus {
    /// Durable snapshot reservation precedes process launch.
    Accepted,
    /// The bounded worker crossed its provider admission boundary.
    Running,
    /// Output envelopes and final metadata committed together.
    Completed,
    /// A closed failure receipt is retained.
    Failed,
    /// Stop requested; containment has not yet been acknowledged.
    Cancelling,
    /// The process owner confirmed containment.
    Cancelled,
}

/// Exact runtime reference and closed status; contains no private content.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfidentialRunReceipt {
    /// Stable company/run reference checked against the requested run.
    pub runtime_run_ref: RuntimeRunRef,
    /// Original reservation time, preserved on replay.
    pub started_at: DateTime<Utc>,
    /// Current metadata-only journal state.
    pub status: ConfidentialRunStatus,
}

/// Closed failure/cancellation code without provider messages or key material.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfidentialFailure {
    /// Validated finite code shared with the bridge's protected journal.
    pub code: String,
    /// Time the closed receipt was committed.
    pub occurred_at: DateTime<Utc>,
}

/// Canonical protected event; decryption requires a separate current authority check.
pub struct ConfidentialEvent {
    /// Dense immutable ordinal from 1 through 512.
    pub ordinal: u32,
    /// Metadata time; opening must match the authenticated inner event time.
    pub occurred_at: DateTime<Utc>,
    /// Exact envelope bytes; PostgreSQL must copy these without re-encryption.
    pub envelope: ConfidentialEnvelope,
}

/// Bounded replay page with independent keyless stop/failure metadata.
pub struct ConfidentialEventBatch {
    /// At most four consecutive protected event envelopes.
    pub events: Vec<ConfidentialEvent>,
    /// Current journal state, never inferred from an empty event list.
    pub status: ConfidentialRunStatus,
    /// Exact closed receipt for failed/cancelled state only.
    pub failure: Option<ConfidentialFailure>,
    /// True only after all events of a terminal run were returned.
    pub terminal: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBatch {
    events: Vec<WireProtectedEvent>,
    status: ConfidentialRunStatus,
    failure: Option<ConfidentialFailure>,
    terminal: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProtectedEvent {
    cursor: String,
    occurred_at: DateTime<Utc>,
    envelope: Box<RawValue>,
}
#[derive(Deserialize)]
struct SelectedIdentity {
    company_id: Uuid,
    run_id: Uuid,
}

impl HermesAdapter {
    async fn protected_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<Option<T>, RuntimeError> {
        let url = self.origin.join(path).map_err(|_| invalid())?;
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            if body.len() > 112 * 1024 {
                return Err(invalid());
            }
            // reqwest owns a transient HTTP buffer; no request body is logged,
            // fingerprinted or written to the bridge's durable plaintext tables.
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        let mut response = request.send().await.map_err(|_| unavailable())?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(unavailable());
        }
        const MAX: usize = 512 * 1024;
        if response.content_length().is_some_and(|n| n > MAX as u64) {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len().saturating_add(chunk.len()) > MAX {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| unavailable())
    }

    fn protected_receipt(
        &self,
        run: Uuid,
        receipt: ConfidentialRunReceipt,
    ) -> Result<ConfidentialRunReceipt, RuntimeError> {
        if receipt.runtime_run_ref != self.reference(run) {
            return Err(invalid());
        }
        Ok(receipt)
    }

    /// Send a newly authorized protected start; never falls back to ordinary runs.
    /// Capability/image and current lease checks belong to the central caller.
    pub async fn start_confidential(
        &self,
        body: ConfidentialStartBody,
    ) -> Result<ConfidentialRunReceipt, RuntimeError> {
        if body.company_id != self.company_id {
            return Err(invalid());
        }
        let receipt = self
            .protected_request(Method::POST, "/v1/confidential/runs", Some(&body.bytes))
            .await?
            .ok_or_else(unavailable)?;
        self.protected_receipt(body.run_id, receipt)
    }

    /// Re-observe an exact committed snapshot without keys or execution admission.
    pub async fn replay_confidential(
        &self,
        expected: &ValidatedIdentity,
        snapshot: &ConfidentialEnvelope,
    ) -> Result<Option<ConfidentialRunReceipt>, RuntimeError> {
        snapshot
            .header()
            .require_expected(expected, PayloadPurpose::Snapshot, 0)
            .map_err(|_| invalid())?;
        let selected: SelectedIdentity =
            serde_json::from_slice(expected.canonical_bytes()).map_err(|_| invalid())?;
        if selected.company_id != self.company_id {
            return Err(invalid());
        }
        let body=serde_json::to_vec(&json!({"company_id":self.company_id,
            "snapshot":serde_json::from_slice::<Value>(snapshot.canonical_bytes()).map_err(|_|invalid())?})).map_err(|_|invalid())?;
        self.protected_request(Method::POST, "/v1/confidential/runs/replay", Some(&body))
            .await?
            .map(|r| self.protected_receipt(selected.run_id, r))
            .transpose()
    }

    /// Read only a known protected reservation or pre-start cancellation tombstone.
    pub async fn lookup_confidential(
        &self,
        key: &str,
    ) -> Result<Option<ConfidentialRunReceipt>, RuntimeError> {
        let run = self.run_id(key)?;
        let body = serde_json::to_vec(
            &json!({"company_id":self.company_id,"run_id":run,"idempotency_key":key}),
        )
        .map_err(|_| invalid())?;
        self.protected_request(Method::POST, "/v1/confidential/runs/lookup", Some(&body))
            .await?
            .map(|r| self.protected_receipt(run, r))
            .transpose()
    }

    /// Keyless stop preserves the original execution owner's containment proof.
    pub async fn cancel_confidential(&self, key: &str) -> Result<CancelOutcome, RuntimeError> {
        let run = self.run_id(key)?;
        let body = serde_json::to_vec(
            &json!({"company_id":self.company_id,"run_id":run,"idempotency_key":key}),
        )
        .map_err(|_| invalid())?;
        let receipt: WireCancellation = self
            .protected_request(Method::POST, "/v1/confidential/runs/cancel", Some(&body))
            .await?
            .ok_or_else(unavailable)?;
        if receipt.runtime_run_ref != Some(self.reference(run)) {
            return Err(invalid());
        }
        Ok(receipt.outcome)
    }

    /// Replay protected bytes only; current authority precedes later decryption/use.
    pub async fn confidential_events(
        &self,
        expected: &ValidatedIdentity,
        after: u32,
    ) -> Result<ConfidentialEventBatch, RuntimeError> {
        if after > 512 {
            return Err(invalid());
        }
        let selected: SelectedIdentity =
            serde_json::from_slice(expected.canonical_bytes()).map_err(|_| invalid())?;
        if selected.company_id != self.company_id {
            return Err(invalid());
        }
        let path = format!(
            "/v1/confidential/runs/{}/events?after={after}&limit=4",
            self.reference(selected.run_id).0
        );
        let wire: WireBatch = self
            .protected_request(Method::GET, &path, None)
            .await?
            .ok_or_else(unavailable)?;
        validate_batch(expected, after, wire)
    }
}

fn validate_batch(
    expected: &ValidatedIdentity,
    after: u32,
    wire: WireBatch,
) -> Result<ConfidentialEventBatch, RuntimeError> {
    if wire.events.len() > 4 {
        return Err(invalid());
    }
    let terminal_state = matches!(
        wire.status,
        ConfidentialRunStatus::Completed
            | ConfidentialRunStatus::Cancelled
            | ConfidentialRunStatus::Failed
    );
    if wire.terminal && !terminal_state {
        return Err(invalid());
    }
    if matches!(
        wire.status,
        ConfidentialRunStatus::Failed | ConfidentialRunStatus::Cancelled
    ) != wire.failure.is_some()
    {
        return Err(invalid());
    }
    if let Some(failure) = &wire.failure {
        if !matches!(
            failure.code.as_str(),
            "executor_interrupted"
                | "executor_unavailable"
                | "policy_denied"
                | "provider_failed"
                | "deadline_exceeded"
                | "provider_incomplete"
                | "provider_response_invalid"
                | "invalid_output"
                | "credential_denied"
                | "runtime_selection_changed"
                | "unsupported_hermes_tool_selection"
                | "cancelled"
        ) || (wire.status == ConfidentialRunStatus::Cancelled) != (failure.code == "cancelled")
        {
            return Err(invalid());
        }
    }
    let mut events = Vec::with_capacity(wire.events.len());
    for (index, event) in wire.events.into_iter().enumerate() {
        let ordinal = after + index as u32 + 1;
        if ordinal > 512 || event.cursor != ordinal.to_string() {
            return Err(invalid());
        }
        let envelope =
            ConfidentialEnvelope::parse(event.envelope.get().as_bytes()).map_err(|_| invalid())?;
        envelope
            .header()
            .require_expected(expected, PayloadPurpose::RuntimeEvent, ordinal)
            .map_err(|_| invalid())?;
        events.push(ConfidentialEvent {
            ordinal,
            occurred_at: event.occurred_at,
            envelope,
        });
    }
    Ok(ConfidentialEventBatch {
        events,
        status: wire.status,
        failure: wire.failure,
        terminal: wire.terminal,
    })
}

#[cfg(test)]
mod tests;
