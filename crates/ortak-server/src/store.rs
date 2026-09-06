use nostr::PublicKey;
use ortak_control::CompanyScope;
use ortak_observability::{ActivityQueries, RunListCursor, RunListPage, RunListQuery};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::{Principal, RequestAuthority},
    error::{ApiError, Result},
    routes::ApiState,
};

mod direct;
mod visibility;
use direct::visible_direct_channels_on;
use visibility::{lock_projects_on, VISIBLE_RUNS};

impl ApiState {
    pub(crate) async fn office_delivery(
        &self,
        principal: &Principal,
        run_id: Uuid,
    ) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT CASE WHEN j.state='failed' OR o.state='failed' THEN 'failed'
                         WHEN o.state='delivered' THEN 'delivered' ELSE 'pending' END AS state,
                    CASE WHEN j.state='failed' THEN j.last_error_code
                         WHEN o.state='failed' THEN 'office_delivery_failed' END AS error_code,
                    o.delivered_at
             FROM runtime_office_outputs j
             LEFT JOIN outbox o ON o.company_id=j.company_id AND o.id=j.outbox_id
             WHERE j.company_id=$1 AND j.run_id=$2",
        )
        .bind(principal.scope.company_id())
        .bind(run_id)
        .fetch_optional(self.control.pool())
        .await?;
        row.map(|row| {
            Ok(serde_json::json!({
                "status": row.try_get::<String, _>("state")?,
                "error_code": row.try_get::<Option<String>, _>("error_code")?,
                "delivered_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("delivered_at")?,
            }))
        })
        .transpose()
    }

    pub(crate) async fn audit(
        &self,
        scope: &CompanyScope,
        key: &PublicKey,
        event: &[u8],
        action: &str,
        outcome: &str,
        run_id: Option<Uuid>,
    ) -> Result<()> {
        let mut connection = self.control.pool().acquire().await?;
        audit_on(&mut connection, scope, key, event, action, outcome, run_id).await
    }

    pub(crate) async fn audit_principal(
        &self,
        principal: &Principal,
        action: &str,
        outcome: &str,
        run_id: Option<Uuid>,
    ) -> Result<()> {
        self.audit(
            &principal.scope,
            &principal.public_key,
            &principal.auth_event_id,
            action,
            outcome,
            run_id,
        )
        .await
    }

    pub(crate) async fn visible_run_on(
        &self,
        connection: &mut PgConnection,
        principal: &Principal,
        run_id: Uuid,
    ) -> Result<bool> {
        lock_projects_on(connection, principal, &[run_id]).await?;
        let direct =
            visible_direct_channels_on(connection, principal, self.config.community_id).await?;
        let mut sql = sqlx::QueryBuilder::new(VISIBLE_RUNS);
        sql.push(" AND r.id = $7");
        let row = sql
            .build()
            .bind(principal.scope.company_id())
            .bind(self.config.community_id)
            .bind(&principal.grant.channel_ids)
            .bind(principal.public_key.to_bytes().as_slice())
            .bind(employee_ids(principal))
            .bind(&direct)
            .bind(run_id)
            .fetch_optional(connection)
            .await?;
        Ok(row.is_some())
    }

    pub(crate) async fn require_run(
        &self,
        principal: &Principal,
        run_id: Uuid,
        action: &str,
        authority: &RequestAuthority,
    ) -> Result<()> {
        let mut held = authority.0.lock().await;
        let connection = held.as_mut().ok_or_else(ApiError::unavailable)?;
        if !self.visible_run_on(connection, principal, run_id).await? {
            audit_on(
                connection,
                &principal.scope,
                &principal.public_key,
                &principal.auth_event_id,
                action,
                "not_found",
                Some(run_id),
            )
            .await?;
            return Err(ApiError::not_found());
        }
        Ok(())
    }

    pub(crate) async fn visible_runs(
        &self,
        principal: &Principal,
        query: &RunListQuery,
        authority: &RequestAuthority,
    ) -> Result<RunListPage> {
        let mut held = authority.0.lock().await;
        let connection = held.as_mut().ok_or_else(ApiError::unavailable)?;
        let direct =
            visible_direct_channels_on(connection, principal, self.config.community_id).await?;
        let mut sql = sqlx::QueryBuilder::new(VISIBLE_RUNS);
        sql.push(
            "
            AND ($7::text IS NULL OR r.employee_id = $7)
            AND ($8::text[] IS NULL OR r.status = ANY($8))
            AND ($9::timestamptz IS NULL OR (r.queued_at, r.id) < ($9, $10::uuid))
            ORDER BY r.queued_at DESC, r.id DESC LIMIT $11",
        );
        let rows = sql
            .build()
            .bind(principal.scope.company_id())
            .bind(self.config.community_id)
            .bind(&principal.grant.channel_ids)
            .bind(principal.public_key.to_bytes().as_slice())
            .bind(employee_ids(principal))
            .bind(&direct)
            .bind(query.employee_id.as_ref().map(|id| id.as_str()))
            .bind(query.status_filter())
            .bind(query.cursor.map(|c| c.queued_at()))
            .bind(query.cursor.map(|c| c.run_id()))
            .bind(i64::from(query.page_size()) + 1)
            .fetch_all(&mut **connection)
            .await?;
        let candidates = rows
            .iter()
            .map(|row| row.try_get::<Uuid, _>("id"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        lock_projects_on(connection, principal, &candidates).await?;
        // Recheck only the bounded candidate set under its project fences. A
        // grant revoked between selection and locking cannot leak a header.
        let mut recheck = sqlx::QueryBuilder::new(VISIBLE_RUNS);
        recheck.push(" AND r.id=ANY($7) ORDER BY r.queued_at DESC,r.id DESC");
        let rows = recheck
            .build()
            .bind(principal.scope.company_id())
            .bind(self.config.community_id)
            .bind(&principal.grant.channel_ids)
            .bind(principal.public_key.to_bytes().as_slice())
            .bind(employee_ids(principal))
            .bind(&direct)
            .bind(&candidates)
            .fetch_all(&mut **connection)
            .await?;
        let has_more = rows.len() > query.page_size() as usize;
        let mut runs = Vec::new();
        // Private MVP bounded fan-out: at most 25 real Activity detail reads.
        for row in rows.iter().take(query.page_size() as usize) {
            runs.push(
                self.control
                    .run_detail(&principal.scope, row.try_get("id")?)
                    .await?
                    .run,
            );
        }
        let next_cursor = if has_more {
            runs.last().map(RunListCursor::after)
        } else {
            None
        };
        Ok(RunListPage {
            runs,
            next_cursor,
            has_more,
        })
    }
}

pub(crate) async fn audit_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    key: &PublicKey,
    event: &[u8],
    action: &str,
    outcome: &str,
    run_id: Option<Uuid>,
) -> Result<()> {
    sqlx::query("INSERT INTO ortak_api_audit (company_id, actor_pubkey, auth_event_id, action, outcome, requested_run_id) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(scope.company_id()).bind(key.to_hex()).bind(event).bind(action).bind(outcome).bind(run_id)
        .execute(connection).await?;
    Ok(())
}

fn employee_ids(principal: &Principal) -> Vec<&str> {
    principal
        .grant
        .employee_ids
        .iter()
        .map(|id| id.as_str())
        .collect()
}
