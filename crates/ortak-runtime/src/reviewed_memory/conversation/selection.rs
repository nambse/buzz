use super::*;
use crate::{DispatchAuthorization, Result as RuntimeResult};
use sqlx::Row;

mod candidates;
mod source;

#[allow(clippy::too_many_arguments)]
pub(super) async fn select(
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    project: Uuid,
    query: &str,
    include_project: bool,
) -> RuntimeResult<Option<ReviewedConversationSelection>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        if authority.company_id() != scope.company_id() || run.is_nil() || project.is_nil()
            || authority.work_origin().is_some_and(|work| work.run_id != run || work.project_id != project)
            || (include_project && authority.work_origin().is_none())
        {
            return Err(invalid());
        }
        let binding = authority.memory_binding().cloned().ok_or_else(invalid)?;
        let mut tx = control.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
            .execute(&mut *tx).await?;
        let witness = ortak_control::postgres::lock_office_authority_on(&mut tx, scope).await?;
        sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 AND status='active' FOR SHARE")
            .bind(scope.company_id()).bind(project).fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        let lease = source::lease(&mut tx, scope, authority).await?;
        let fresh = crate::postgres::authorize_memory_selection_on(&mut tx, scope, &lease, witness.clone()).await?;
        let DispatchAuthorization::Authorized(fresh) = fresh else {return Err(invalid())};
        if fresh.run_spec(run)? != authority.run_spec(run)? || fresh.memory_binding() != authority.memory_binding() {
            return Err(invalid());
        }
        if !source::requires_origin(&mut tx, scope, authority, run, project).await? {
            tx.commit().await?;
            return Ok(None);
        }
        let row = sqlx::query("SELECT requester_public_key,provenance_bytes,valid_before
            FROM ortak_conversation_run_origin($1,$2,$3)")
            .bind(scope.company_id()).bind(run).bind(project).fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        let provenance: Vec<u8> = row.try_get("provenance_bytes")?;
        let origin = ConversationMemoryOrigin::from_observation(&row.try_get::<Vec<u8>,_>("requester_public_key")?, &provenance)?;
        let parsed = origin.parsed_provenance()?;
        let audience = parsed.audience();
        if audience.company_id() != scope.company_id() || audience.project_id() != project
            || audience.employee_id() != authority.employee_id() || Some(audience.channel_id()) != authority.input().channel_id
        {
            return Err(invalid());
        }
        sqlx::query("SELECT epoch FROM conversation_memory_authorities
            WHERE company_id=$1 AND community_id=$2 AND project_id=$3 AND channel_id=$4 FOR SHARE")
            .bind(scope.company_id()).bind(audience.community_id()).bind(project).bind(audience.channel_id())
            .fetch_optional(&mut *tx).await?;
        let mut selected = ReviewedConversationSelection {company_id:scope.company_id(),project_id:project,
            employee_id:authority.employee_id().clone(),binding,origin,records:vec![],truncated:false};
        if !query.is_empty() {
            let rows = candidates::read(&mut tx, &mut selected, run, query, include_project).await?;
            candidates::choose(&mut selected, authority, rows)?;
        }
        let live: bool = sqlx::query_scalar("SELECT $1::timestamptz IS NULL OR $1>clock_timestamp()")
            .bind(row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("valid_before")?)
            .fetch_one(&mut *tx).await?;
        if !live || !ortak_control::postgres::office_authority_matches_on(&mut tx,scope,&witness).await? {
            return Err(invalid());
        }
        tx.commit().await?;
        Ok(Some(selected))
    }).await.map_err(|_| invalid())?
}

fn invalid() -> crate::RunSupervisionError {
    crate::postgres::invalid("conversation reviewed selection rejected".into())
}
