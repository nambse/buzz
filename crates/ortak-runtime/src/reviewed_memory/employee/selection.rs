use super::*;
use crate::DispatchAuthorization;
use sqlx::{PgConnection, Row};
mod lease;
mod rows;

pub(super) async fn select(
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    destination: EmployeeReviewedDestination,
) -> RuntimeResult<Option<ReviewedEmployeeSelection>> {
    tokio::time::timeout(Duration::from_secs(5),async {
        if scope.company_id()!=authority.company_id() || run.is_nil() || destination.target_id.is_nil()
            || destination.destination_channel_id.is_nil()
            || Some(destination.destination_channel_id)!=authority.input().channel_id {
            return Err(invalid());
        }
        let mut tx=control.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
            .execute(&mut *tx).await?;
        let witness=ortak_control::postgres::lock_office_authority_on(&mut tx,scope).await?;
        if let Some(work)=authority.work_origin() {
            sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 AND status='active' FOR SHARE")
                .bind(scope.company_id()).bind(work.project_id).fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        }
        let lease=lease::lease(&mut tx,scope,authority).await?;
        let DispatchAuthorization::Authorized(fresh)=crate::postgres::authorize_memory_selection_on(&mut tx,scope,&lease,witness.clone()).await?
            else {return Err(invalid())};
        if fresh.run_spec(run)?!=authority.run_spec(run)? || fresh.memory_binding()!=authority.memory_binding() {return Err(invalid());}
        // Check exclusion against retained run rows before any employee remote
        // request. No caller-supplied Work source, requester or payload mode.
        let mode=sqlx::query("SELECT coalesce(to_jsonb(r)->>'payload_mode','ordinary') AS mode,
            CASE WHEN r.work_item_id IS NULL THEN d.origin_type='human' ELSE w.source_message_id IS NOT NULL END AS eligible
            FROM runs r LEFT JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
            LEFT JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
            LEFT JOIN work_items w ON w.company_id=x.company_id AND w.project_id=x.project_id AND w.id=x.work_item_id
            WHERE r.company_id=$1 AND r.id=$2 AND r.employee_id=$3 AND r.employee_revision_id=$4")
            .bind(scope.company_id()).bind(run).bind(authority.employee_id().as_str()).bind(authority.employee_revision_id())
            .fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        if mode.try_get::<String,_>("mode")?!="ordinary" {return Err(invalid());}
        if !mode.try_get::<Option<bool>,_>("eligible")?.ok_or_else(invalid)? {tx.commit().await?;return Ok(None);}
        let observed=sqlx::query("SELECT origin_bytes,valid_before FROM ortak_employee_memory_run_origin($1,$2,$3)")
            .bind(scope.company_id()).bind(run).bind(destination.destination_channel_id)
            .fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        let origin=EmployeeMemoryOrigin::from_observation(&observed.try_get::<Vec<u8>,_>("origin_bytes")?)?;
        origin.validate_for(authority)?;
        let target=sqlx::query("SELECT deployment_id,creation_receipt FROM employee_reviewed_memory_targets
            WHERE company_id=$1 AND id=$2 AND employee_id=$3 AND destination_channel_id=$4 AND binding=$5
                AND enabled AND runtime_consumption_enabled AND valid_until>clock_timestamp()")
            .bind(scope.company_id()).bind(destination.target_id).bind(authority.employee_id().as_str())
            .bind(destination.destination_channel_id).bind(serde_json::to_value(authority.memory_binding()).map_err(|_|invalid())?)
            .fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        let mut selected=ReviewedEmployeeSelection{company_id:scope.company_id(),employee_id:authority.employee_id().clone(),
            binding:authority.memory_binding().cloned().ok_or_else(invalid)?,destination,origin,records:vec![],truncated:false,
            deployment_id:target.try_get("deployment_id")?,creation_receipt:target.try_get("creation_receipt")?};
        let initial=rows::read(&mut tx,&selected,run,None).await?;
        selected.truncated=initial.len()==32;
        let mut channels=vec![selected.origin.parsed()?.source.channel_id,destination.destination_channel_id];
        let ids=initial.iter().map(|r|r.try_get::<Uuid,_>("id")).collect::<std::result::Result<Vec<_>,_>>()?;
        for row in &initial {channels.push(row.try_get("source_channel_id")?);}
        channels.sort();channels.dedup();
        sqlx::query("SELECT channel_id FROM employee_memory_channel_authorities WHERE company_id=$1 AND employee_id=$2
            AND channel_id=ANY($3) ORDER BY channel_id FOR SHARE")
            .bind(scope.company_id()).bind(authority.employee_id().as_str()).bind(&channels).fetch_all(&mut *tx).await?;
        sqlx::query("SELECT id FROM employee_reviewed_memory_facts WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE")
            .bind(scope.company_id()).bind(&ids).fetch_all(&mut *tx).await?;
        sqlx::query("SELECT id FROM employee_reviewed_memory_targets WHERE company_id=$1 AND id=$2 FOR SHARE")
            .bind(scope.company_id()).bind(destination.target_id).fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        let current=rows::read(&mut tx,&selected,run,Some(&ids)).await?;
        let mut content_bytes=0usize;
        for row in current {
            let record=rows::record(&row)?;
            if selected.records.len()==8 || content_bytes+record.content.len()>8192 {
                selected.truncated=true;break;
            }
            record.validate()?;
            EmployeeContextRecord::Employee{record:record.clone()}.rendered()?;
            content_bytes+=record.content.len();
            selected.records.push(ReviewedEmployeeSelectionPin{pin:record.pin,provenance:record.provenance});
        }
        let now=sqlx::query("SELECT origin_bytes,valid_before FROM ortak_employee_memory_run_origin($1,$2,$3)")
            .bind(scope.company_id()).bind(run).bind(destination.destination_channel_id)
            .fetch_optional(&mut *tx).await?.ok_or_else(invalid)?;
        if EmployeeMemoryOrigin::from_observation(&now.try_get::<Vec<u8>,_>("origin_bytes")?)?!=selected.origin
            || !ortak_control::postgres::office_authority_matches_on(&mut tx,scope,&witness).await? {return Err(invalid());}
        tx.commit().await?;
        Ok(Some(selected))
    }).await.map_err(|_|invalid())?
}

fn invalid() -> crate::RunSupervisionError {
    crate::postgres::invalid("employee reviewed selection rejected".into())
}
