//! Shared canonical authority derivation for dispatch and active-run review.
use super::invalid;
use crate::authority::{
    bound_message_text, validate_pinned_revision, DispatchAuthority, DispatchRefusal, RunInput,
    StoredMemoryBinding, StoredRuntimeBinding,
};
use crate::error::{Result, RunSupervisionError};
use crate::repository::DispatchAuthorization;
use chrono::Utc;
use ortak_control::inbox::is_supported_channel_kind;
use ortak_control::office_authority::OfficeAuthority;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::ports::Normalization;
use ortak_control::postgres::lock_office_authority_on;
use ortak_control::service::office_input_hash;
use ortak_control::{CompanyScope, MessageId, PgControlPlane};
use ortak_domain::{EmployeeId, EmployeeStatus};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

const AUTHORITY_SQL: &str = "SELECT o.kind, o.state, o.dedup_key, o.lease_token, o.run_id,
            (o.lease_expires_at > clock_timestamp()) AS lease_live,
            c.status AS company_status,
            o.routing_decision_id, o.employee_id,
            d.id AS decision_id, d.message_id, d.root_message_id, d.office_input_hash,
            d.office_authority_generation,
            rr.action AS recipient_action, rr.employee_revision_id AS pinned_revision_id,
            v.batch_hop,
            e.status AS employee_status,
            rr.employee_lifecycle_epoch=e.lifecycle_epoch AS current_lifecycle,
            rev.id AS revision_id, rev.manifest, active_rev.manifest AS active_manifest,
            rb.adapter AS binding_adapter, rb.profile_ref AS binding_profile_ref,
            rb.model AS binding_model, rb.workspace_ref AS binding_workspace_ref,
            rb.credential_refs AS binding_credential_refs, rb.options AS binding_options,
            rb.validated_at AS binding_validated_at,
            mb.adapter AS memory_adapter, mb.endpoint_ref AS memory_endpoint_ref,
            mb.workspace AS memory_workspace, mb.user_peer AS memory_user_peer,
            mb.employee_peer AS memory_employee_peer, mb.options AS memory_options,
            mb.validated_at AS memory_validated_at,
            amb.adapter AS active_memory_adapter, amb.endpoint_ref AS active_memory_endpoint_ref,
            amb.workspace AS active_memory_workspace, amb.user_peer AS active_memory_user_peer,
            amb.employee_peer AS active_memory_employee_peer, amb.options AS active_memory_options,
            amb.validated_at AS active_memory_validated_at,
            i.state AS inbox_state, i.event_kind, i.channel_id, i.event_created_at, i.author_pubkey,
            ev.kind AS message_kind, ev.channel_id AS message_channel_id,
            ev.content AS message_content, ev.deleted_at AS message_deleted_at
       FROM outbox o
       JOIN companies c ON c.id = o.company_id
       LEFT JOIN routing_decisions d
         ON d.company_id = o.company_id AND d.id = o.routing_decision_id
       LEFT JOIN routing_recipients rr
         ON rr.company_id = o.company_id
        AND rr.routing_decision_id = o.routing_decision_id
        AND rr.employee_id = o.employee_id
       LEFT JOIN delivery_chain_visits v
         ON v.company_id = o.company_id
        AND v.root_message_id = d.root_message_id
        AND v.employee_id = o.employee_id
        AND v.routing_decision_id = d.id
       LEFT JOIN employees e
         ON e.company_id = o.company_id AND e.id = o.employee_id
       LEFT JOIN employee_revisions rev
         ON rev.company_id = o.company_id
        AND rev.employee_id = o.employee_id
        AND rev.id = rr.employee_revision_id
       LEFT JOIN employee_revisions active_rev
         ON active_rev.company_id = e.company_id
        AND active_rev.employee_id = e.id
        AND active_rev.id = e.active_revision_id
       LEFT JOIN employee_runtime_bindings rb
         ON rb.company_id = o.company_id
        AND rb.employee_id = o.employee_id
        AND rb.revision_id = rev.id
       LEFT JOIN employee_memory_bindings mb
         ON mb.company_id = o.company_id AND mb.employee_id = o.employee_id
        AND mb.revision_id = rev.id
       LEFT JOIN employee_memory_bindings amb
         ON amb.company_id = e.company_id AND amb.employee_id = e.id
        AND amb.revision_id = e.active_revision_id
       LEFT JOIN office_inbox i
         ON i.company_id = o.company_id AND i.event_id = d.message_id
       LEFT JOIN office_company_bindings ocb
         ON ocb.company_id = o.company_id
       LEFT JOIN events ev
         ON ev.community_id = ocb.community_id
        AND ev.created_at = i.event_created_at
        AND ev.id = d.message_id
      WHERE o.company_id = $1 AND o.id = $2";

pub(super) fn parse_employee_status(value: &str) -> Result<EmployeeStatus> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| invalid(format!("employees.status holds {value:?}")))
}

pub(super) fn stored_binding(row: &PgRow) -> Result<Option<StoredRuntimeBinding>> {
    let adapter: Option<String> = row.try_get("binding_adapter")?;
    let Some(adapter) = adapter else {
        return Ok(None);
    };
    let credential_refs: serde_json::Value = row.try_get("binding_credential_refs")?;
    let options: serde_json::Value = row.try_get("binding_options")?;
    let validated_at: Option<chrono::DateTime<Utc>> = row.try_get("binding_validated_at")?;
    Ok(Some(StoredRuntimeBinding {
        adapter,
        profile_ref: row.try_get("binding_profile_ref")?,
        model: row.try_get("binding_model")?,
        workspace_ref: row.try_get("binding_workspace_ref")?,
        credential_refs: serde_json::from_value::<Vec<String>>(credential_refs)
            .map_err(|_| invalid("employee_runtime_bindings.credential_refs".to_owned()))?,
        options: serde_json::from_value::<BTreeMap<String, String>>(options)
            .map_err(|_| invalid("employee_runtime_bindings.options".to_owned()))?,
        validated: validated_at.is_some(),
    }))
}

pub(super) fn stored_memory(row: &PgRow, prefix: &str) -> Result<Option<StoredMemoryBinding>> {
    let Some(adapter) = row.try_get::<Option<String>, _>((format!("{prefix}_adapter")).as_str())?
    else {
        return Ok(None);
    };
    let options: serde_json::Value = row.try_get((format!("{prefix}_options")).as_str())?;
    Ok(Some(StoredMemoryBinding {
        binding: ortak_domain::MemoryBinding {
            adapter,
            endpoint_ref: row.try_get((format!("{prefix}_endpoint_ref")).as_str())?,
            workspace: row.try_get((format!("{prefix}_workspace")).as_str())?,
            user_peer: row.try_get((format!("{prefix}_user_peer")).as_str())?,
            employee_peer: row.try_get((format!("{prefix}_employee_peer")).as_str())?,
            options: serde_json::from_value(options)
                .map_err(|_| invalid("employee_memory_bindings.options".to_owned()))?,
        },
        validated: row
            .try_get::<Option<chrono::DateTime<Utc>>, _>(
                (format!("{prefix}_validated_at")).as_str(),
            )?
            .is_some(),
    }))
}

pub(super) async fn authorize(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutboxLease,
) -> Result<DispatchAuthorization> {
    let mut tx = control.pool().begin().await?;
    let office_authority = lock_office_authority_on(&mut tx, scope).await?;
    authorize_on(&mut tx, scope, lease, office_authority).await
}

/// Derives canonical dispatch facts under the caller's shared mutation fence.
/// Call before any row locks; the witness must originate in this transaction.
pub(crate) async fn authorize_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    lease: &OutboxLease,
    office_authority: OfficeAuthority,
) -> Result<DispatchAuthorization> {
    let row = sqlx::query(AUTHORITY_SQL)
        .bind(scope.company_id())
        .bind(lease.id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Err(RunSupervisionError::UnknownOutboxRow {
            outbox_id: lease.id,
        });
    };

    // 1. The row itself: kind, then the lease's own copies of the routing
    //    hints must agree with the row before anything else is derived.
    let kind_raw: String = row.try_get("kind")?;
    let kind = OutboxKind::parse(&kind_raw)
        .ok_or_else(|| invalid(format!("outbox.kind holds {kind_raw:?}")))?;
    if kind == OutboxKind::WorkRunDispatch {
        return super::work::authorize_on(connection, scope, lease, office_authority).await;
    }
    if kind != OutboxKind::RunDispatch {
        return Err(RunSupervisionError::WrongKind { found: kind });
    }
    let row_decision: Option<Uuid> = row.try_get("routing_decision_id")?;
    let row_employee: Option<String> = row.try_get("employee_id")?;
    let row_dedup: String = row.try_get("dedup_key")?;
    if lease.routing_decision_id != row_decision
        || lease.employee_id != row_employee
        || lease.dedup_key != row_dedup
    {
        return Err(RunSupervisionError::LeaseInconsistent {
            outbox_id: lease.id,
        });
    }

    // 2. Lease fence at the database clock.
    let state: String = row.try_get("state")?;
    let lease_token: Option<Uuid> = row.try_get("lease_token")?;
    let lease_live: Option<bool> = row.try_get("lease_live")?;
    if state != "pending" || lease_token != Some(lease.lease_token) || lease_live != Some(true) {
        return Ok(DispatchAuthorization::StaleLease);
    }

    derive_on(
        connection,
        scope,
        row,
        lease.id,
        lease.lease_token,
        office_authority,
    )
    .await
}

async fn derive_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    row: PgRow,
    outbox_id: Uuid,
    lease_token: Uuid,
    office_authority: OfficeAuthority,
) -> Result<DispatchAuthorization> {
    let row_employee: Option<String> = row.try_get("employee_id")?;
    // 3. Routing provenance.
    let refused = |refusal| Ok(DispatchAuthorization::Refused(refusal));
    if let Some(run) = row.try_get::<Option<Uuid>, _>("run_id")? {
        // Initial dispatch has no uses. Retrying or renewing an admitted Office
        // run must retain the same current conversation input before run locks.
        let current: bool = sqlx::query_scalar("SELECT ortak_lock_run_reviewed_memory($1,$2)")
            .bind(scope.company_id())
            .bind(run)
            .fetch_one(&mut *connection)
            .await?;
        if !current {
            return refused(DispatchRefusal::OfficeAuthorityChanged);
        }
    }
    if row.try_get::<String, _>("company_status")? != "active" {
        return refused(DispatchRefusal::CompanyNotActive);
    }
    let Some(routing_decision_id) = row.try_get::<Option<Uuid>, _>("decision_id")? else {
        return refused(DispatchRefusal::DecisionMissing);
    };
    // Historical decisions without a routing-time witness remain inert even
    // if a canonical input hash is present. The current generation may differ:
    // fresh canonical revalidation preserves retries with pinned permissions.
    if row
        .try_get::<Option<i64>, _>("office_authority_generation")?
        .is_none()
    {
        return refused(DispatchRefusal::OfficeAuthorityChanged);
    }
    let Some(employee_id_raw) = row_employee else {
        return refused(DispatchRefusal::RecipientMissing);
    };
    let employee_id = EmployeeId::parse(employee_id_raw.as_str())
        .map_err(|error| invalid(format!("outbox.employee_id: {error}")))?;
    let message_bytes: Vec<u8> = row.try_get("message_id")?;
    let root_bytes: Vec<u8> = row.try_get("root_message_id")?;
    let message_id = MessageId::try_from_slice(&message_bytes)?;
    let root_message_id = MessageId::try_from_slice(&root_bytes)?;
    match row.try_get::<Option<String>, _>("recipient_action")? {
        None => return refused(DispatchRefusal::RecipientMissing),
        Some(action) if action != "wake" => {
            return refused(DispatchRefusal::RecipientNotWake { action })
        }
        Some(_) => {}
    }
    let Some(employee_revision_id) = row.try_get::<Option<Uuid>, _>("pinned_revision_id")? else {
        return refused(DispatchRefusal::RecipientRevisionUnpinned);
    };
    if row.try_get::<Option<i16>, _>("batch_hop")?.is_none() {
        return refused(DispatchRefusal::VisitMissing);
    }
    let inbox_state: Option<String> = row.try_get("inbox_state")?;
    if inbox_state.as_deref() != Some("decided") {
        return refused(DispatchRefusal::InboxNotDecided { state: inbox_state });
    }

    // 4. Employee lifecycle and the pinned revision's validated binding.
    let Some(employee_status) = row.try_get::<Option<String>, _>("employee_status")? else {
        return refused(DispatchRefusal::EmployeeMissing);
    };
    let employee_status = parse_employee_status(&employee_status)?;
    if row.try_get::<Option<bool>, _>("current_lifecycle")? != Some(true) {
        return refused(DispatchRefusal::EmployeeLifecycleChanged);
    }
    let Some(revision_id) = row.try_get::<Option<Uuid>, _>("revision_id")? else {
        if employee_status != EmployeeStatus::Active {
            return refused(DispatchRefusal::EmployeeNotActive {
                status: employee_status,
            });
        }
        return refused(DispatchRefusal::RevisionMissing);
    };
    if revision_id != employee_revision_id {
        return Err(invalid(
            "pinned revision join returned another revision".to_owned(),
        ));
    }
    let manifest: serde_json::Value = row.try_get("manifest")?;
    let stored = stored_binding(&row)?;
    let configuration =
        match validate_pinned_revision(&employee_id, employee_status, &manifest, stored.as_ref()) {
            Ok(configuration) => configuration,
            Err(refusal) => return refused(refusal),
        };

    // Permissions remain pinned, but an identity rotation must not admit
    // a run whose eventual Office reply would use a retired signing key.
    let pinned: ortak_domain::Employee = serde_json::from_value(manifest)
        .map_err(|_| invalid("validated pinned manifest could not be decoded".to_owned()))?;
    let active = row
        .try_get::<Option<serde_json::Value>, _>("active_manifest")?
        .and_then(|value| serde_json::from_value::<ortak_domain::Employee>(value).ok());
    if active.as_ref().is_none_or(|active| {
        !active
            .office
            .public_key
            .eq_ignore_ascii_case(&pinned.office.public_key)
            || active.office.signer_ref != pinned.office.signer_ref
    }) {
        return refused(DispatchRefusal::OfficeAuthorityChanged);
    }

    let memory = stored_memory(&row, "memory")?;
    let active_memory = stored_memory(&row, "active_memory")?;
    let configuration = match configuration.with_validated_memory(
        memory.as_ref(),
        active
            .as_ref()
            .and_then(|employee| employee.memory.as_ref()),
    ) {
        Ok(configuration) => configuration,
        Err(refusal) => return refused(refusal),
    };

    let configuration = match configuration.with_validated_memory(
        active_memory.as_ref(),
        active
            .as_ref()
            .and_then(|employee| employee.memory.as_ref()),
    ) {
        Ok(configuration) => configuration,
        Err(refusal) => return refused(refusal),
    };

    // 5. Last-mile channel-kind guard, before any content is read as
    //    text. A stale or hand-seeded dispatch for a gift wrap (1059) or
    //    any other non-channel kind is refused here even if it somehow
    //    reached a `wake` recipient row, and the inbox copy of kind and
    //    channel must agree with the canonical signed event.
    let event_kind: Option<i32> = row.try_get("event_kind")?;
    let channel_id: Option<Uuid> = row.try_get("channel_id")?;
    let Some(event_kind) = event_kind else {
        return refused(DispatchRefusal::MessageUnavailable);
    };
    if !is_supported_channel_kind(event_kind) {
        return refused(DispatchRefusal::UnsupportedMessageKind { kind: event_kind });
    }
    let Some(channel_id) = channel_id else {
        return refused(DispatchRefusal::MessageChannelMissing);
    };
    let Some(content) = row.try_get::<Option<String>, _>("message_content")? else {
        return refused(DispatchRefusal::MessageUnavailable);
    };
    if row.try_get::<Option<i32>, _>("message_kind")? != Some(event_kind) {
        return refused(DispatchRefusal::MessageProvenanceMismatch { field: "kind" });
    }
    if row.try_get::<Option<Uuid>, _>("message_channel_id")? != Some(channel_id) {
        return refused(DispatchRefusal::MessageProvenanceMismatch { field: "channel" });
    }
    if row
        .try_get::<Option<chrono::DateTime<Utc>>, _>("message_deleted_at")?
        .is_some()
    {
        return refused(DispatchRefusal::MessageDeleted);
    }
    let (body, truncated) = match bound_message_text(&content) {
        Ok(bounded) => bounded,
        Err(refusal) => return refused(refusal),
    };
    let input = RunInput {
        body,
        truncated,
        channel_id: Some(channel_id),
        event_kind,
    };

    // Re-read the same canonical normalization under the mutation fence.
    // Revision policy is intentionally excluded from this hash: the run
    // still uses the immutable configuration pinned by its recipient.
    let author: Vec<u8> = row.try_get("author_pubkey")?;
    let inbox = ortak_control::inbox::InboxEvent {
        event_id: message_id,
        event_created_at: row.try_get("event_created_at")?,
        event_kind,
        author_pubkey: author
            .as_slice()
            .try_into()
            .map_err(|_| invalid("inbox author key length".to_owned()))?,
        channel_id: Some(channel_id),
    };
    let normalized =
        match ortak_office::PgChannelNormalizer::normalize_on(connection, scope, &inbox).await? {
            Normalization::Message(normalized) => normalized,
            _ => return refused(DispatchRefusal::OfficeAuthorityChanged),
        };
    let expected_hash: Option<Vec<u8>> = row.try_get("office_input_hash")?;
    let current_hash = office_input_hash(
        &normalized.envelope,
        normalized.root_message_id,
        &normalized.eligible_employee_ids,
    );
    if expected_hash.as_deref() != Some(current_hash.as_slice())
        || !normalized.eligible_employee_ids.contains(&employee_id)
        || normalized.root_message_id != root_message_id
    {
        return refused(DispatchRefusal::OfficeAuthorityChanged);
    }

    Ok(DispatchAuthorization::Authorized(Box::new(
        DispatchAuthority::new(
            scope.company_id(),
            outbox_id,
            lease_token,
            routing_decision_id,
            employee_id,
            employee_revision_id,
            message_id,
            root_message_id,
            configuration,
            input,
        )
        .with_office_authority(office_authority),
    )))
}

/// Revalidates active Office facts while retaining the immutable run configuration.
pub(super) async fn refresh_admission(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<bool> {
    let mut tx = control.pool().begin().await?;
    let witness = lock_office_authority_on(&mut tx, scope).await?;
    let outbox_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM outbox WHERE company_id = $1 AND run_id = $2 AND kind = 'run_dispatch'",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(outbox_id) = outbox_id else {
        return Ok(false);
    };
    let row = sqlx::query(AUTHORITY_SQL)
        .bind(scope.company_id())
        .bind(outbox_id)
        .fetch_one(&mut *tx)
        .await?;
    let authority =
        match derive_on(&mut tx, scope, row, outbox_id, Uuid::nil(), witness.clone()).await? {
            DispatchAuthorization::Authorized(authority) => authority,
            _ => return Ok(false),
        };
    let run = sqlx::query(
        "SELECT status, employee_id, employee_revision_id, routing_decision_id,
                message_id, root_message_id, runtime_adapter
           FROM runs WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(run) = run else { return Ok(false) };
    let status: String = run.try_get("status")?;
    if super::parse_status(&status)?.is_terminal() {
        return Ok(true);
    }
    if run.try_get::<String, _>("employee_id")? != authority.employee_id().as_str()
        || run.try_get::<Uuid, _>("employee_revision_id")? != authority.employee_revision_id()
        || run.try_get::<Option<Uuid>, _>("routing_decision_id")? != authority.routing_decision_id()
        || run.try_get::<Option<Vec<u8>>, _>("message_id")?.as_deref()
            != authority
                .message_id()
                .map(|id| id.as_bytes().to_vec())
                .as_deref()
        || run
            .try_get::<Option<Vec<u8>>, _>("root_message_id")?
            .as_deref()
            != authority
                .root_message_id()
                .map(|id| id.as_bytes().to_vec())
                .as_deref()
        || run.try_get::<String, _>("runtime_adapter")? != authority.binding().adapter
    {
        return Ok(false);
    }
    // Even an unchanged generation/deadline is a new admission attempt. Its
    // fresh token makes the deferred guard recheck expiry immediately at commit.
    sqlx::query(
        "UPDATE runs SET office_admission_generation = $3,
                office_admission_valid_before = $4, office_admission_token = $5
          WHERE company_id = $1 AND id = $2",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .bind(witness.generation())
    .bind(witness.valid_before())
    .bind(Uuid::new_v4())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}
