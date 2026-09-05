use nostr::PublicKey;
use ortak_control::CompanyScope;
use ortak_observability::{ActivityQueries, RunListCursor, RunListPage, RunListQuery};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
};

// Shared audience gate used before paging and for all single-run operations.
// Canonical event partition, author, kind and channel must match the inbox copy.
// Work/DM/noncanonical runs deliberately have no visibility in this API.
const VISIBLE_RUNS: &str = "
    SELECT r.id FROM runs r
    JOIN office_company_bindings b ON b.company_id = r.company_id AND b.community_id = $2
    JOIN office_inbox i ON i.company_id = r.company_id AND i.event_id = r.message_id
    JOIN events e ON e.community_id = b.community_id AND e.id = i.event_id AND e.created_at = i.event_created_at
      AND e.channel_id = i.channel_id AND e.kind = i.event_kind AND e.pubkey = i.author_pubkey
    JOIN channels c ON c.community_id = e.community_id AND c.id = e.channel_id
    WHERE r.company_id = $1 AND r.employee_id = ANY($5) AND r.work_item_id IS NULL
      AND e.channel_id = ANY($3) AND e.kind IN (9, 40002) AND e.deleted_at IS NULL
      AND c.channel_type::text = 'stream' AND c.deleted_at IS NULL
      AND (c.visibility::text = 'open' OR EXISTS (
        SELECT 1 FROM channel_members m WHERE m.community_id = b.community_id
          AND m.channel_id = e.channel_id AND m.pubkey = $4 AND m.removed_at IS NULL))";

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
        let mut sql = sqlx::QueryBuilder::new(VISIBLE_RUNS);
        sql.push(" AND r.id = $6");
        let row = sql
            .build()
            .bind(principal.scope.company_id())
            .bind(self.config.community_id)
            .bind(&principal.grant.channel_ids)
            .bind(principal.public_key.to_bytes().as_slice())
            .bind(employee_ids(principal))
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
    ) -> Result<()> {
        let mut connection = self.control.pool().acquire().await?;
        if !self
            .visible_run_on(&mut connection, principal, run_id)
            .await?
        {
            audit_on(
                &mut connection,
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
    ) -> Result<RunListPage> {
        let mut sql = sqlx::QueryBuilder::new(VISIBLE_RUNS);
        sql.push(
            "
            AND ($6::text IS NULL OR r.employee_id = $6)
            AND ($7::text[] IS NULL OR r.status = ANY($7))
            AND ($8::timestamptz IS NULL OR (r.queued_at, r.id) < ($8, $9::uuid))
            ORDER BY r.queued_at DESC, r.id DESC LIMIT $10",
        );
        let rows = sql
            .build()
            .bind(principal.scope.company_id())
            .bind(self.config.community_id)
            .bind(&principal.grant.channel_ids)
            .bind(principal.public_key.to_bytes().as_slice())
            .bind(employee_ids(principal))
            .bind(query.employee_id.as_ref().map(|id| id.as_str()))
            .bind(query.status_filter())
            .bind(query.cursor.map(|c| c.queued_at()))
            .bind(query.cursor.map(|c| c.run_id()))
            .bind(i64::from(query.page_size()) + 1)
            .fetch_all(self.control.pool())
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
