//! One score-only private Codex transport; no employee runtime or delivery capability.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const ADAPTER: &str = "hermes-codex";
const VERSION: &str = "ortak-hermes-semantic-v1";

/// Explicit private deployment and exact reviewed OAuth profile variant.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesCodexConfig {
    /// Central scoring deployment identity, independent of candidate identities.
    pub deployment_id: Uuid,
    /// Fixed private HTTPS or literal-loopback origin; path is adapter-owned.
    pub origin: String,
    /// Exact selected model.
    pub model: String,
    /// Exact provider response model required by this deployment.
    pub response_model: String,
    /// Exact reviewed reasoning effort; no implicit downgrade.
    pub reasoning_effort: String,
    /// SHA-256 of the complete server-owned public profile binding.
    pub binding_sha256: String,
    /// Opaque credential reference for the private listener, never a provider token.
    pub bridge_token_ref: ortak_domain::CredentialRef,
}

impl HermesCodexConfig {
    fn transport(&self) -> SemanticConfig {
        SemanticConfig {
            deployment_id: self.deployment_id,
            origin: self.origin.clone(),
            model: self.model.clone(),
            response_model: self.response_model.clone(),
            token_ref: self.bridge_token_ref.clone(),
        }
    }

    /// Validates all public selections without credential resolution or network I/O.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.transport().validate()?;
        if self.binding_sha256.len() != 64
            || !self
                .binding_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !matches!(
                self.reasoning_effort.as_str(),
                "low" | "medium" | "high" | "xhigh" | "max"
            )
            || (self.reasoning_effort == "max"
                && self.model != "gpt-6-astra"
                && !self.model.starts_with("gpt-5.6"))
        {
            return Err("invalid semantic profile variant");
        }
        Ok(())
    }
}

/// Central company scorer with the shared redaction, cache, circuit and finite slots.
pub struct HermesCodexScorer {
    inner: ChatCompletionsScorer,
    selection: HermesCodexConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    deployment_id: Uuid,
    binding_sha256: String,
    model: String,
    reasoning_effort: String,
    prompt_version: String,
    schema_version: String,
    scores: Vec<response::Score>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

impl HermesCodexScorer {
    /// Constructs one exact selected listener. OAuth secrets remain in its owned store.
    pub fn new(
        scope: &CompanyScope,
        selection: HermesCodexConfig,
        token: SemanticToken,
    ) -> Result<Self, &'static str> {
        selection.validate()?;
        let mut inner = ChatCompletionsScorer::new(scope, selection.transport(), token)?;
        inner.endpoint = config::validate(&inner.config)?
            .join("/v1/semantic/score")
            .map_err(|_| "invalid semantic operation")?;
        Ok(Self { inner, selection })
    }

    fn outcome(
        &self,
        result: Result<response::Parsed, &'static str>,
        started: Instant,
        request: Option<&request::Request>,
        cached: bool,
    ) -> ScoringOutcome {
        let mut outcome = self.inner.outcome(result, started, request, cached);
        outcome.metadata.adapter = ADAPTER.to_owned();
        outcome.metadata.version = VERSION.to_owned();
        if let Some(usage) = outcome.metadata.usage.as_mut() {
            usage["reasoning_effort"] = json!(self.selection.reasoning_effort);
            usage["binding_sha256"] = json!(self.selection.binding_sha256);
            // Shared preparation also builds the legacy envelope. Its byte
            // count is not the private transport's wire length.
            usage["request_bytes"] = serde_json::Value::Null;
            usage["redacted_input_bytes"] = json!(request.map(|r| r.data.to_string().len()));
        }
        outcome
    }

    fn parse(
        &self,
        bytes: &[u8],
        input: &SemanticScoringInput,
    ) -> Result<response::Parsed, &'static str> {
        let value: Envelope = serde_json::from_slice(bytes).map_err(|_| "invalid_response")?;
        if value.deployment_id != self.selection.deployment_id
            || value.binding_sha256 != self.selection.binding_sha256
            || value.model != self.selection.response_model
            || value.reasoning_effort != self.selection.reasoning_effort
            || value.prompt_version != PROMPT_VERSION
            || value.schema_version != SCHEMA_VERSION
        {
            return Err("semantic_selection_changed");
        }
        let usage = value.usage;
        Ok(response::Parsed {
            scores: response::validate_scores(value.scores, input)?,
            prompt_tokens: response::bounded_tokens(usage.as_ref().and_then(|u| u.input_tokens))?,
            completion_tokens: response::bounded_tokens(
                usage.as_ref().and_then(|u| u.output_tokens),
            )?,
            total_tokens: response::bounded_tokens(usage.as_ref().and_then(|u| u.total_tokens))?,
            response_bytes: bytes.len(),
        })
    }

    async fn remote(
        &self,
        request: &request::Request,
        input: &SemanticScoringInput,
        budget: ScoringBudget,
    ) -> Result<response::Parsed, &'static str> {
        let remaining = budget
            .remaining()
            .saturating_sub(Duration::from_millis(100))
            .min(Duration::from_millis(4500));
        let budget_ms = remaining.as_millis();
        if budget_ms == 0 {
            return Err("request_timeout");
        }
        let body = serde_json::to_vec(&json!({
            "deployment_id":self.selection.deployment_id,
            "binding_sha256":self.selection.binding_sha256,
            "request_id":Uuid::new_v4(),
            "prompt_version":PROMPT_VERSION,"schema_version":SCHEMA_VERSION,
            "budget_ms":budget_ms,"input":request.data
        }))
        .map_err(|_| "input_encoding")?;
        if body.len() > request::MAX_WIRE {
            return Err("input_bounds");
        }
        let mut response = self
            .inner
            .client
            .post(self.inner.endpoint.clone())
            .header(AUTHORIZATION, self.inner.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "request_timeout"
                } else {
                    "transport_failed"
                }
            })?;
        if response.status() == StatusCode::REQUEST_TIMEOUT {
            return Err("request_timeout");
        }
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
        self.parse(&bytes, input)
    }
}

impl SemanticScorer for HermesCodexScorer {
    fn metadata(&self) -> ScorerMetadata {
        let mut metadata = self.inner.metadata();
        metadata.adapter = ADAPTER.to_owned();
        metadata.version = VERSION.to_owned();
        metadata
    }

    async fn score(&self, input: &SemanticScoringInput, budget: ScoringBudget) -> ScoringOutcome {
        let started = Instant::now();
        if budget.remaining().is_zero() {
            return self.outcome(Err("request_timeout"), started, None, false);
        }
        if input.company_id() != self.inner.company {
            return self.outcome(Err("company_mismatch"), started, None, false);
        }
        let request = match request::build(input, &self.inner.config, &self.inner.redaction) {
            Ok(mut request) => {
                let identity = json!([
                    request.key.to_vec(),
                    ADAPTER,
                    VERSION,
                    self.selection.binding_sha256,
                    self.selection.reasoning_effort
                ]);
                request.key = Sha256::digest(identity.to_string().as_bytes()).into();
                request
            }
            Err(code) => return self.outcome(Err(code), started, None, false),
        };
        if let Some(cached) = self.inner.state.cached(&request.key) {
            return self.outcome(Ok(cached), started, Some(&request), true);
        }
        let Ok(_permit) = self.inner.operations.try_acquire() else {
            return self.outcome(Err("scorer_busy"), started, Some(&request), false);
        };
        let attempt = match self.inner.state.attempt() {
            Ok(attempt) => attempt,
            Err(code) => return self.outcome(Err(code), started, Some(&request), false),
        };
        let result =
            tokio::time::timeout_at(budget.deadline(), self.remote(&request, input, budget))
                .await
                .unwrap_or(Err("request_timeout"));
        let result = if budget.remaining().is_zero() {
            Err("request_timeout")
        } else {
            result
        };
        attempt.finish(result.is_ok());
        if let Ok(parsed) = &result {
            self.inner.state.insert(request.key, parsed.clone());
        }
        self.outcome(result, started, Some(&request), false)
    }
}
