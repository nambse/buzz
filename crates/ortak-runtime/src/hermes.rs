//! Authenticated transport to the isolated durable Hermes execution bridge.

use std::{fmt, time::Duration};

use chrono::{DateTime, Utc};
use ortak_control::adapter::{Detail, HealthReport, ResourceOutcome};
use ortak_control::run_event::RunEventPayload;
use ortak_control::runtime::{
    CancelOutcome, CancelStartReceipt, RunSpec, RunStartReceipt, RuntimeAdapter,
    RuntimeCapabilities, RuntimeCapability, RuntimeCursor, RuntimeError, RuntimeEvent,
    RuntimeEventBatch, RuntimeResourceRequest, RuntimeRunRef,
};
use ortak_domain::{ProvisioningMode, RuntimeBinding};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Method, StatusCode,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

/// A bridge connection fixed to one company and one configured origin.
#[derive(Clone)]
pub struct HermesAdapter {
    client: reqwest::Client,
    origin: Url,
    company_id: Uuid,
}

impl fmt::Debug for HermesAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesAdapter")
            .field("company_id", &self.company_id)
            .finish_non_exhaustive()
    }
}

fn unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        detail: Detail::new("Hermes bridge request failed"),
    }
}
fn invalid() -> RuntimeError {
    RuntimeError::InvalidSpec {
        detail: Detail::new("invalid Hermes bridge request or response"),
    }
}

impl HermesAdapter {
    /// Creates a bounded client. Redirects, proxy environment variables, URL
    /// credentials and non-loopback plaintext endpoints are refused.
    pub fn new(company_id: Uuid, origin: &str, bearer_token: &str) -> Result<Self, RuntimeError> {
        let origin = Url::parse(origin).map_err(|_| invalid())?;
        let loopback = origin.host_str().is_some_and(|h| {
            h == "localhost"
                || h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || !(origin.scheme() == "https" || (origin.scheme() == "http" && loopback))
            || bearer_token.is_empty()
            || bearer_token.len() > 4096
        {
            return Err(invalid());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {bearer_token}")).map_err(|_| invalid())?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            client,
            origin,
            company_id,
        })
    }

    fn run_id(&self, key: &str) -> Result<Uuid, RuntimeError> {
        let prefix = format!("ortak-run:{}:", self.company_id);
        key.strip_prefix(&prefix)
            .and_then(|id| Uuid::parse_str(id).ok())
            .filter(|id| crate::run_idempotency_key(self.company_id, *id) == key)
            .ok_or_else(invalid)
    }

    fn reference(&self, run_id: Uuid) -> RuntimeRunRef {
        RuntimeRunRef(format!("ortak:{}:{run_id}", self.company_id))
    }

    fn reference_run(&self, reference: &RuntimeRunRef) -> Result<Uuid, RuntimeError> {
        reference
            .0
            .strip_prefix(&format!("ortak:{}:", self.company_id))
            .and_then(|id| Uuid::parse_str(id).ok())
            .filter(|id| self.reference(*id) == *reference)
            .ok_or_else(invalid)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<T>, RuntimeError> {
        let url = self.origin.join(path).map_err(|_| invalid())?;
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body).map_err(|_| invalid())?;
            if bytes.len() > 256 * 1024 {
                return Err(invalid());
            }
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes);
        }
        let mut response = request.send().await.map_err(|_| unavailable())?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(unavailable());
        }
        const MAX_RESPONSE: usize = 4 * 1024 * 1024;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| unavailable())
    }

    fn receipt(&self, run_id: Uuid, receipt: WireReceipt) -> Result<RunStartReceipt, RuntimeError> {
        if receipt.runtime_run_ref != self.reference(run_id) {
            return Err(invalid());
        }
        Ok(RunStartReceipt {
            runtime_run_ref: receipt.runtime_run_ref,
            started_at: receipt.started_at,
        })
    }
}

#[derive(Deserialize)]
struct WireReceipt {
    runtime_run_ref: RuntimeRunRef,
    started_at: DateTime<Utc>,
}
#[derive(Deserialize)]
struct WireCancellation {
    runtime_run_ref: Option<RuntimeRunRef>,
    outcome: CancelOutcome,
}
#[derive(Deserialize)]
struct WireEvents {
    events: Vec<WireEvent>,
    terminal: bool,
}
#[derive(Deserialize)]
struct WireEvent {
    cursor: String,
    occurred_at: DateTime<Utc>,
    payload: RunEventPayload,
}
#[derive(Deserialize)]
struct WireProfile {
    profile_ref: String,
    healthy: bool,
}

impl RuntimeAdapter for HermesAdapter {
    fn adapter_name(&self) -> &str {
        "hermes"
    }

    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError> {
        let capabilities: RuntimeCapabilities = self
            .request(Method::GET, "/v1/capabilities", None)
            .await?
            .ok_or_else(unavailable)?;
        if capabilities.adapter != self.adapter_name()
            || capabilities.api_version != "ortak-hermes-bridge/v1"
        {
            return Err(invalid());
        }
        Ok(capabilities)
    }

    async fn health(&self, binding: &RuntimeBinding) -> Result<HealthReport, RuntimeError> {
        let profile: WireProfile = self
            .request(
                Method::POST,
                "/v1/profiles/inspect",
                Some(json!({"company_id": self.company_id, "binding": binding})),
            )
            .await?
            .ok_or_else(unavailable)?;
        if Some(&profile.profile_ref) != binding.profile_ref.as_ref() {
            return Err(invalid());
        }
        Ok(if profile.healthy {
            HealthReport::healthy("isolated profile is available")
        } else {
            HealthReport::unhealthy("isolated profile is unavailable")
        })
    }

    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError> {
        if request.mode != ProvisioningMode::Adopt {
            return Err(RuntimeError::Unsupported {
                capability: RuntimeCapability::ProfileCreate,
            });
        }
        if !self.health(&request.binding).await?.is_healthy() {
            return Err(unavailable());
        }
        request
            .binding
            .profile_ref
            .as_ref()
            .map(ResourceOutcome::adopted)
            .ok_or_else(invalid)
    }

    async fn delete_created_profile(
        &self,
        _resource_ref: &str,
        _idempotency_key: &str,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::Unsupported {
            capability: RuntimeCapability::ProfileDelete,
        })
    }

    async fn start_run(&self, spec: &RunSpec) -> Result<RunStartReceipt, RuntimeError> {
        spec.validate()?;
        let run_id = self.run_id(&spec.idempotency_key)?;
        if run_id != spec.run_id || spec.binding.adapter != self.adapter_name() {
            return Err(invalid());
        }
        let receipt = self
            .request(
                Method::POST,
                "/v1/runs",
                Some(json!({"company_id": self.company_id, "spec": spec})),
            )
            .await?
            .ok_or_else(unavailable)?;
        self.receipt(run_id, receipt)
    }

    async fn lookup_start(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<RunStartReceipt>, RuntimeError> {
        let run_id = self.run_id(idempotency_key)?;
        self.request(Method::POST, "/v1/runs/lookup", Some(json!({"company_id": self.company_id,"run_id": run_id,"idempotency_key": idempotency_key})))
            .await?.map(|receipt| self.receipt(run_id, receipt)).transpose()
    }

    async fn cancel_start(
        &self,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<CancelStartReceipt, RuntimeError> {
        let run_id = self.run_id(idempotency_key)?;
        if reason.len() > 2048 {
            return Err(invalid());
        }
        let receipt: WireCancellation = self.request(Method::POST, "/v1/runs/cancel", Some(json!({"company_id": self.company_id,"run_id": run_id,"idempotency_key": idempotency_key,"reason": reason}))).await?.ok_or_else(unavailable)?;
        if receipt
            .runtime_run_ref
            .as_ref()
            .is_some_and(|r| *r != self.reference(run_id))
        {
            return Err(invalid());
        }
        Ok(CancelStartReceipt {
            runtime_run_ref: receipt.runtime_run_ref,
            outcome: receipt.outcome,
        })
    }

    async fn next_events(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        after: Option<&RuntimeCursor>,
        limit: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError> {
        self.reference_run(runtime_run_ref)?;
        let after = after
            .map(|cursor| cursor.0.parse::<u64>().map_err(|_| invalid()))
            .transpose()?;
        let limit = limit.clamp(1, 100);
        let mut path = format!("/v1/runs/{}/events?limit={limit}", runtime_run_ref.0);
        if let Some(after) = after {
            path.push_str(&format!("&after={after}"));
        }
        let batch: WireEvents = self
            .request(Method::GET, &path, None)
            .await?
            .ok_or_else(|| RuntimeError::UnknownRun {
                runtime_run_ref: runtime_run_ref.clone(),
            })?;
        if batch.events.len() > limit {
            return Err(invalid());
        }
        let mut previous = after;
        let mut events = Vec::with_capacity(batch.events.len());
        for event in batch.events {
            let cursor = event.cursor.parse::<u64>().map_err(|_| invalid())?;
            if previous.map_or(cursor > 1, |previous| {
                previous.checked_add(1) != Some(cursor)
            }) {
                return Err(invalid());
            }
            previous = Some(cursor);
            events.push(RuntimeEvent {
                cursor: RuntimeCursor(event.cursor),
                occurred_at: event.occurred_at,
                payload: event.payload,
            });
        }
        Ok(RuntimeEventBatch {
            events,
            terminal: batch.terminal,
        })
    }

    async fn cancel_run(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        reason: &str,
    ) -> Result<CancelOutcome, RuntimeError> {
        let run_id = self.reference_run(runtime_run_ref)?;
        self.cancel_start(&crate::run_idempotency_key(self.company_id, run_id), reason)
            .await
            .map(|receipt| receipt.outcome)
    }
}
