//! Durable selected input registry, frozen Work uses and bounded tool journal.
//! Callers supply adapter-verified values, never an HTTP-provided I/O witness.

use super::{invalid, work};
use crate::{DispatchAuthorization, Result};
use ortak_control::workspace::{
    PreparedWorkspace, WorkspaceGrant, WorkspaceResult, WorkspaceToolRequest,
};
use ortak_control::{CompanyScope, PgControlPlane};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

mod actions;
mod admission;
mod readers;
mod settlement;
pub use settlement::settle_workspace_receipts;
mod registry;
pub(crate) use actions::{
    acknowledge, claim, delivery_current, interrupt_run, next_run, record, retry,
};
pub(crate) use admission::{freeze, preflight};
pub use readers::{confirm_reader_absence, unresolved_reader, UnresolvedReader};
pub(crate) use readers::{plan_reader, reader_stopped};
pub(crate) use registry::register;
pub use registry::revoke;

#[derive(Clone, Debug)]
pub(crate) struct ActionLease {
    pub run_id: Uuid,
    pub request: WorkspaceToolRequest,
    pub token: Uuid,
    pub attempt: i32,
}
pub(crate) struct ClaimedAction {
    pub lease: ActionLease,
    pub prepared: PreparedWorkspace,
    pub result: Option<WorkspaceResult>,
}
pub(crate) struct SelectedRun {
    pub run_id: Uuid,
    pub grant: WorkspaceGrant,
    pub request: Option<WorkspaceToolRequest>,
}

fn decode_grant(bytes: Vec<u8>) -> Result<WorkspaceGrant> {
    if bytes.len() > 16384 {
        return Err(invalid("workspace grant exceeds ceiling".into()));
    }
    let grant: WorkspaceGrant = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("invalid retained workspace grant".into()))?;
    grant.validate()?;
    Ok(grant)
}

pub(crate) async fn current_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
) -> Result<bool> {
    let witness = ortak_control::postgres::lock_office_authority_on(connection, scope).await?;
    let outbox: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='work_run_dispatch'",
    )
    .bind(scope.company_id())
    .bind(run)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(outbox) = outbox else {
        return Ok(false);
    };
    let authority =
        match work::derive_on(connection, scope, run, outbox, Uuid::nil(), witness).await? {
            DispatchAuthorization::Authorized(authority) => authority,
            _ => return Ok(false),
        };
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(scope.company_id())
            .bind(run)
            .fetch_optional(&mut *connection)
            .await?;
    let current: bool = sqlx::query_scalar(
        "SELECT coalesce(ortak_run_workspace_current($1,$2),false)
        AND NOT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)
        AND NOT EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(scope.company_id())
    .bind(run)
    .fetch_one(&mut *connection)
    .await?;
    if !current || !matches!(status.as_deref(), Some("running" | "waiting")) {
        return Ok(false);
    }
    work::renew(connection, scope, &authority, run, None).await?;
    Ok(true)
}

async fn selected_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
) -> Result<(WorkspaceGrant, PreparedWorkspace)> {
    let row = sqlx::query(
        "SELECT b.grant_bytes,u.manifest_hash,u.store_ref FROM run_workspace_uses u
        JOIN workspace_bindings b ON b.company_id=u.company_id AND b.id=u.workspace_id
        WHERE u.company_id=$1 AND u.run_id=$2",
    )
    .bind(scope.company_id())
    .bind(run)
    .fetch_one(connection)
    .await?;
    let grant = decode_grant(row.try_get("grant_bytes")?)?;
    let prepared = PreparedWorkspace {
        run_id: run,
        manifest_hash: hex::encode(row.try_get::<Vec<u8>, _>("manifest_hash")?),
        store_ref: row.try_get("store_ref")?,
    };
    Ok((grant, prepared))
}
