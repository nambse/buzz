//! Current project/source/employee audience checks for retained execution evidence.
use super::*;
use serde::Serialize;

/// Safe execution metadata for a currently authorized project reader.
#[derive(Clone, Debug, Serialize)]
pub struct WorkExecutionView {
    /// Durable run.
    pub run_id: Uuid,
    /// Assigned employee.
    pub employee_id: EmployeeId,
    /// Version at admission.
    pub execution_version: i64,
    /// Current run state.
    pub status: String,
    /// Saved deliverable, when materialized.
    pub artifact_id: Option<Uuid>,
    /// Closed terminal output code.
    pub output_code: Option<String>,
    /// True after the terminal output job releases this item’s execution slot.
    pub reconciled: bool,
    /// Currently readable frozen sources, or a pending/revoked selection state.
    pub reference_context: Option<WorkExecutionContext>,
}

/// Source metadata only; transcript and artifact text retain separate read gates.
#[derive(Clone, Debug, Serialize)]
pub struct WorkExecutionContext {
    /// `pending`, `ready`, or `unavailable` under current source authority.
    pub status: String,
    /// Exact prior deliverable, only when its author is in this reader's audience.
    pub artifact_id: Option<Uuid>,
    /// Work version of that prior deliverable's producing execution.
    pub artifact_execution_version: Option<i64>,
    /// Canonical selected message ids; unavailable sources disclose none.
    pub message_ids: Vec<String>,
    /// True when any selected message text or older history was omitted.
    pub history_limited: bool,
}

/// Verified bounded text artifact; content is never interpreted as HTML or a path.
pub struct WorkTextArtifact {
    /// Complete UTF-8 bytes, at most 32 KiB.
    pub content: String,
    /// Lowercase SHA-256 digest of these exact bytes.
    pub sha256: String,
}

impl AuthorizedWork {
    /// Most recent twenty visible executions, filtered before the finite limit.
    pub async fn executions(&self, item_id: Uuid) -> Result<Vec<WorkExecutionView>> {
        bounded(async {
            let (mut tx, deadline) = self.begin().await?;
            self.item_on(&mut tx, item_id, false).await?;
            let employees: Vec<_> = self
                .principal
                .employee_ids
                .iter()
                .map(EmployeeId::as_str)
                .collect();
            let rows = sqlx::query(
                "SELECT x.run_id,x.employee_id,x.execution_version,r.status,j.artifact_id,x.reconciled_at IS NOT NULL AS reconciled,
                coalesce(x.result_code,j.last_error_code) AS output_code,x.context_version,
                a.id AS reference_id,prior.execution_version AS reference_version,
                x.reference_artifact_id IS NOT NULL AND a.id IS NULL AS reference_hidden,
                s.run_id IS NOT NULL AS frozen,ortak_run_work_context_current(x.company_id,x.run_id) AS context_current,
                CASE WHEN s.run_id IS NOT NULL AND ortak_run_work_context_current(x.company_id,x.run_id)
                  THEN ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{spec,context,work_context}' END AS context
                FROM work_executions x
                JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
                LEFT JOIN runtime_work_outputs j ON j.company_id=x.company_id AND j.run_id=x.run_id
                LEFT JOIN artifacts a ON a.company_id=x.company_id AND a.id=x.reference_artifact_id AND a.employee_id=ANY($3)
                LEFT JOIN work_executions prior ON prior.company_id=a.company_id AND prior.run_id=a.run_id
                LEFT JOIN run_context_snapshots s ON s.company_id=x.company_id AND s.run_id=x.run_id
                WHERE x.company_id=$1 AND x.work_item_id=$2 AND x.employee_id=ANY($3)
                ORDER BY x.requested_at DESC,x.run_id DESC LIMIT 20",
            )
            .bind(self.scope.company_id())
            .bind(item_id)
            .bind(employees)
            .fetch_all(&mut *tx)
            .await?;
            let result = rows
                .into_iter()
                .map(|row| {
                    Ok(WorkExecutionView {
                        run_id: row.try_get("run_id")?,
                        employee_id: EmployeeId::parse(row.try_get::<String, _>("employee_id")?)?,
                        execution_version: row.try_get("execution_version")?,
                        status: row.try_get("status")?,
                        artifact_id: row.try_get("artifact_id")?,
                        output_code: row.try_get("output_code")?,
                        reconciled: row.try_get("reconciled")?,
                        reference_context: reference_context(&row)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            self.finish(tx, deadline).await?;
            Ok(result)
        })
        .await
    }

    /// Read one same-item artifact after current project and canonical source authorization.
    pub async fn text_artifact(
        &self,
        item_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<WorkTextArtifact> {
        bounded(async {
            let (mut tx,deadline)=self.begin().await?;
            self.item_on(&mut tx,item_id,false).await?;
            let employees:Vec<_>=self.principal.employee_ids.iter().map(EmployeeId::as_str).collect();
            let row=sqlx::query("SELECT content_bytes,content_hash FROM artifacts WHERE company_id=$1 AND work_item_id=$2 AND id=$3 AND employee_id=ANY($4)")
                .bind(self.scope.company_id()).bind(item_id).bind(artifact_id).bind(employees).fetch_optional(&mut *tx).await?
                .ok_or(WorkError::WorkItemNotFound{work_item_id:item_id})?;
            let bytes:Vec<u8>=row.try_get("content_bytes")?;
            let hash:Vec<u8>=row.try_get("content_hash")?;
            use sha2::{Digest,Sha256};
            if bytes.is_empty() || bytes.len()>32768 || Sha256::digest(&bytes).as_slice()!=hash.as_slice() {
                return Err(invalid("artifact digest or bounds invalid"));
            }
            let result=WorkTextArtifact {content:String::from_utf8(bytes).map_err(|_|invalid("artifact encoding invalid"))?,sha256:hex::encode(hash)};
            self.finish(tx,deadline).await?;
            Ok(result)
        }).await
    }
}

fn reference_context(row: &sqlx::postgres::PgRow) -> Result<Option<WorkExecutionContext>> {
    if row.try_get::<i16, _>("context_version")? == 0 {
        return Ok(None);
    }
    let current = row.try_get::<bool, _>("context_current")?
        && !row.try_get::<bool, _>("reference_hidden")?;
    let frozen = row.try_get::<bool, _>("frozen")?;
    let context: Option<serde_json::Value> = row.try_get("context")?;
    let messages = context
        .as_ref()
        .and_then(|v| v.get("messages"))
        .and_then(|v| v.as_array());
    Ok(Some(WorkExecutionContext {
        status: if !current {
            "unavailable"
        } else if frozen {
            "ready"
        } else {
            "pending"
        }
        .into(),
        artifact_id: if current {
            row.try_get("reference_id")?
        } else {
            None
        },
        artifact_execution_version: if current {
            row.try_get("reference_version")?
        } else {
            None
        },
        message_ids: if current {
            messages
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    m.get("message_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .collect()
        } else {
            vec![]
        },
        history_limited: current
            && (context
                .as_ref()
                .and_then(|c| c.get("omitted_history"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || messages
                    .into_iter()
                    .flatten()
                    .any(|m| m.get("truncated").and_then(|v| v.as_bool()) == Some(true))),
    }))
}
