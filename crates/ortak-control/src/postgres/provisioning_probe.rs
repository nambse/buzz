//! Retained, bounded diagnostic admission shared by managed and CLI attempts.
use super::*;
use crate::CompanyScope;
use chrono::{DateTime, Utc};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

/// Database-issued diagnostic identity. Fields cannot be manufactured by callers.
#[derive(Clone, Debug)]
pub struct ProvisioningRuntimeProbe {
    company: Uuid,
    operation: Uuid,
    probe: Uuid,
    generation: i32,
    origin: String,
    token_env: String,
    state: String,
    deadline: DateTime<Utc>,
}
impl ProvisioningRuntimeProbe {
    /// Stable child identity, persisted before bridge admission.
    pub fn id(&self) -> Uuid {
        self.probe
    }
    /// Original operation; an older operation can require cleanup first.
    pub fn operation_id(&self) -> Uuid {
        self.operation
    }
    /// Current retained state.
    pub fn state(&self) -> &str {
        &self.state
    }
    /// Original selected bridge, needed for exact cleanup after selection changes.
    pub fn origin(&self) -> &str {
        &self.origin
    }
    /// Credential environment reference, never its value.
    pub fn token_environment(&self) -> &str {
        &self.token_env
    }
    /// Fixed admission deadline; reconnecting never renews it.
    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }
}
fn probe(row: PgRow) -> Result<ProvisioningRuntimeProbe> {
    Ok(ProvisioningRuntimeProbe {
        company: row.try_get("company_id")?,
        operation: row.try_get("operation_id")?,
        probe: row.try_get("probe_id")?,
        generation: row.try_get("generation")?,
        origin: row.try_get("bridge_origin")?,
        token_env: row.try_get("bridge_token_env")?,
        state: row.try_get("state")?,
        deadline: row.try_get("deadline")?,
    })
}
fn refused() -> ControlError {
    ControlError::InvalidData("runtime probe authority or attempt changed".into())
}

impl PgControlPlane {
    /// Rechecks the sealed operation plus the current Office/company boundary
    /// during external diagnostics. Cleanup does not require this authority.
    pub async fn check_provisioning_runtime_probe_authority(
        &self,
        scope: &CompanyScope,
        operation: Uuid,
    ) -> Result<()> {
        self.check_operation_lifecycle(scope, operation).await?;
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id JOIN communities cm ON cm.id=b.community_id JOIN provisioning_operations o ON o.company_id=c.id AND o.id=$3 WHERE c.id=$1 AND b.community_id=$2 AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND NOT o.dry_run AND o.status IN('pending','running','failed') AND o.result_revision_id IS NULL)")
            .bind(scope.company_id()).bind(scope.community_id()).bind(operation).fetch_one(&self.pool).await?;
        if !active {
            return Err(refused());
        }
        Ok(())
    }
    /// Reads the employee's pending child first, otherwise this operation's last
    /// receipt. Current authority is required before exposing recovery references.
    pub async fn provisioning_runtime_probe(
        &self,
        scope: &CompanyScope,
        operation: Uuid,
    ) -> Result<Option<ProvisioningRuntimeProbe>> {
        self.check_provisioning_runtime_probe_authority(scope, operation)
            .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='500ms'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        self.provisioning_guard_on(&mut tx, scope, Some(operation))
            .await?;
        let row=sqlx::query("SELECT p.* FROM provisioning_runtime_probes p JOIN provisioning_operations o ON o.company_id=p.company_id AND o.employee_id=p.employee_id WHERE o.company_id=$1 AND o.id=$2 AND (p.state='running' OR p.operation_id=o.id) ORDER BY (p.state='running') DESC,p.generation DESC LIMIT 1")
            .bind(scope.company_id()).bind(operation).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        row.map(probe).transpose()
    }

    /// Persists one fresh identity under the operation lock. A previous running
    /// child anywhere for this employee prevents another admission.
    pub async fn admit_provisioning_runtime_probe(
        &self,
        scope: &CompanyScope,
        operation: Uuid,
        origin: &str,
        token_env: &str,
        previous: Option<Uuid>,
    ) -> Result<ProvisioningRuntimeProbe> {
        self.check_provisioning_runtime_probe_authority(scope, operation)
            .await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='500ms'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        lock_office_authority_on(&mut tx, scope).await?;
        self.provisioning_guard_on(&mut tx, scope, Some(operation))
            .await?;
        let employee:Option<String>=sqlx::query_scalar("SELECT employee_id FROM provisioning_operations WHERE company_id=$1 AND id=$2 AND status IN('pending','running','failed') AND NOT dry_run FOR UPDATE")
            .bind(scope.company_id()).bind(operation).fetch_optional(&mut *tx).await?;
        let employee = employee.ok_or_else(refused)?;
        let latest=sqlx::query("SELECT probe_id,generation FROM provisioning_runtime_probes WHERE company_id=$1 AND operation_id=$2 ORDER BY generation DESC LIMIT 1")
            .bind(scope.company_id()).bind(operation).fetch_optional(&mut *tx).await?;
        let last = latest
            .as_ref()
            .map(|r| r.try_get::<Uuid, _>("probe_id"))
            .transpose()?;
        let generation = latest
            .as_ref()
            .map(|r| r.try_get::<i32, _>("generation"))
            .transpose()?
            .unwrap_or(0)
            + 1;
        if last != previous || generation > 20 {
            return Err(refused());
        }
        let row=sqlx::query("INSERT INTO provisioning_runtime_probes(company_id,operation_id,employee_id,generation,probe_id,bridge_origin,bridge_token_env,state,created_at,deadline) VALUES($1,$2,$3,$4,$5,$6,$7,'running',clock_timestamp(),clock_timestamp()+interval '89 seconds') RETURNING *")
            .bind(scope.company_id()).bind(operation).bind(employee).bind(generation).bind(Uuid::new_v4()).bind(origin).bind(token_env).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE provisioning_operations SET status='running',current_step='validate_runtime_profile',error_message=NULL,finished_at=NULL,updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(scope.company_id()).bind(operation).execute(&mut *tx).await?;
        tx.commit().await?;
        probe(row)
    }

    /// Retains exact containment after the bridge acknowledged it. Failure
    /// accounting is permitted after revocation; a success needs current authority.
    pub async fn settle_provisioning_runtime_probe(
        &self,
        scope: &CompanyScope,
        selected: &ProvisioningRuntimeProbe,
        error: Option<&str>,
    ) -> Result<()> {
        if selected.company != scope.company_id() {
            return Err(refused());
        }
        if error.is_none() {
            self.check_provisioning_runtime_probe_authority(scope, selected.operation)
                .await?;
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='500ms'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        if error.is_none() {
            lock_office_authority_on(&mut tx, scope).await?;
            self.provisioning_guard_on(&mut tx, scope, Some(selected.operation))
                .await?;
        }
        let affected=sqlx::query("UPDATE provisioning_runtime_probes SET state=CASE WHEN $5::text IS NULL THEN 'succeeded' ELSE 'failed' END,contained_at=clock_timestamp(),error_code=$5 WHERE company_id=$1 AND operation_id=$2 AND generation=$3 AND probe_id=$4 AND state='running'")
            .bind(scope.company_id()).bind(selected.operation).bind(selected.generation).bind(selected.probe).bind(error).execute(&mut *tx).await?.rows_affected();
        if affected != 1 {
            return Err(refused());
        }
        tx.commit().await?;
        Ok(())
    }
}
