#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Bounded semantic evidence for the central router, explicitly configured only.
//!
//! This adapter has no dispatch, database, memory, tool or activation capability.
//! The control layer owns deadlines, current authority and durable silent outcomes.

mod config;
mod hermes;
mod request;
mod response;
mod state;

pub use config::{SemanticConfig, SemanticToken};
pub use hermes::{HermesCodexConfig, HermesCodexScorer};

use ortak_control::{
    ports::{ScoringOutcome, SemanticScorer},
    routing::ScorerMetadata,
    run_event::RedactionPolicy,
    semantic::SemanticScoringInput,
    CompanyScope, ScoringBudget,
};
use ortak_router::SemanticScoringFailure;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client, StatusCode,
};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Fixed adapter identity persisted by the central routing transaction.
pub const ADAPTER: &str = "chat-completions";
/// Reviewed scoring implementation version.
pub const SCORER_VERSION: &str = "ortak-semantic-v1";
/// Compiled instruction version; request text cannot override it.
pub const PROMPT_VERSION: &str = "relevance-v1";
/// Exact response contract version.
pub const SCHEMA_VERSION: &str = "scores-v1";
/// Redaction policy revision included in local cache identity.
pub const REDACTOR_VERSION: &str = "ortak-patterns-v1";

/// Company-bound scorer with finite concurrency, cache and provider failure state.
/// It intentionally has no Debug or serialization implementation.
pub struct ChatCompletionsScorer {
    company: Uuid,
    config: SemanticConfig,
    endpoint: Url,
    client: Client,
    authorization: HeaderValue,
    redaction: RedactionPolicy,
    state: state::State,
    operations: Semaphore,
}

impl ChatCompletionsScorer {
    /// Binds an explicitly resolved company, deployment, exact model and token.
    /// This performs no request, model discovery or implicit credential lookup.
    pub fn new(
        scope: &CompanyScope,
        config: SemanticConfig,
        token: SemanticToken,
    ) -> Result<Self, &'static str> {
        let origin = config::validate(&config)?;
        if scope.company_id().is_nil()
            || config.token_ref != token.reference
            || token.secret.len() < 8
            || token.secret.len() > 16_384
            || token.secret.bytes().any(|b| !b.is_ascii_graphic())
        {
            return Err("invalid semantic company or authentication selection");
        }
        let secret = Zeroizing::new(format!("Bearer {}", token.secret.as_str()));
        let mut authorization =
            HeaderValue::from_str(&secret).map_err(|_| "invalid semantic authentication")?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(|_| "semantic HTTP client unavailable")?;
        let endpoint = origin
            .join("/v1/chat/completions")
            .map_err(|_| "invalid semantic operation")?;
        // A accidentally pasted selected provider token is scrubbed as well as
        // the common secret forms. The token never enters config/cache metadata.
        let redaction = RedactionPolicy::new().with_literal_secrets([token.secret.as_str()]);
        Ok(Self {
            company: scope.company_id(),
            config,
            endpoint,
            client,
            authorization,
            redaction,
            state: state::State::default(),
            operations: Semaphore::new(2),
        })
    }

    fn outcome(
        &self,
        result: Result<response::Parsed, &'static str>,
        started: Instant,
        request: Option<&request::Request>,
        cached: bool,
    ) -> ScoringOutcome {
        let mut metadata = self.metadata();
        metadata.latency_ms = Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32);
        let (result, parsed, code) = match result {
            Ok(parsed) => (Ok(parsed.scores.clone()), Some(parsed), None),
            Err(code) => (
                Err(if code == "request_timeout" {
                    SemanticScoringFailure::TimedOut
                } else {
                    SemanticScoringFailure::Unavailable
                }),
                None,
                Some(code),
            ),
        };
        metadata.usage = Some(json!({"cache_hit":cached,"failure_code":code,
            "request_bytes":request.map(|r|r.bytes.len()),"redacted":request.map(|r|r.redacted),
            "response_bytes":if cached {Some(0)} else {parsed.as_ref().map(|p|p.response_bytes)},
            "prompt_tokens":if cached {Some(0)} else {parsed.as_ref().and_then(|p|p.prompt_tokens)},
            "completion_tokens":if cached {Some(0)} else {parsed.as_ref().and_then(|p|p.completion_tokens)},
            "total_tokens":if cached {Some(0)} else {parsed.as_ref().and_then(|p|p.total_tokens)},
            "schema_version":SCHEMA_VERSION,"redactor_version":REDACTOR_VERSION,
            "deployment_id":self.config.deployment_id,"response_model":self.config.response_model}));
        ScoringOutcome { result, metadata }
    }

    async fn remote(
        &self,
        request: &request::Request,
        input: &SemanticScoringInput,
    ) -> Result<response::Parsed, &'static str> {
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(request.bytes.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "request_timeout"
                } else {
                    "transport_failed"
                }
            })?;
        if response.status() != StatusCode::OK {
            return Err("provider_rejected");
        }
        if response
            .content_length()
            .is_some_and(|n| n > request::MAX_WIRE as u64)
        {
            return Err("response_bounds");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "response_interrupted")? {
            if bytes.len().saturating_add(chunk.len()) > request::MAX_WIRE {
                return Err("response_bounds");
            }
            bytes.extend_from_slice(&chunk);
        }
        response::parse(&bytes, input, &self.config.response_model)
    }
}

impl SemanticScorer for ChatCompletionsScorer {
    fn metadata(&self) -> ScorerMetadata {
        ScorerMetadata {
            adapter: ADAPTER.to_owned(),
            model: Some(self.config.model.clone()),
            prompt_version: Some(PROMPT_VERSION.to_owned()),
            version: SCORER_VERSION.to_owned(),
            latency_ms: None,
            usage: None,
        }
    }

    async fn score(&self, input: &SemanticScoringInput, budget: ScoringBudget) -> ScoringOutcome {
        let started = Instant::now();
        if budget.remaining().is_zero() {
            return self.outcome(Err("request_timeout"), started, None, false);
        }
        if input.company_id() != self.company {
            return self.outcome(Err("company_mismatch"), started, None, false);
        }
        let request = match request::build(input, &self.config, &self.redaction) {
            Ok(request) => request,
            Err(code) => return self.outcome(Err(code), started, None, false),
        };
        if let Some(cached) = self.state.cached(&request.key) {
            return self.outcome(Ok(cached), started, Some(&request), true);
        }
        // No queued futures or hidden request retries; saturation records silence.
        let Ok(_permit) = self.operations.try_acquire() else {
            return self.outcome(Err("scorer_busy"), started, Some(&request), false);
        };
        let attempt = match self.state.attempt() {
            Ok(attempt) => attempt,
            Err(code) => return self.outcome(Err(code), started, Some(&request), false),
        };
        let result = tokio::time::timeout_at(budget.deadline(), self.remote(&request, input))
            .await
            .unwrap_or(Err("request_timeout"));
        let result = if budget.remaining().is_zero() {
            Err("request_timeout")
        } else {
            result
        };
        attempt.finish(result.is_ok());
        if let Ok(parsed) = &result {
            self.state.insert(request.key, parsed.clone());
        }
        self.outcome(result, started, Some(&request), false)
    }
}

#[cfg(test)]
mod tests;
