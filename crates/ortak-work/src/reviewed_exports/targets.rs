use super::*;
use std::collections::BTreeSet;

/// Replaces one worker's complete finite project advertisement after actual
/// validation. The current active revision/memory/lifecycle are joined in SQL.
/// Retired targets remain as immutable cleanup provenance.
pub async fn advertise_targets(
    control: &PgControlPlane,
    scope: &CompanyScope,
    targets: &[ReviewedMemoryTarget],
) -> Result<usize> {
    advertise_targets_with_conversations(control, scope, targets, &[]).await
}

/// Advertises a complete owned recipe with separate, explicit conversation
/// selections. Removing a selection retires its old consumption epoch; ordinary
/// refresh preserves it. Current project/channel identity is checked in SQL.
pub async fn advertise_targets_with_conversations(
    control: &PgControlPlane,
    scope: &CompanyScope,
    targets: &[ReviewedMemoryTarget],
    conversations: &[ReviewedConversationTarget],
) -> Result<usize> {
    bounded(async {
        if targets.len()>128 || conversations.len()>128{return Err(invalid());}
        let mut unique=BTreeSet::new();
        let mut prepared=Vec::new();
        for target in targets {
            let receipt=&target.creation_receipt;
            if target.project_id.is_nil()||target.deployment_id.is_nil()||target.valid_for.is_zero()
                ||target.binding.adapter!="honcho"||!unique.insert((target.project_id,target.employee_id.clone()))
                ||serde_json::to_vec(receipt).map_err(|_|invalid())?.len()>16384
                ||receipt["company_id"]!=json!(scope.company_id())||receipt["employee_id"]!=json!(target.employee_id)
                ||receipt["deployment_id"]!=json!(target.deployment_id)||receipt["binding"]!=json!(target.binding)
                ||receipt["request_hash"].as_str().is_none_or(|v|v.len()!=64||!v.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))
                ||!receipt["native_ids"].is_object(){return Err(invalid());}
            prepared.push((target,hash(&json!({"request_hash":receipt["request_hash"],"native_ids":receipt["native_ids"]}))?));
        }
        let mut selected=std::collections::BTreeMap::new();
        let mut channels=BTreeSet::new();
        let mut counts=std::collections::BTreeMap::new();
        for selection in conversations {
            let key=(selection.project_id,selection.employee_id.clone());
            let count=counts.entry(selection.employee_id.clone()).or_insert(0usize);
            *count+=1;
            if selection.project_id.is_nil() || selection.channel_id.is_nil() || *count>16
                || !unique.contains(&key) || selected.insert(key,selection.channel_id).is_some()
                || !channels.insert((selection.employee_id.clone(),selection.channel_id)) {
                return Err(invalid());
            }
        }
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        let witness=ortak_control::postgres::lock_office_authority_on(&mut tx,scope).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))").bind(format!("ortak-reviewed-targets:{}",scope.company_id())).execute(&mut *tx).await?;
        let project_ids:Vec<_>=unique.iter().map(|(project,_)|*project).collect();
        sqlx::query("SELECT p.id FROM projects p WHERE p.company_id=$1 AND
            (p.id=ANY($2) OR EXISTS(SELECT 1 FROM reviewed_memory_targets t
                WHERE t.company_id=p.company_id AND t.project_id=p.id AND t.enabled))
            ORDER BY p.id FOR SHARE OF p NOWAIT")
            .bind(scope.company_id()).bind(&project_ids).fetch_all(&mut *tx).await?;
        for ((project,_),channel) in &selected {
            let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_api_bindings b
                WHERE b.company_id=$1 AND b.project_id=$2 AND b.community_id=$3 AND b.channel_id=$4)")
                .bind(scope.company_id()).bind(project).bind(scope.community_id()).bind(channel)
                .fetch_one(&mut *tx).await?;
            if !current{return Err(invalid());}
            // An operator may select a conversation before its first approval.
            // Establish the retained epoch under the same Office/project locks;
            // the database enforces its exact scope and finite registration cap.
            sqlx::query_scalar::<_,i64>("SELECT ortak_register_conversation_authority($1,$2,$3,$4)")
                .bind(scope.company_id()).bind(scope.community_id()).bind(project).bind(channel)
                .fetch_one(&mut *tx).await?;
        }
        sqlx::query("UPDATE reviewed_memory_targets SET enabled=false,updated_at=clock_timestamp() WHERE company_id=$1 AND enabled")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        let mut admitted=0;
        for (target,binding_hash) in prepared {
            let channel=selected.get(&(target.project_id,target.employee_id.clone())).copied();
            let affected=sqlx::query("INSERT INTO reviewed_memory_targets(company_id,community_id,project_id,employee_id,deployment_id,binding,creation_receipt,binding_hash,employee_revision_id,employee_lifecycle_epoch,enabled,valid_until,runtime_consumption_enabled,conversation_channel_id,conversation_consumption_enabled)
                SELECT e.company_id,b.community_id,p.id,e.id,$4,$5,$6,$7,e.active_revision_id,e.lifecycle_epoch,true,clock_timestamp()+make_interval(secs=>$8),$9,$10,$10::uuid IS NOT NULL
                FROM employees e JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
                JOIN projects p ON p.company_id=e.company_id AND p.id=$2 JOIN project_api_bindings b ON b.company_id=p.company_id AND b.project_id=p.id
                WHERE e.company_id=$1 AND e.id=$3 AND e.status='active' AND p.status='active' AND r.manifest->'memory'=$5
                ON CONFLICT(company_id,project_id,employee_id,deployment_id,binding_hash) DO UPDATE SET enabled=true,
                    employee_revision_id=EXCLUDED.employee_revision_id,employee_lifecycle_epoch=EXCLUDED.employee_lifecycle_epoch,
                    runtime_consumption_enabled=EXCLUDED.runtime_consumption_enabled,
                    conversation_channel_id=COALESCE(reviewed_memory_targets.conversation_channel_id,EXCLUDED.conversation_channel_id),
                    conversation_consumption_enabled=EXCLUDED.conversation_consumption_enabled,
                    valid_until=EXCLUDED.valid_until,updated_at=clock_timestamp()
                WHERE reviewed_memory_targets.binding=EXCLUDED.binding AND reviewed_memory_targets.creation_receipt=EXCLUDED.creation_receipt
                    AND (reviewed_memory_targets.conversation_channel_id IS NULL OR EXCLUDED.conversation_channel_id IS NULL
                        OR reviewed_memory_targets.conversation_channel_id=EXCLUDED.conversation_channel_id)")
                .bind(scope.company_id()).bind(target.project_id).bind(target.employee_id.as_str()).bind(target.deployment_id)
                .bind(json!(target.binding)).bind(&target.creation_receipt).bind(binding_hash).bind(target.valid_for.as_secs_f64().min(55.0))
                .bind(target.runtime_consumption_enabled).bind(channel)
                .execute(&mut *tx).await?.rows_affected();
            admitted+=affected as usize;
        }
        // Retiring the complete recipe stops runtime use durably. Refreshing an
        // unchanged target does not advance its consumption epoch.
        sqlx::query("UPDATE reviewed_memory_targets SET runtime_consumption_enabled=false,conversation_consumption_enabled=false,updated_at=clock_timestamp()
            WHERE company_id=$1 AND NOT enabled AND (runtime_consumption_enabled OR conversation_consumption_enabled)")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        if let Some(deadline)=witness.valid_before() {
            let live:bool=sqlx::query_scalar("SELECT clock_timestamp()<$1").bind(deadline).fetch_one(&mut *tx).await?;
            if !live{return Err(WorkError::OperationTimedOut);}
        }
        tx.commit().await?;Ok(admitted)
    }).await
}
