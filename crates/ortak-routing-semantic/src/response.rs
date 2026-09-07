use std::collections::BTreeSet;

use ortak_control::semantic::SemanticScoringInput;
use ortak_domain::{EmployeeId, EvidenceLabel, SemanticScore};
use serde::Deserialize;
use serde_json::Value;

use crate::request::EVIDENCE;

#[derive(Clone)]
pub(crate) struct Parsed {
    pub scores: Vec<SemanticScore>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub response_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    scores: Vec<Score>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Score {
    employee_id: EmployeeId,
    score: f64,
    evidence: String,
}

// Typed fields reject duplicate control keys before interpreting the envelope.
// Additional provider metadata is ignored and never copied to the decision.
#[derive(Deserialize)]
struct Envelope {
    model: String,
    choices: Vec<Choice>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Choice {
    index: u64,
    finish_reason: String,
    message: Message,
}
#[derive(Deserialize)]
struct Message {
    role: String,
    content: String,
    refusal: Option<Value>,
    tool_calls: Option<Value>,
    function_call: Option<Value>,
}
#[derive(Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

pub(crate) fn parse(
    bytes: &[u8],
    input: &SemanticScoringInput,
    model: &str,
) -> Result<Parsed, &'static str> {
    let value: Envelope = serde_json::from_slice(bytes).map_err(|_| "invalid_response")?;
    if value.model != model || value.choices.len() != 1 {
        return Err("model_or_choice_mismatch");
    }
    let choice = &value.choices[0];
    let message = &choice.message;
    if choice.finish_reason != "stop"
        || choice.index != 0
        || message.role != "assistant"
        || message.refusal.is_some()
        || message.tool_calls.is_some()
        || message.function_call.is_some()
    {
        return Err("refused_or_incomplete_response");
    }
    let output: Output = serde_json::from_str(&message.content).map_err(|_| "invalid_scores")?;
    let scores = validate_scores(output.scores, input)?;
    let usage = value.usage;
    Ok(Parsed {
        scores,
        prompt_tokens: bounded_tokens(usage.as_ref().and_then(|u| u.prompt_tokens))?,
        completion_tokens: bounded_tokens(usage.as_ref().and_then(|u| u.completion_tokens))?,
        total_tokens: bounded_tokens(usage.as_ref().and_then(|u| u.total_tokens))?,
        response_bytes: bytes.len(),
    })
}

pub(crate) fn bounded_tokens(value: Option<u64>) -> Result<Option<u64>, &'static str> {
    if value.is_some_and(|n| n > 1_000_000) {
        return Err("invalid_usage");
    }
    Ok(value)
}

pub(crate) fn validate_scores(
    output: Vec<Score>,
    input: &SemanticScoringInput,
) -> Result<Vec<SemanticScore>, &'static str> {
    let expected: BTreeSet<_> = input
        .request()
        .candidates()
        .iter()
        .map(|c| c.employee_id())
        .collect();
    if output.len() != expected.len() {
        return Err("candidate_coverage");
    }
    let mut seen = BTreeSet::new();
    let mut scores = Vec::with_capacity(output.len());
    for item in output {
        if !expected.contains(&item.employee_id)
            || !seen.insert(item.employee_id.clone())
            || !item.score.is_finite()
            || !(0.0..=1.0).contains(&item.score)
            || !EVIDENCE.contains(&item.evidence.as_str())
        {
            return Err("invalid_scores");
        }
        scores.push(SemanticScore {
            employee_id: item.employee_id,
            score: item.score as f32,
            evidence: vec![EvidenceLabel::parse(item.evidence).map_err(|_| "invalid_scores")?],
        });
    }
    scores.sort_by(|a, b| a.employee_id.cmp(&b.employee_id));
    Ok(scores)
}
