use super::*;
use ortak_domain::{RecipientAction, RoutingMode, RoutingReason};
use sqlx::postgres::PgRow;

fn token(row: &PgRow, field: &str) -> Result<Option<String>> {
    let value: Option<String> = row.try_get(field)?;
    Ok(value.filter(|v| {
        !v.is_empty()
            && v.len() <= 128
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    }))
}
fn value(row: &PgRow, field: &str) -> Result<Value> {
    Ok(row
        .try_get::<Option<Value>, _>(field)?
        .unwrap_or(Value::Null))
}
fn number(row: &PgRow, field: &str) -> Result<Option<u64>> {
    let text: Option<String> = row.try_get(field)?;
    Ok(text
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|v| *v <= 1_000_000))
}
fn kind<T: serde::de::DeserializeOwned>(row: &PgRow, field: &str) -> Result<T> {
    serde_json::from_value(json!(row.try_get::<String, _>(field)?))
        .map_err(|_| ApiError::unavailable())
}

pub(super) fn decision(row: &PgRow, rows: &[PgRow]) -> Result<Value> {
    let mode: RoutingMode = kind(row, "mode")?;
    let reason: RoutingReason = kind(row, "summary_reason")?;
    let mut recipients = Vec::new();
    for row in rows.iter().take(32) {
        let action: RecipientAction = kind(row, "action")?;
        let reason: RoutingReason = kind(row, "reason")?;
        let score: Option<f32> = row.try_get("score")?;
        let evidence = value(row, "evidence")?;
        // The provider's closed evidence vocabulary, never free-form text or
        // historical adapter labels that could carry private prose.
        let labels = evidence
            .as_array()
            .into_iter()
            .flatten()
            .take(8)
            .filter_map(Value::as_str)
            .filter(|label| {
                matches!(
                    *label,
                    "responsibility_match"
                        | "domain_match"
                        | "role_match"
                        | "insufficient_context"
                        | "no_match"
                )
            })
            .collect::<Vec<_>>();
        recipients.push(
            json!({"employee_id":row.try_get::<String,_>("employee_id")?,
            "action":action,"reason":reason,
            "score":score.filter(|s|s.is_finite() && (0.0..=1.0).contains(s)),"evidence":labels}),
        );
    }
    let effort = value(row, "effort")?;
    let effort = effort
        .as_str()
        .filter(|v| matches!(*v, "low" | "medium" | "high" | "xhigh" | "max"));
    let failure = value(row, "failure_code")?;
    let failure = failure.as_str().filter(|v| {
        matches!(
            *v,
            "request_timeout"
                | "provider_rejected"
                | "transport_failed"
                | "response_interrupted"
                | "response_bounds"
                | "invalid_response"
                | "semantic_selection_changed"
                | "invalid_scores"
                | "candidate_coverage"
                | "invalid_usage"
                | "scorer_busy"
                | "circuit_open"
                | "refused_or_incomplete_response"
                | "model_or_choice_mismatch"
                | "input_bounds"
        )
    });
    Ok(
        json!({"decision_id":row.try_get::<Uuid,_>("id")?,"mode":mode,"summary_reason":reason,
        "policy_version":token(row,"policy_version")?,
        "decided_at":row.try_get::<chrono::DateTime<chrono::Utc>,_>("decided_at")?,
        "scorer":{"adapter":token(row,"scorer_adapter")?,"model":token(row,"scorer_model")?,
            "prompt_version":token(row,"scorer_prompt_version")?,"version":token(row,"scorer_version")?,
            "latency_ms":row.try_get::<Option<i32>,_>("scorer_latency_ms")?,"reasoning_effort":effort,
            "cache_hit":value(row,"cached")?.as_bool(),"failure_code":failure,
            "input_tokens":number(row,"input_tokens")?,"output_tokens":number(row,"output_tokens")?,
            "total_tokens":number(row,"total_tokens")?},
        "recipients":recipients,"recipients_truncated":rows.len()>32}),
    )
}
