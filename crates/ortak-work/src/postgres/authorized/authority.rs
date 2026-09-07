//! Live Office and durable project authorization under their shared fences.
use super::*;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

// The partition, author, kind, and channel in the inbox must agree with canonical storage.
// This expression is shared by detail and list queries, before their LIMIT.
pub(super) const SOURCE_VISIBLE: &str = "(w.source_message_id IS NULL OR EXISTS (
 SELECT 1 FROM office_inbox i
 JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
  AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
 WHERE i.company_id=w.company_id AND i.event_id=w.source_message_id AND i.state='decided'
  AND i.channel_id=$3 AND e.kind IN (9,40002) AND e.deleted_at IS NULL))";

impl AuthorizedWork {
    pub(super) async fn begin(&self) -> Result<(Transaction<'_, Postgres>, Option<DateTime<Utc>>)> {
        let mut tx = self.control.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout='500ms'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        // Must precede every operation/project/item row lock.
        let witness =
            ortak_control::postgres::lock_office_authority_on(&mut tx, &self.scope).await?;
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS (
 SELECT 1 FROM companies c
 JOIN office_company_bindings b ON b.company_id=c.id AND b.community_id=$2
 JOIN communities cm ON cm.id=b.community_id
 WHERE c.id=$1 AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL
 AND (EXISTS(SELECT 1 FROM relay_members rm WHERE rm.community_id=$2 AND rm.pubkey=$3)
      OR EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=$2 AND m.pubkey=$4 AND m.removed_at IS NULL))
 AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=$2 AND u.pubkey=$4
      AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM employee_office_bindings eb WHERE eb.company_id=$1 AND eb.public_key=$4)
 AND NOT EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=$2 AND m.pubkey=$4 AND m.role='bot'))")
            .bind(self.scope.company_id()).bind(self.principal.community_id)
            .bind(&self.principal.public_key).bind(&self.principal.key_bytes).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(WorkError::AccessDenied);
        }
        Ok((tx, witness.valid_before()))
    }
    pub(super) async fn finish(
        &self,
        mut tx: Transaction<'_, Postgres>,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let live: bool =
            sqlx::query_scalar("SELECT $1::timestamptz IS NULL OR clock_timestamp()<$1")
                .bind(deadline)
                .fetch_one(&mut *tx)
                .await?;
        if !live {
            return Err(WorkError::OperationTimedOut);
        }
        tx.commit().await?;
        Ok(())
    }
    pub(super) async fn channel_on(&self, c: &mut PgConnection, channel: Uuid) -> Result<bool> {
        if !self.principal.channel_ids.contains(&channel) {
            return Ok(false);
        }
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM channels c JOIN channel_members m
 ON m.community_id=c.community_id AND m.channel_id=c.id AND m.pubkey=$3 AND m.removed_at IS NULL
 WHERE c.community_id=$1 AND c.id=$2 AND c.channel_type::text='stream' AND c.deleted_at IS NULL)",
        )
        .bind(self.principal.community_id)
        .bind(channel)
        .bind(&self.principal.key_bytes)
        .fetch_one(c)
        .await?)
    }
    pub(super) async fn project_on(&self, c: &mut PgConnection, id: Uuid) -> Result<ApiProject> {
        let missing = || WorkError::ProjectNotFound { project_id: id };
        // Locks the parent even when the grant is absent; ACL writes take this row exclusively.
        let row = sqlx::query(PROJECT_FOR_SHARE_SQL)
            .bind(self.scope.company_id())
            .bind(id)
            .fetch_optional(&mut *c)
            .await?
            .ok_or_else(missing)?;
        let grant=sqlx::query("SELECT a.channel_id,g.role FROM project_api_bindings a
 JOIN project_access_grants g ON g.company_id=a.company_id AND g.project_id=a.project_id
 WHERE a.company_id=$1 AND a.project_id=$2 AND a.community_id=$3 AND g.actor_pubkey=$4 AND g.revoked_at IS NULL")
            .bind(self.scope.company_id()).bind(id).bind(self.principal.community_id).bind(&self.principal.public_key)
            .fetch_optional(&mut *c).await?.ok_or_else(missing)?;
        let channel_id = grant.try_get("channel_id")?;
        if !self.channel_on(c, channel_id).await? {
            return Err(missing());
        }
        Ok(ApiProject {
            record: project_record(&row)?,
            channel_id,
            role: ProjectRole::parse(grant.try_get("role")?)?,
        })
    }
    pub(super) async fn source_on(
        &self,
        c: &mut PgConnection,
        channel: Uuid,
        hex: &str,
    ) -> Result<()> {
        let message = MessageId::parse_hex(hex)?;
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM office_inbox i
 JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
 AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
 WHERE i.company_id=$1 AND i.event_id=$4 AND i.channel_id=$3 AND i.state='decided'
 AND e.kind IN(9,40002) AND e.deleted_at IS NULL)",
        )
        .bind(self.scope.company_id())
        .bind(self.principal.community_id)
        .bind(channel)
        .bind(message.as_bytes().as_slice())
        .fetch_one(c)
        .await?;
        if !visible {
            return Err(WorkError::SourceMessageNotDecided {
                message_id: hex.into(),
            });
        }
        Ok(())
    }
    pub(super) async fn item_on(
        &self,
        c: &mut PgConnection,
        id: Uuid,
        write: bool,
    ) -> Result<(ApiProject, WorkItemAggregate)> {
        let missing = || WorkError::WorkItemNotFound { work_item_id: id };
        let project_id: Uuid = sqlx::query_scalar(ITEM_PROJECT_SQL)
            .bind(self.scope.company_id())
            .bind(id)
            .fetch_optional(&mut *c)
            .await?
            .ok_or_else(missing)?;
        let project = match self.project_on(c, project_id).await {
            Err(WorkError::ProjectNotFound { .. }) => return Err(missing()),
            other => other?,
        };
        let mut q = sqlx::QueryBuilder::new(
            "SELECT w.id FROM work_items w WHERE w.company_id=$1 AND w.id=$4 AND ",
        );
        q.push(SOURCE_VISIBLE).push(if write {
            " FOR UPDATE OF w"
        } else {
            " FOR SHARE OF w"
        });
        q.build()
            .bind(self.scope.company_id())
            .bind(self.principal.community_id)
            .bind(project.channel_id)
            .bind(id)
            .fetch_optional(&mut *c)
            .await?
            .ok_or_else(missing)?;
        Ok((project, require_aggregate(c, &self.scope, id).await?))
    }
    pub(super) async fn employee_on(
        &self,
        c: &mut PgConnection,
        channel: Uuid,
        id: &EmployeeId,
    ) -> Result<()> {
        let valid=self.principal.employee_ids.contains(id) && sqlx::query_scalar::<_,bool>("SELECT EXISTS(
 SELECT 1 FROM employees e
 JOIN employee_revisions rev ON rev.company_id=e.company_id AND rev.employee_id=e.id AND rev.id=e.active_revision_id
 JOIN employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
 AND encode(b.public_key,'hex')=rev.manifest #>> '{office,public_key}' AND b.signer_ref=rev.manifest #>> '{office,signer_ref}'
 JOIN channel_members m ON m.community_id=$3 AND m.channel_id=$4 AND m.pubkey=b.public_key AND m.removed_at IS NULL
 WHERE e.company_id=$1 AND e.id=$2 AND e.status='active' AND b.verified_at IS NOT NULL
 AND b.valid_from<=clock_timestamp() AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
 AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=$3 AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL))")
            .bind(self.scope.company_id()).bind(id.as_str()).bind(self.principal.community_id).bind(channel).fetch_one(c).await?;
        if !valid {
            return Err(WorkError::EmployeeNotAssignable {
                employee_id: id.clone(),
            });
        }
        Ok(())
    }
    pub(super) fn contribute(&self, role: ProjectRole) -> Result<()> {
        if self.principal.operator && role.contributes() {
            Ok(())
        } else {
            Err(WorkError::AccessDenied)
        }
    }
    pub(super) async fn execution_employee_on(
        &self,
        c: &mut PgConnection,
        channel: Uuid,
        id: &EmployeeId,
    ) -> Result<()> {
        self.employee_on(c, channel, id).await?;
        let selected: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM office_routing_cohorts cohort
            JOIN office_routing_channels h ON h.company_id=cohort.company_id AND h.community_id=cohort.community_id
            JOIN office_routing_employees e ON e.company_id=cohort.company_id
            WHERE cohort.company_id=$1 AND cohort.community_id=$2 AND cohort.state='enabled'
            AND h.channel_id=$3 AND e.employee_id=$4)")
            .bind(self.scope.company_id()).bind(self.principal.community_id).bind(channel)
            .bind(id.as_str()).fetch_one(c).await?;
        if !selected {
            return Err(WorkError::EmployeeNotAssignable {
                employee_id: id.clone(),
            });
        }
        Ok(())
    }
    pub(super) fn review(&self, role: ProjectRole) -> Result<()> {
        if self.principal.operator && role.reviews() {
            Ok(())
        } else {
            Err(WorkError::AccessDenied)
        }
    }
}
