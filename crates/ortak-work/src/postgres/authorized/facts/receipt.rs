use super::*;
use chrono::{DateTime, Utc};

impl AuthorizedWork {
    pub(super) async fn fact_operation_on(
        &self,
        c: &mut PgConnection,
        id: Uuid,
        action: &str,
        hash: &[u8],
    ) -> Result<Option<Uuid>> {
        if id.is_nil() {
            return Err(WorkError::InvalidQuery("operation id must not be nil"));
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-reviewed-memory:{}:{}:{id}",
                self.scope.company_id(),
                self.principal.public_key
            ))
            .execute(&mut *c)
            .await?;
        let row = sqlx::query(
            "SELECT action,request_hash,fact_id FROM reviewed_memory_operations
            WHERE company_id=$1 AND actor_pubkey=$2 AND operation_id=$3",
        )
        .bind(self.scope.company_id())
        .bind(&self.principal.public_key)
        .bind(id)
        .fetch_optional(c)
        .await?;
        row.map(|row| {
            if row.try_get::<String, _>("action")? != action
                || row.try_get::<Vec<u8>, _>("request_hash")? != hash
            {
                return Err(WorkError::OperationConflict);
            }
            Ok(row.try_get("fact_id")?)
        })
        .transpose()
    }

    pub(super) async fn record_fact_operation_on(
        &self,
        c: &mut PgConnection,
        op: Uuid,
        action: &str,
        hash: &[u8],
        fact: &ReviewedFact,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO reviewed_memory_operations(company_id,actor_pubkey,operation_id,action,request_hash,
            fact_id,project_id,result_version,auth_event_id,valid_before,community_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
            .bind(self.scope.company_id()).bind(&self.principal.public_key).bind(op).bind(action).bind(hash)
            .bind(fact.id).bind(fact.project_id).bind(fact.version).bind(self.principal.auth_event_id.as_slice())
            .bind(deadline).bind(self.principal.community_id).execute(c).await?;
        Ok(())
    }
}
