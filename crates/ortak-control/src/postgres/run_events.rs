use sqlx::Row;
use uuid::Uuid;

use super::PgControlPlane;
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::ports::{RunEventAppend, RunEventRepository};
use crate::run_event::RunEvent;

impl RunEventRepository for PgControlPlane {
    async fn append_run_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        events: &[RunEvent],
    ) -> Result<RunEventAppend> {
        for event in events {
            event.validate()?;
            if event.run_id != run_id {
                return Err(ControlError::RunEventRejected {
                    run_id,
                    detail: "event belongs to a different run",
                });
            }
        }

        let mut tx = self.pool.begin().await?;
        let run =
            sqlx::query("SELECT status FROM runs WHERE company_id = $1 AND id = $2 FOR UPDATE")
                .bind(scope.company_id())
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(run) = run else {
            return Err(ControlError::RunEventRejected {
                run_id,
                detail: "unknown run",
            });
        };
        let status: String = run.try_get("status")?;
        if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
            return Err(ControlError::RunEventRejected {
                run_id,
                detail: "run is terminal",
            });
        }

        let mut next: i64 = sqlx::query(
            "SELECT coalesce(max(sequence) + 1, 0) AS next
               FROM run_events WHERE company_id = $1 AND run_id = $2",
        )
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("next")?;

        let mut sequences = Vec::with_capacity(events.len());
        let mut duplicate_cursors = Vec::new();
        for event in events {
            if let Some(cursor) = &event.runtime_cursor {
                let seen = sqlx::query(
                    "SELECT 1 FROM run_events
                      WHERE company_id = $1 AND run_id = $2 AND runtime_cursor = $3",
                )
                .bind(scope.company_id())
                .bind(run_id)
                .bind(cursor)
                .fetch_optional(&mut *tx)
                .await?;
                if seen.is_some() {
                    duplicate_cursors.push(cursor.clone());
                    continue;
                }
            }
            sqlx::query(
                "INSERT INTO run_events
                     (company_id, run_id, sequence, event_type, occurred_at,
                      runtime_cursor, payload, artifact_ref)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(scope.company_id())
            .bind(run_id)
            .bind(next)
            .bind(event.event_type().as_str())
            .bind(event.occurred_at)
            .bind(event.runtime_cursor.as_deref())
            .bind(event.payload_json()?)
            .bind(event.artifact_ref.as_deref())
            .execute(&mut *tx)
            .await?;
            sequences.push(next);
            next += 1;
        }
        tx.commit().await?;
        Ok(RunEventAppend {
            sequences,
            duplicate_cursors,
        })
    }

    async fn run_events_after(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        after: i64,
        limit: i64,
    ) -> Result<Vec<RunEvent>> {
        let rows = sqlx::query(
            "SELECT sequence, occurred_at, runtime_cursor, payload, artifact_ref
               FROM run_events
              WHERE company_id = $1 AND run_id = $2 AND sequence > $3
              ORDER BY sequence
              LIMIT $4",
        )
        .bind(scope.company_id())
        .bind(run_id)
        .bind(after)
        .bind(limit.clamp(1, 1_000))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let payload: serde_json::Value = row.try_get("payload")?;
                Ok(RunEvent {
                    run_id,
                    sequence: Some(row.try_get("sequence")?),
                    occurred_at: row.try_get("occurred_at")?,
                    runtime_cursor: row.try_get("runtime_cursor")?,
                    artifact_ref: row.try_get("artifact_ref")?,
                    payload: serde_json::from_value(payload)?,
                })
            })
            .collect()
    }
}
