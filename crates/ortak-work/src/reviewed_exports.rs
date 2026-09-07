//! Durable reviewed-fact publication and narrowly scoped remote text cleanup.
//! Adapter I/O belongs to the server worker, outside these transactions.
mod jobs;
mod targets;
mod types;
pub use jobs::{acknowledge, claim, fail, prepare};
pub use targets::{advertise_targets, advertise_targets_with_conversations};
pub use types::*;

use crate::{Result, WorkError};
use chrono::{DateTime, Utc};
use ortak_control::{CompanyScope, PgControlPlane};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use std::time::Duration;
use uuid::Uuid;

pub(crate) fn invalid() -> WorkError {
    WorkError::InvalidQuery("reviewed memory export rejected")
}
pub(crate) fn hash(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > 32768 {
        return Err(invalid());
    }
    Ok(Sha256::digest(bytes).to_vec())
}
pub(crate) fn expiry(value: DateTime<Utc>) -> String {
    use chrono::{SecondsFormat, Timelike};
    let micros = value.nanosecond() / 1000 * 1000;
    value
        .with_nanosecond(micros)
        .unwrap_or(value)
        .to_rfc3339_opts(
            if micros == 0 {
                SecondsFormat::Secs
            } else {
                SecondsFormat::Micros
            },
            true,
        )
}
pub(crate) fn operation_key(fact: Uuid, action: ReviewedExportAction) -> String {
    format!("reviewed:{}:{fact}", action.as_str())
}
// These are the canonical wire fingerprint fields shared by enqueue and replay.
#[allow(clippy::too_many_arguments)]
pub(crate) fn request_hash(
    company: Uuid,
    project: Uuid,
    fact: Uuid,
    employee: &ortak_domain::EmployeeId,
    binding: &ortak_domain::MemoryBinding,
    action: ReviewedExportAction,
    content: &str,
    source_hash: &[u8],
    approved_by: &str,
    approval_id: Uuid,
    expires: DateTime<Utc>,
) -> Result<Vec<u8>> {
    let mut body = json!({"company_id":company,"employee_id":employee,
        "idempotency_key":operation_key(fact,action)});
    if action == ReviewedExportAction::Publish {
        let object = body.as_object_mut().ok_or_else(invalid)?;
        object.extend(json!({"content":content,"content_hash":hex::encode(Sha256::digest(content.as_bytes())),
            "source_hash":hex::encode(source_hash),"approved_by":approved_by,"approval_id":approval_id,
            "expires_at":expiry(expires)}).as_object().ok_or_else(invalid)?.clone());
    }
    let object = body.as_object_mut().ok_or_else(invalid)?;
    object.extend(
        json!({"family":"reviewed-project/1","workspace_id":binding.workspace,
        "project_id":project,"record_id":fact,"action":action.as_str()})
        .as_object()
        .ok_or_else(invalid)?
        .clone(),
    );
    hash(&body)
}
async fn bounds(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(c).await?;
    Ok(())
}
async fn bounded<T>(future: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| WorkError::OperationTimedOut)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_request_hash_matches_the_reviewed_wire_family_and_microsecond_expiry() {
        // Independent Python json.dumps(sort_keys=True,separators=(',',':'),ensure_ascii=False)
        // vectors, including non-ASCII text and truncated nanoseconds.
        let company = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let project = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let fact = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
        let approval = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
        let employee = ortak_domain::EmployeeId::parse("ada").unwrap();
        let binding: ortak_domain::MemoryBinding = serde_json::from_value(
            json!({"adapter":"honcho","endpoint_ref":"service://fixture",
            "workspace":"reviewed-fixture","user_peer":"human","employee_peer":"ada","options":{}}),
        )
        .unwrap();
        let expires = DateTime::parse_from_rfc3339("2026-09-06T12:34:56.123456789Z")
            .unwrap()
            .with_timezone(&Utc);
        for (action, expected) in [
            (
                ReviewedExportAction::Publish,
                "badf7b4ec8fdc82149bb944a011a1a55a8a0132833033ba40e5b21ca5839dfb7",
            ),
            (
                ReviewedExportAction::Withdraw,
                "d67c5208a1a53a87632f559974e7408cc1122da2ea9f358629e2459befd6e8cd",
            ),
        ] {
            assert_eq!(
                hex::encode(
                    request_hash(
                        company,
                        project,
                        fact,
                        &employee,
                        &binding,
                        action,
                        "Reviewed café fact",
                        &[0xab; 32],
                        &"cd".repeat(32),
                        approval,
                        expires
                    )
                    .unwrap()
                ),
                expected
            );
        }
    }
}
