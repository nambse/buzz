use super::*;

impl<'a> Review<'a> {
    pub(super) fn new(
        state: &'a ApiState,
        principal: &'a Principal,
        employee: EmployeeId,
    ) -> Result<Self> {
        // This ceiling survives every recovery path. Operator does not imply the
        // new capability; only configured can_review_employee_memory grants it.
        if !principal.grant.employee_ids.contains(&employee) {
            return Err(forbidden());
        }
        Ok(Self {
            state,
            principal,
            employee,
        })
    }
    pub(super) fn scope(&self) -> &CompanyScope {
        &self.principal.scope
    }
    pub(super) fn actor(&self) -> [u8; 32] {
        self.principal.public_key.to_bytes()
    }
    pub(super) fn can_review(&self) -> bool {
        self.principal.grant.can_review_employee_memory
    }
    pub(super) fn channel_allowed(&self, channel: Uuid) -> bool {
        self.principal.grant.channel_ids.contains(&channel)
    }
    pub(super) async fn begin(&self) -> Result<(Transaction<'static, Postgres>, DateTime<Utc>)> {
        let mut tx = self.state.control.pool().begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL lock_timeout='500ms'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        // Prior statement: snapshots alone do not fence a concurrent mutation.
        sqlx::query("SELECT ortak_lock_office_authority($1)")
            .bind(self.scope().company_id())
            .execute(&mut *tx)
            .await?;
        if !crate::auth::human_allowed_on(
            &mut tx,
            self.scope(),
            self.state.config.community_id,
            &self.principal.public_key,
        )
        .await?
        {
            return Err(forbidden());
        }
        if !self.command_current(&mut tx, "stop").await? {
            return Err(forbidden());
        }
        let deadline = sqlx::query_scalar("SELECT clock_timestamp()+interval '5 seconds'")
            .fetch_one(&mut *tx)
            .await?;
        Ok((tx, deadline))
    }
    pub(super) async fn command_current(
        &self,
        connection: &mut PgConnection,
        action: &str,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT coalesce(ortak_employee_memory_command_current($1,$2,$3,$4),false)",
        )
        .bind(self.scope().company_id())
        .bind(self.employee.as_str())
        .bind(self.actor().as_slice())
        .bind(action)
        .fetch_one(connection)
        .await?)
    }
    pub(super) async fn finish(
        &self,
        mut tx: Transaction<'_, Postgres>,
        deadline: DateTime<Utc>,
    ) -> Result<()> {
        let current: bool = sqlx::query_scalar("SELECT clock_timestamp()<$1")
            .bind(deadline)
            .fetch_one(&mut *tx)
            .await?;
        if !current {
            return Err(ApiError::unavailable());
        }
        tx.commit().await?;
        Ok(())
    }
    pub(super) async fn lock_scopes(
        &self,
        connection: &mut PgConnection,
        source: Uuid,
        destination: Uuid,
    ) -> Result<()> {
        // Recovery must not register or revive a missing/closed scope. Lock only
        // retained keys, in the same ordering as approval and epoch mutations.
        sqlx::query(
            "SELECT channel_id FROM employee_memory_channel_authorities
            WHERE company_id=$1 AND community_id=$2 AND employee_id=$3 AND channel_id IN($4,$5)
            ORDER BY channel_id FOR SHARE",
        )
        .bind(self.scope().company_id())
        .bind(self.state.config.community_id)
        .bind(self.employee.as_str())
        .bind(source)
        .bind(destination)
        .fetch_all(connection)
        .await?;
        Ok(())
    }
}
