//! Both run origins share current configured employee and source audiences.
use super::*;

// Work reads retain archived/reviewed items for recovery. Starting or cancelling
// has a separate current contributor gate. Promoted sources remain canonical.
pub(super) const VISIBLE_RUNS: &str = "
 SELECT r.id FROM runs r
 LEFT JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
 LEFT JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
 WHERE r.company_id=$1 AND r.employee_id=ANY($5) AND (
 (r.work_item_id IS NULL AND EXISTS (
  SELECT 1 FROM office_company_bindings b
  JOIN office_inbox i ON i.company_id=b.company_id AND i.event_id=r.message_id
  JOIN events e ON e.community_id=b.community_id AND e.id=i.event_id AND e.created_at=i.event_created_at
    AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
  JOIN channels c ON c.community_id=e.community_id AND c.id=e.channel_id
  WHERE b.company_id=r.company_id AND b.community_id=$2
    AND e.channel_id=ANY($3) AND e.kind IN(9,40002) AND e.deleted_at IS NULL
    AND (c.channel_type::text='stream' OR (c.channel_type::text='dm' AND c.id=ANY($6)
      AND EXISTS(SELECT 1 FROM channel_members dm JOIN employee_office_bindings identity
        ON identity.company_id=r.company_id AND identity.public_key=dm.pubkey AND identity.employee_id=r.employee_id
        WHERE dm.community_id=c.community_id AND dm.channel_id=c.id))) AND c.deleted_at IS NULL
    AND (c.visibility::text='open' OR EXISTS(SELECT 1 FROM channel_members m
      WHERE m.community_id=b.community_id AND m.channel_id=e.channel_id AND m.pubkey=$4 AND m.removed_at IS NULL))))
 OR (r.work_item_id=w.id AND r.employee_id=x.employee_id AND EXISTS (
  SELECT 1 FROM projects p
  JOIN project_api_bindings b ON b.company_id=p.company_id AND b.project_id=p.id
  JOIN project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id
  JOIN channels c ON c.community_id=b.community_id AND c.id=b.channel_id
  JOIN channel_members m ON m.community_id=c.community_id AND m.channel_id=c.id AND m.pubkey=$4 AND m.removed_at IS NULL
  WHERE p.company_id=r.company_id AND p.id=w.project_id AND b.community_id=$2
    AND b.channel_id=ANY($3) AND g.actor_pubkey=encode($4,'hex') AND g.revoked_at IS NULL
    AND c.channel_type::text='stream' AND c.deleted_at IS NULL
    AND (w.source_message_id IS NULL OR EXISTS(SELECT 1 FROM office_inbox i
      JOIN events e ON e.community_id=b.community_id AND e.id=i.event_id AND e.created_at=i.event_created_at
        AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
      WHERE i.company_id=w.company_id AND i.event_id=w.source_message_id AND i.state='decided'
        AND i.channel_id=b.channel_id AND e.kind IN(9,40002) AND e.deleted_at IS NULL)))))";

pub(super) async fn lock_projects_on(
    connection: &mut PgConnection,
    principal: &Principal,
    run_ids: &[Uuid],
) -> Result<()> {
    // A page supplies at most 26 IDs. ACL mutations take these parents exclusively;
    // lock them before any run row, in deterministic order across mixed pages.
    sqlx::query("SELECT p.id FROM projects p WHERE p.company_id=$1 AND EXISTS(
        SELECT 1 FROM work_executions x WHERE x.company_id=p.company_id AND x.project_id=p.id AND x.run_id=ANY($2))
        ORDER BY p.id FOR SHARE OF p")
        .bind(principal.scope.company_id()).bind(run_ids).fetch_all(connection).await?;
    Ok(())
}

impl ApiState {
    pub(crate) async fn work_output(
        &self,
        principal: &Principal,
        run_id: Uuid,
    ) -> Result<Option<serde_json::Value>> {
        let row=sqlx::query("SELECT j.state,j.artifact_id,coalesce(x.result_code,j.last_error_code) AS code,x.work_item_id
            FROM runtime_work_outputs j JOIN work_executions x ON x.company_id=j.company_id AND x.run_id=j.run_id
            WHERE j.company_id=$1 AND j.run_id=$2")
            .bind(principal.scope.company_id()).bind(run_id).fetch_optional(self.control.pool()).await?;
        row.map(|row| Ok(serde_json::json!({"status":row.try_get::<String,_>("state")?,
            "artifact_id":row.try_get::<Option<Uuid>,_>("artifact_id")?,"work_item_id":row.try_get::<Uuid,_>("work_item_id")?,
            "error_code":row.try_get::<Option<String>,_>("code")?}))).transpose()
    }

    pub(crate) async fn can_cancel_run_on(
        &self,
        connection: &mut PgConnection,
        principal: &Principal,
        run_id: Uuid,
    ) -> Result<bool> {
        if principal.grant.role != crate::Role::Operator {
            return Ok(false);
        }
        // The caller already holds Office + the Work project fence and checks
        // the read audience. An operator's viewer grant never grants execution.
        Ok(sqlx::query_scalar(
            "SELECT r.work_item_id IS NULL OR EXISTS(
            SELECT 1 FROM work_executions x JOIN project_access_grants g
            ON g.company_id=x.company_id AND g.project_id=x.project_id
            WHERE x.company_id=r.company_id AND x.run_id=r.id AND g.actor_pubkey=$3
            AND g.revoked_at IS NULL AND g.role IN('owner','contributor'))
            FROM runs r WHERE r.company_id=$1 AND r.id=$2",
        )
        .bind(principal.scope.company_id())
        .bind(run_id)
        .bind(principal.public_key.to_hex())
        .fetch_optional(connection)
        .await?
        .unwrap_or(false))
    }
}
