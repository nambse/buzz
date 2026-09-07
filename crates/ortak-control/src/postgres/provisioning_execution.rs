//! Repository-bound management leases. Ordinary operator CLI repositories remain unchanged.
use sqlx::PgConnection;
use uuid::Uuid;

use super::PgControlPlane;
use crate::{CompanyScope, ControlError, Result};

#[derive(Clone, Debug)]
pub(super) struct Execution {
    company: Uuid,
    command: Uuid,
    token: Uuid,
}

impl PgControlPlane {
    /// Applies one leased disable intent atomically. No credentials or adapters
    /// are needed; durable runtime reconciliation drains the invalidated runs.
    pub async fn disable_employee_for_command(&self, scope: &CompanyScope) -> Result<()> {
        let execution = self.provisioning_execution.as_ref().ok_or_else(|| {
            ControlError::InvalidData("disable requires a managed command".into())
        })?;
        let mut tx = self.pool.begin().await?;
        let locked: bool = sqlx::query_scalar(
            "SELECT pg_try_advisory_xact_lock(ortak_office_company_lock_key($1))",
        )
        .bind(scope.company_id())
        .fetch_one(&mut *tx)
        .await?;
        if !locked {
            return Err(ControlError::InvalidData(
                "disable authority is busy".into(),
            ));
        }
        self.provisioning_guard_on(&mut tx, scope, None).await?;
        let changed=sqlx::query("UPDATE employees e SET status='disabled',updated_at=clock_timestamp() FROM employee_management_commands c WHERE c.company_id=$1 AND c.id=$2 AND c.action='disable' AND e.company_id=c.company_id AND e.id=c.employee_id AND e.status IN('active','paused') AND e.lifecycle_epoch=c.employee_lifecycle_epoch AND e.active_revision_id IS NOT DISTINCT FROM c.expected_revision_id")
            .bind(scope.company_id()).bind(execution.command).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(ControlError::InvalidData("disable intent changed".into()));
        }
        sqlx::query("UPDATE employee_management_commands SET status='succeeded',lease_token=NULL,lease_expires_at=NULL,error_code=NULL,updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(scope.company_id()).bind(execution.command).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Refuses an old operation before any credential lookup or adapter work.
    /// Retained compensation uses its separate database-only path.
    pub async fn check_operation_lifecycle(
        &self,
        scope: &CompanyScope,
        operation: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        super::lock_office_authority_on(&mut tx, scope).await?;
        self.provisioning_guard_on(&mut tx, scope, Some(operation))
            .await?;
        let reenable = self
            .reenable_operation_on(&mut tx, scope, operation)
            .await?;
        let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provisioning_operations o LEFT JOIN employees e ON e.company_id=o.company_id AND e.id=o.employee_id WHERE o.company_id=$1 AND o.id=$2 AND o.employee_lifecycle_epoch=coalesce(e.lifecycle_epoch,0) AND (e.status IS DISTINCT FROM 'disabled' OR $3))")
            .bind(scope.company_id()).bind(operation).bind(reenable).fetch_one(&mut *tx).await?;
        if !current {
            return Err(ControlError::InvalidData(
                "provisioning lifecycle changed".into(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn reenable_operation_on(
        &self,
        connection: &mut PgConnection,
        scope: &CompanyScope,
        operation: Uuid,
    ) -> Result<bool> {
        let Some(execution) = &self.provisioning_execution else {
            return Ok(false);
        };
        self.provisioning_guard_on(connection, scope, Some(operation))
            .await?;
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM employee_management_commands c JOIN employees e ON e.company_id=c.company_id AND e.id=c.employee_id WHERE c.company_id=$1 AND c.id=$2 AND c.operation_id=$3 AND c.action='reenable' AND e.status='disabled' AND c.employee_lifecycle_epoch=e.lifecycle_epoch AND c.expected_revision_id IS NOT DISTINCT FROM e.active_revision_id)")
            .bind(scope.company_id()).bind(execution.command).bind(operation).fetch_one(connection).await?)
    }

    /// Binds a repository to a currently authorized, leased prepared-resource
    /// command. Its identity cannot subsequently be replaced by callers. Each
    /// provisioning write rechecks the lease and current authority in its own
    /// transaction, including a deferred check at commit.
    pub async fn for_provisioning_command(
        &self,
        scope: &CompanyScope,
        command: Uuid,
        token: Uuid,
    ) -> Result<Self> {
        let control = Self {
            pool: self.pool.clone(),
            provisioning_execution: Some(Execution {
                company: scope.company_id(),
                command,
                token,
            }),
        };
        control.check_provisioning_execution(scope).await?;
        Ok(control)
    }

    /// Rechecks command authority before adapter construction or external I/O.
    /// No database transaction is held while the caller performs that I/O.
    pub async fn check_provisioning_execution(&self, scope: &CompanyScope) -> Result<()> {
        if self.provisioning_execution.is_none() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        self.provisioning_guard_on(&mut tx, scope, None).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn provisioning_guard_on(
        &self,
        connection: &mut PgConnection,
        scope: &CompanyScope,
        operation: Option<Uuid>,
    ) -> Result<()> {
        let Some(execution) = &self.provisioning_execution else {
            return Ok(());
        };
        if execution.company != scope.company_id() {
            return Err(ControlError::InvalidData(
                "management scope mismatch".into(),
            ));
        }
        // The function obtains Office -> policy -> command locks in that order.
        // It records a transaction-local, deferred commit guard as well.
        sqlx::query("SELECT ortak_management_guard($1,$2,$3,$4)")
            .bind(execution.company)
            .bind(execution.command)
            .bind(execution.token)
            .bind(operation)
            .execute(connection)
            .await?;
        Ok(())
    }
}
