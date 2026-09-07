//! One immutable administrative receipt per authenticated operation.
use super::*;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

pub(super) struct Receipt {
    pub project_id: Uuid,
    pub work_item_id: Option<Uuid>,
    pub result_version: i64,
}
pub(super) fn fingerprint(value: impl serde::Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| WorkError::InvalidQuery("invalid operation payload"))?;
    if bytes.len() > 16 * 1024 {
        return Err(WorkError::InvalidQuery("operation exceeds 16 KiB"));
    }
    Ok(Sha256::digest(bytes).to_vec())
}
impl AuthorizedWork {
    pub(super) async fn operation_on(
        &self,
        c: &mut PgConnection,
        id: Uuid,
        action: &str,
        hash: &[u8],
    ) -> Result<Option<Receipt>> {
        if id.is_nil() {
            return Err(WorkError::InvalidQuery("operation_id must not be nil"));
        }
        // Hash collisions serialize unrelated work but never expand access or change identity.
        let key = format!(
            "ortak-work:{}:{}:{}",
            self.scope.company_id(),
            self.principal.public_key,
            id
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(key)
            .execute(&mut *c)
            .await?;
        let row = sqlx::query(
            "SELECT action,request_hash,project_id,work_item_id,result_version FROM work_api_operations
 WHERE company_id=$1 AND actor_pubkey=$2 AND operation_id=$3",
        )
        .bind(self.scope.company_id())
        .bind(&self.principal.public_key)
        .bind(id)
        .fetch_optional(c)
        .await?;
        row.map(|r| {
            if r.try_get::<String, _>("action")? != action
                || r.try_get::<Vec<u8>, _>("request_hash")? != hash
            {
                return Err(WorkError::OperationConflict);
            }
            Ok(Receipt {
                project_id: r.try_get("project_id")?,
                work_item_id: r.try_get("work_item_id")?,
                result_version: r.try_get("result_version")?,
            })
        })
        .transpose()
    }
    pub(super) async fn replay_transition_from(
        &self,
        c: &mut PgConnection,
        receipt: &Receipt,
        id: Uuid,
        expected_version: i64,
        target: WorkState,
        reason: &Option<String>,
    ) -> Result<WorkState> {
        if expected_version.checked_add(1) != Some(receipt.result_version) {
            return Err(invalid("receipt version disagrees with operation"));
        }
        let row=sqlx::query("SELECT payload,actor_type,actor_id FROM work_item_history WHERE company_id=$1 AND work_item_id=$2 AND version=$3 AND sequence=$4")
            .bind(self.scope.company_id()).bind(id).bind(receipt.result_version).bind(receipt.result_version-1)
            .fetch_optional(c).await?.ok_or_else(||invalid("receipt transition history is missing"))?;
        if row.try_get::<String, _>("actor_type")? != "human"
            || row.try_get::<Option<String>, _>("actor_id")?.as_deref()
                != Some(self.principal.public_key.as_str())
        {
            return Err(invalid("receipt actor disagrees with history"));
        }
        let event: WorkEvent = serde_json::from_value(row.try_get("payload")?)
            .map_err(|_| invalid("receipt transition history is unreadable"))?;
        match event {
            WorkEvent::StateChanged {
                from,
                to,
                reason: stored_reason,
            } if to == target && stored_reason == *reason => Ok(from),
            _ => Err(invalid("receipt transition disagrees with operation")),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn record_on(
        &self,
        c: &mut PgConnection,
        id: Uuid,
        action: &str,
        hash: &[u8],
        project: Uuid,
        item: Option<Uuid>,
        version: i64,
        valid_before: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO work_api_operations
 (company_id,actor_pubkey,operation_id,action,request_hash,project_id,work_item_id,result_version,auth_event_id,valid_before)
 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(self.scope.company_id()).bind(&self.principal.public_key).bind(id).bind(action).bind(hash)
            .bind(project).bind(item).bind(version).bind(self.principal.auth_event_id.as_slice()).bind(valid_before)
            .execute(c).await?;
        Ok(())
    }
}
