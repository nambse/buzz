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
                coalesce(x.result_code,j.last_error_code) AS output_code FROM work_executions x
                JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
                LEFT JOIN runtime_work_outputs j ON j.company_id=x.company_id AND j.run_id=x.run_id
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
