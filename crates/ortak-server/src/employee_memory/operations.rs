use super::*;

mod approval;

struct Replay {
    fact: Uuid,
    version: i32,
}
impl Review<'_> {
    async fn operation_on(
        &self,
        connection: &mut PgConnection,
        operation: Uuid,
        action: &str,
        bytes: &[u8],
    ) -> Result<Option<Replay>> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-employee-memory-operation:{}:{}:{operation}",
                self.scope().company_id(),
                self.principal.public_key.to_hex()
            ))
            .execute(&mut *connection)
            .await?;
        let row=sqlx::query("SELECT o.fact_id,o.action,o.submitted_bytes,o.submitted_hash,o.result_version,f.employee_id
            FROM employee_reviewed_memory_operations o JOIN employee_reviewed_memory_facts f
                ON f.company_id=o.company_id AND f.community_id=o.community_id AND f.id=o.fact_id
            WHERE o.company_id=$1 AND o.actor_public_key=$2 AND o.operation_id=$3")
            .bind(self.scope().company_id()).bind(self.actor().as_slice()).bind(operation).fetch_optional(connection).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.try_get::<String, _>("employee_id")? != self.employee.as_str() {
            return Err(forbidden());
        }
        if row.try_get::<String, _>("action")? != action
            || row.try_get::<Vec<u8>, _>("submitted_bytes")? != bytes
            || row.try_get::<Vec<u8>, _>("submitted_hash")?.as_slice()
                != digest(bytes).as_bytes().as_slice()
        {
            return Err(conflict());
        }
        Ok(Some(Replay {
            fact: row.try_get("fact_id")?,
            version: row.try_get("result_version")?,
        }))
    }
    // Keep each persisted command/provenance field explicit at this SQL seam.
    #[allow(clippy::too_many_arguments)]
    async fn record_on(
        &self,
        connection: &mut PgConnection,
        operation: Uuid,
        fact: Uuid,
        action: &str,
        bytes: &[u8],
        version: i32,
        before: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO employee_reviewed_memory_operations
            (company_id,community_id,actor_public_key,operation_id,fact_id,action,submitted_bytes,submitted_hash,result_version,auth_event_id,valid_before)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
            .bind(self.scope().company_id()).bind(self.state.config.community_id).bind(self.actor().as_slice())
            .bind(operation).bind(fact).bind(action).bind(bytes).bind(digest(bytes).as_bytes().as_slice()).bind(version)
            .bind(&self.principal.auth_event_id).bind(before).execute(connection).await?;
        Ok(())
    }
    async fn receipt_on(
        &self,
        connection: &mut PgConnection,
        operation: Uuid,
        fact: Uuid,
        action: &str,
        version: i32,
        created: bool,
    ) -> Result<(Value, Option<DateTime<Utc>>)> {
        // Replayed approval remains effect version1 even after later Stop; the
        // separately projected current fact is version2. Never rewrite history.
        let fact_row = self.fact_on(connection, fact).await?;
        let (view, before) = self.project_fact(connection, &fact_row).await?;
        Ok((
            json!({"operation_id":operation,"created":created,
            "effect":{"fact_id":fact,"action":action,"result_version":version},"fact":view}),
            before,
        ))
    }
    pub(super) async fn stop(&self, fact: Uuid, request: Stop) -> Result<Value> {
        nonnil(fact)?;
        nonnil(request.operation_id)?;
        if request.expected_version != 1 {
            return Err(ApiError::invalid());
        }
        let bytes = wire::stop(request.operation_id, fact, request.expected_version)?;
        let (mut tx, mut deadline) = self.begin().await?;
        if let Some(replay) = self
            .operation_on(&mut tx, request.operation_id, "stop", &bytes)
            .await?
        {
            if replay.fact != fact {
                return Err(conflict());
            }
            let (value, before) = self
                .receipt_on(
                    &mut tx,
                    request.operation_id,
                    fact,
                    "stop",
                    replay.version,
                    false,
                )
                .await?;
            if let Some(before) = before {
                deadline = deadline.min(before);
            }
            self.finish(tx, deadline).await?;
            return Ok(value);
        }
        let row = self.fact_on(&mut tx, fact).await?;
        self.lock_scopes(&mut tx, row.source_channel_id, row.destination_channel_id)
            .await?;
        let changed=sqlx::query("UPDATE employee_reviewed_memory_facts SET version=2,revoked_at=clock_timestamp(),revoked_by=$4
            WHERE company_id=$1 AND employee_id=$2 AND id=$3 AND approved_by=$4 AND version=1")
            .bind(self.scope().company_id()).bind(self.employee.as_str()).bind(fact).bind(self.actor().as_slice())
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(conflict());
        }
        self.record_on(
            &mut tx,
            request.operation_id,
            fact,
            "stop",
            &bytes,
            2,
            deadline,
        )
        .await?;
        let (value, before) = self
            .receipt_on(&mut tx, request.operation_id, fact, "stop", 2, true)
            .await?;
        if let Some(before) = before {
            deadline = deadline.min(before);
        }
        self.finish(tx, deadline).await?;
        Ok(value)
    }
}
