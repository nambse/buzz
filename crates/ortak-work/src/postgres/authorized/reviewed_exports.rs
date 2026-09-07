//! Human publication and same-job retry reuse the current Work review boundary.
use super::*;
use crate::reviewed_exports::{self as exports, ReviewedExportAction, ReviewedExportView};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
enum ExportAudience {
    Project,
    Conversation,
}

impl ExportAudience {
    fn conversation(self) -> bool {
        matches!(self, Self::Conversation)
    }
    fn hash(self, mut value: Value) -> Result<Vec<u8>> {
        if self.conversation() {
            value
                .as_object_mut()
                .ok_or_else(exports::invalid)?
                .insert("audience_kind".into(), json!("conversation"));
        }
        exports::hash(&value)
    }
}

async fn current(c: &mut PgConnection, company: Uuid, fact: Uuid) -> Result<ReviewedExportView> {
    let value: Option<Value> = sqlx::query_scalar("SELECT ortak_reviewed_export_view($1,$2)")
        .bind(company)
        .bind(fact)
        .fetch_one(c)
        .await?;
    serde_json::from_value(value.ok_or_else(exports::invalid)?).map_err(|_| exports::invalid())
}
impl AuthorizedWork {
    async fn export_fact_on(
        &self,
        c: &mut PgConnection,
        project: Uuid,
        fact: Uuid,
        channel: Uuid,
        audience: ExportAudience,
    ) -> Result<(ReviewedFact, Option<DateTime<Utc>>)> {
        match audience {
            ExportAudience::Project => Ok((self.fact_on(c, project, fact, channel).await?, None)),
            ExportAudience::Conversation => {
                let (record, deadline) = self.conversation_fact_on(c, project, fact).await?;
                Ok((record.fact, deadline))
            }
        }
    }
    async fn export_replay_on(
        &self,
        c: &mut PgConnection,
        operation: Uuid,
        fact: Uuid,
        action: &str,
        hash: &[u8],
    ) -> Result<bool> {
        if operation.is_nil() {
            return Err(exports::invalid());
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-reviewed-export:{}:{}:{operation}",
                self.scope.company_id(),
                self.principal.public_key
            ))
            .execute(&mut *c)
            .await?;
        let row=sqlx::query("SELECT fact_id,action,request_hash FROM reviewed_memory_export_commands WHERE company_id=$1 AND actor_pubkey=$2 AND operation_id=$3")
            .bind(self.scope.company_id()).bind(&self.principal.public_key).bind(operation).fetch_optional(c).await?;
        if let Some(row) = row {
            if row.try_get::<Uuid, _>("fact_id")? != fact
                || row.try_get::<String, _>("action")? != action
                || row.try_get::<Vec<u8>, _>("request_hash")? != hash
            {
                return Err(WorkError::OperationConflict);
            }
            return Ok(true);
        }
        Ok(false)
    }
    // Keep each persisted command/provenance field explicit at this SQL seam.
    #[allow(clippy::too_many_arguments)]
    async fn export_command_on(
        &self,
        c: &mut PgConnection,
        operation: Uuid,
        fact: Uuid,
        action: &str,
        hash: &[u8],
        version: i32,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO reviewed_memory_export_commands(company_id,community_id,actor_pubkey,operation_id,fact_id,action,request_hash,result_version,auth_event_id,valid_before)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(self.scope.company_id()).bind(self.principal.community_id).bind(&self.principal.public_key).bind(operation).bind(fact).bind(action)
            .bind(hash).bind(version).bind(self.principal.auth_event_id.as_slice()).bind(deadline).execute(c).await?;
        Ok(())
    }
    /// Explicitly publish one previously preview-only fact. The target comes from
    /// the current worker's validated finite advertisement, never the request body.
    /// The instruction and both publication/expiry-cleanup jobs commit together.
    pub async fn publish_reviewed_fact(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        expected_version: i64,
        confirmed: bool,
    ) -> Result<ReviewedExportView> {
        self.publish_fact_scoped(
            operation,
            project,
            fact,
            expected_version,
            confirmed,
            ExportAudience::Project,
        )
        .await
    }

    /// Publish one explicitly confirmed conversation fact to its original owned
    /// project target. The server rechecks the reviewed audience and source.
    pub async fn publish_reviewed_conversation_fact(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        expected_version: i64,
        confirmed: bool,
    ) -> Result<ReviewedExportView> {
        self.publish_fact_scoped(
            operation,
            project,
            fact,
            expected_version,
            confirmed,
            ExportAudience::Conversation,
        )
        .await
    }

    async fn publish_fact_scoped(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        expected_version: i64,
        confirmed: bool,
        audience: ExportAudience,
    ) -> Result<ReviewedExportView> {
        bounded(async {
            if !confirmed||expected_version!=1{return Err(exports::invalid());}
            let hash=audience.hash(json!({"project_id":project,"fact_id":fact,"expected_version":expected_version,"confirmed":confirmed}))?;
            let (mut tx,mut deadline)=self.begin().await?;
            let replay=self.export_replay_on(&mut tx,operation,fact,"publish",&hash).await?;
            let project=self.project_on(&mut tx,project).await?;self.review(project.role)?;
            if audience.conversation() {
                sqlx::query("SELECT epoch FROM conversation_memory_authorities WHERE company_id=$1 AND project_id=$2 AND channel_id=$3 FOR SHARE")
                    .bind(self.scope.company_id()).bind(project.record.project.id).bind(project.channel_id)
                    .fetch_optional(&mut *tx).await?.ok_or(WorkError::AccessDenied)?;
            }
            sqlx::query("SELECT id FROM reviewed_memory_facts WHERE company_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE")
                .bind(self.scope.company_id()).bind(project.record.project.id).bind(fact).fetch_optional(&mut *tx).await?.ok_or(WorkError::AccessDenied)?;
            let (reviewed,source_deadline)=self.export_fact_on(&mut tx,project.record.project.id,fact,project.channel_id,audience).await?;
            deadline=match (deadline,source_deadline) {(Some(a),Some(b))=>Some(a.min(b)),(a,b)=>a.or(b)};
            if replay {let value=current(&mut tx,self.scope.company_id(),fact).await?;self.finish(tx,deadline).await?;return Ok(value);}
            if reviewed.version!=1||reviewed.status!="active"||!reviewed.source_visible{return Err(WorkError::AccessDenied);}
            self.employee_on(&mut tx,project.channel_id,&reviewed.employee_id).await?;
            let existing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reviewed_memory_exports WHERE company_id=$1 AND fact_id=$2)")
                .bind(self.scope.company_id()).bind(fact).fetch_one(&mut *tx).await?;
            if existing{return Err(WorkError::OperationConflict);}
            let targets=sqlx::query("SELECT t.id,t.binding,t.employee_revision_id,t.employee_lifecycle_epoch FROM reviewed_memory_targets t
                WHERE t.company_id=$1 AND t.project_id=$2 AND t.employee_id=$3
                AND CASE WHEN $5::boolean THEN ortak_conversation_export_eligible($1,$4,t.id) ELSE ortak_reviewed_export_eligible($1,$4,t.id) END
                ORDER BY t.id LIMIT 2 FOR SHARE OF t")
                .bind(self.scope.company_id()).bind(reviewed.project_id).bind(reviewed.employee_id.as_str()).bind(fact).bind(audience.conversation()).fetch_all(&mut *tx).await?;
            if targets.len()!=1{return Err(WorkError::InvalidQuery("no unique current reviewed-memory publication target"));}
            let target=&targets[0];let target_id:Uuid=target.try_get("id")?;
            let binding:ortak_domain::MemoryBinding=serde_json::from_value(target.try_get("binding")?).map_err(|_|exports::invalid())?;
            let provenance=sqlx::query("SELECT promotion_operation_id,ortak_reviewed_export_source_hash(f) AS source_hash FROM reviewed_memory_facts f WHERE company_id=$1 AND id=$2")
                .bind(self.scope.company_id()).bind(fact).fetch_one(&mut *tx).await?;
            let source_hash:Vec<u8>=provenance.try_get("source_hash")?;
            let approval:Uuid=provenance.try_get("promotion_operation_id")?;
            let content=reviewed.content.as_deref().ok_or(WorkError::AccessDenied)?;
            use sha2::{Digest,Sha256};
            sqlx::query("INSERT INTO reviewed_memory_exports(company_id,community_id,fact_id,project_id,employee_id,target_id,employee_revision_id,
                employee_lifecycle_epoch,content_hash,source_hash,requested_by,operation_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
                .bind(self.scope.company_id()).bind(self.principal.community_id).bind(fact).bind(reviewed.project_id).bind(reviewed.employee_id.as_str())
                .bind(target_id).bind(target.try_get::<Uuid,_>("employee_revision_id")?).bind(target.try_get::<i64,_>("employee_lifecycle_epoch")?)
                .bind(Sha256::digest(content.as_bytes()).as_slice()).bind(&source_hash).bind(&self.principal.public_key).bind(operation).execute(&mut *tx).await?;
            for action in [ReviewedExportAction::Publish,ReviewedExportAction::Withdraw] {
                let request_hash=exports::request_hash(self.scope.company_id(),reviewed.project_id,fact,&reviewed.employee_id,&binding,action,content,
                    &source_hash,&reviewed.approved_by,approval,reviewed.expires_at)?;
                sqlx::query("INSERT INTO reviewed_memory_export_jobs(company_id,community_id,fact_id,action,idempotency_key,request_hash,next_attempt_at)
                    VALUES($1,$2,$3,$4,$5,$6,CASE WHEN $4='withdraw' THEN $7 ELSE clock_timestamp() END)")
                    .bind(self.scope.company_id()).bind(self.principal.community_id).bind(fact).bind(action.as_str()).bind(exports::operation_key(fact,action))
                    .bind(request_hash).bind(reviewed.expires_at).execute(&mut *tx).await?;
            }
            self.export_command_on(&mut tx,operation,fact,"publish",&hash,0,deadline).await?;
            let value=current(&mut tx,self.scope.company_id(),fact).await?;self.finish(tx,deadline).await?;Ok(value)
        }).await
    }

    /// Reopens one failed job with the original remote key and payload. Cleanup
    /// remains recoverable when source visibility or employee Active status is lost.
    pub async fn retry_reviewed_export(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        action: ReviewedExportAction,
        version: i32,
    ) -> Result<ReviewedExportView> {
        self.retry_export_scoped(
            operation,
            project,
            fact,
            action,
            version,
            ExportAudience::Project,
        )
        .await
    }

    /// Retry the same retained conversation publication or withdrawal job.
    /// Withdrawal recovery does not require the old source to remain visible.
    pub async fn retry_reviewed_conversation_export(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        action: ReviewedExportAction,
        version: i32,
    ) -> Result<ReviewedExportView> {
        self.retry_export_scoped(
            operation,
            project,
            fact,
            action,
            version,
            ExportAudience::Conversation,
        )
        .await
    }

    async fn retry_export_scoped(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        action: ReviewedExportAction,
        version: i32,
        audience: ExportAudience,
    ) -> Result<ReviewedExportView> {
        bounded(async {
            if !(0..8).contains(&version){return Err(exports::invalid());}
            let operation_action=format!("retry_{}",action.as_str());
            let hash=audience.hash(json!({"project_id":project,"fact_id":fact,"action":action,"retry_version":version}))?;
            let (mut tx,mut deadline)=self.begin().await?;
            let replay=self.export_replay_on(&mut tx,operation,fact,&operation_action,&hash).await?;
            let project=self.project_on(&mut tx,project).await?;self.review(project.role)?;
            let (reviewed,source_deadline)=self.export_fact_on(&mut tx,project.record.project.id,fact,project.channel_id,audience).await?;
            deadline=match (deadline,source_deadline) {(Some(a),Some(b))=>Some(a.min(b)),(a,b)=>a.or(b)};
            if !replay {
                if action==ReviewedExportAction::Publish {
                    self.employee_on(&mut tx,project.channel_id,&reviewed.employee_id).await?;
                    let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reviewed_memory_exports x JOIN employees e ON e.company_id=x.company_id AND e.id=x.employee_id
                        WHERE x.company_id=$1 AND x.fact_id=$2 AND x.employee_revision_id=e.active_revision_id AND x.employee_lifecycle_epoch=e.lifecycle_epoch
                            AND CASE WHEN $3::boolean THEN ortak_conversation_export_eligible(x.company_id,x.fact_id,x.target_id)
                                ELSE ortak_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id) END)")
                        .bind(self.scope.company_id()).bind(fact).bind(audience.conversation()).fetch_one(&mut *tx).await?;
                    if !live{return Err(WorkError::AccessDenied);}
                }
                let affected=sqlx::query("UPDATE reviewed_memory_export_jobs SET state='pending',attempt_count=0,retry_version=retry_version+1,
                    next_attempt_at=clock_timestamp(),last_error_code=NULL,updated_at=clock_timestamp()
                    WHERE company_id=$1 AND fact_id=$2 AND action=$3 AND state='failed' AND retry_version=$4 AND lease_token IS NULL")
                    .bind(self.scope.company_id()).bind(fact).bind(action.as_str()).bind(version).execute(&mut *tx).await?.rows_affected();
                if affected!=1{return Err(WorkError::OperationConflict);}
                self.export_command_on(&mut tx,operation,fact,&operation_action,&hash,version+1,deadline).await?;
            }
            let value=current(&mut tx,self.scope.company_id(),fact).await?;self.finish(tx,deadline).await?;Ok(value)
        }).await
    }
}
