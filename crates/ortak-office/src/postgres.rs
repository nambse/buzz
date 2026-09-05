//! PostgreSQL implementation of [`OfficeDeliveryRepository`] on the existing
//! [`PgControlPlane`], using only migration 0045 tables.
//!
//! Provenance is derived, never accepted:
//!
//! - `runs` (company-scoped by id) supplies `employee_id` and
//!   `employee_revision_id`, and must be `completed` with a `reply` or
//!   `channel` delivery intent.
//! - `employee_revisions` (company, employee, id) supplies the Office public
//!   key and signer reference the pinned revision declares in its manifest.
//! - `employee_office_bindings` (company, public key) must be the row for
//!   that key, owned by the same employee, naming the same signer reference,
//!   verified, and inside its validity window at the database clock. Its
//!   `signer_ref` and `public_key` columns are what signing uses.
//!
//! `outbox.payload` pins the derived intent, `signed_event_id` /
//! `signed_event_bytes` hold the frozen event, and `lease_token` / `state`
//! fence every write.

use ortak_control::office_identity::OfficePublicKey;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::{CredentialRef, EmployeeId};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};

mod authority;
pub(crate) use authority::before_publish;
use uuid::Uuid;

use crate::error::{BindingRejection, OfficeDeliveryError, Result};
use crate::event::{FrozenSignedEvent, IntentFingerprint, OfficePublishPayload, StoredSignedEvent};
use crate::repository::{
    AuthorizedOfficePublish, EnqueueOutcome, FreezeOutcome, FrozenLookup, OfficeDeliveryRepository,
    OfficePublishDraft,
};

/// Delivery intents that publish a message. `silent` never enqueues.
const PUBLISHING_INTENTS: [&str; 2] = ["reply", "channel"];
/// The only run status that may publish: delivery happens at completion.
const PUBLISHABLE_STATUS: &str = "completed";

/// Identity and signing provenance derived from the control plane for one
/// run, before an outbox row is involved.
struct Provenance {
    employee_id: EmployeeId,
    employee_revision_id: Uuid,
    binding_id: Uuid,
    signer_ref: CredentialRef,
    public_key: OfficePublicKey,
}

fn unauthorized(
    employee_id: &str,
    employee_revision_id: Uuid,
    reason: BindingRejection,
) -> OfficeDeliveryError {
    OfficeDeliveryError::BindingUnauthorized {
        employee_id: employee_id.to_owned(),
        employee_revision_id,
        reason,
    }
}

/// Derives who publishes for `run_id` and under which signer and key.
///
/// Everything is read by company id; nothing from the caller's draft other
/// than the run id is consulted.
async fn derive_provenance(
    connection: &mut PgConnection,
    company_id: Uuid,
    run_id: Uuid,
) -> Result<Provenance> {
    let run = sqlx::query(
        "SELECT r.employee_id, r.employee_revision_id, r.status, r.delivery_intent,
                rev.manifest #>> '{office,public_key}' AS manifest_public_key,
                rev.manifest #>> '{office,signer_ref}' AS manifest_signer_ref
           FROM runs r
           JOIN employee_revisions rev
             ON rev.company_id = r.company_id
            AND rev.employee_id = r.employee_id
            AND rev.id = r.employee_revision_id
          WHERE r.company_id = $1 AND r.id = $2 FOR UPDATE OF r",
    )
    .bind(company_id)
    .bind(run_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(run) = run else {
        return Err(OfficeDeliveryError::UnknownRun { run_id });
    };
    let employee_id_raw: String = run.try_get("employee_id")?;
    let employee_revision_id: Uuid = run.try_get("employee_revision_id")?;
    let status: String = run.try_get("status")?;
    let delivery_intent: Option<String> = run.try_get("delivery_intent")?;
    if status != PUBLISHABLE_STATUS
        || !delivery_intent
            .as_deref()
            .is_some_and(|intent| PUBLISHING_INTENTS.contains(&intent))
    {
        return Err(OfficeDeliveryError::RunNotPublishable {
            run_id,
            status,
            delivery_intent,
        });
    }
    let employee_id = EmployeeId::parse(employee_id_raw.as_str()).map_err(|error| {
        OfficeDeliveryError::Control(ortak_control::ControlError::InvalidData(format!(
            "runs.employee_id for run {run_id}: {error}"
        )))
    })?;
    let manifest_public_key: Option<String> = run.try_get("manifest_public_key")?;
    let manifest_signer_ref: Option<String> = run.try_get("manifest_signer_ref")?;
    let Some(public_key) = manifest_public_key
        .as_deref()
        .and_then(|hex| OfficePublicKey::parse_hex(hex).ok())
    else {
        return Err(unauthorized(
            &employee_id_raw,
            employee_revision_id,
            BindingRejection::RevisionWithoutKey,
        ));
    };

    let binding = sqlx::query(
        "SELECT id, employee_id, signer_ref,
                verified_at IS NOT NULL AS verified,
                valid_from <= clock_timestamp() AS started,
                (valid_until IS NULL OR valid_until > clock_timestamp()) AS unexpired
           FROM employee_office_bindings
          WHERE company_id = $1 AND public_key = $2",
    )
    .bind(company_id)
    .bind(public_key.as_bytes().to_vec())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(binding) = binding else {
        return Err(unauthorized(
            &employee_id_raw,
            employee_revision_id,
            BindingRejection::Missing,
        ));
    };
    let binding_id: Uuid = binding.try_get("id")?;
    let binding_employee: String = binding.try_get("employee_id")?;
    let signer_ref_raw: String = binding.try_get("signer_ref")?;
    let verified: bool = binding.try_get("verified")?;
    let started: bool = binding.try_get("started")?;
    let unexpired: bool = binding.try_get("unexpired")?;
    let rejection = if binding_employee != employee_id_raw {
        Some(BindingRejection::WrongEmployee)
    } else if manifest_signer_ref.as_deref() != Some(signer_ref_raw.as_str()) {
        Some(BindingRejection::SignerMismatch)
    } else if !verified {
        Some(BindingRejection::Unverified)
    } else if !started {
        Some(BindingRejection::NotYetValid)
    } else if !unexpired {
        Some(BindingRejection::Retired)
    } else {
        None
    };
    if let Some(reason) = rejection {
        return Err(unauthorized(&employee_id_raw, employee_revision_id, reason));
    }
    let signer_ref = CredentialRef::parse(&signer_ref_raw).map_err(|error| {
        OfficeDeliveryError::Control(ortak_control::ControlError::InvalidData(format!(
            "employee_office_bindings.signer_ref for binding {binding_id}: {error}"
        )))
    })?;
    Ok(Provenance {
        employee_id,
        employee_revision_id,
        binding_id,
        signer_ref,
        public_key,
    })
}

/// Derives the authorized publish for `draft` on outbox row `outbox_id`.
async fn authorize(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    outbox_id: Uuid,
    draft: &OfficePublishDraft,
) -> Result<AuthorizedOfficePublish> {
    let provenance = derive_provenance(connection, scope.company_id(), draft.run_id).await?;
    authority::channel(connection, scope, draft, &provenance).await?;
    let intent = draft
        .clone()
        .into_intent(provenance.employee_id, provenance.employee_revision_id);
    intent.validate()?;
    Ok(AuthorizedOfficePublish::new(
        outbox_id,
        intent,
        provenance.binding_id,
        provenance.signer_ref,
        provenance.public_key,
    ))
}

/// The caller-visible part of an authorized publish, for re-authorization.
fn draft_of(authorized: &AuthorizedOfficePublish) -> OfficePublishDraft {
    let intent = authorized.intent();
    OfficePublishDraft {
        company_id: intent.company_id,
        run_id: intent.run_id,
        kind: intent.kind,
        tags: intent.tags.clone(),
        content: intent.content.clone(),
    }
}

struct PublishRow {
    kind: String,
    run_id: Option<Uuid>,
    state: String,
    lease_token: Option<Uuid>,
    lease_live: bool,
    payload: serde_json::Value,
    signed_event_id: Option<Vec<u8>>,
    signed_event_bytes: Option<Vec<u8>>,
}

impl PublishRow {
    fn from_row(row: &PgRow) -> Result<Self> {
        Ok(Self {
            kind: row.try_get("kind")?,
            run_id: row.try_get("run_id")?,
            state: row.try_get("state")?,
            lease_token: row.try_get("lease_token")?,
            lease_live: row.try_get("lease_live")?,
            payload: row.try_get("payload")?,
            signed_event_id: row.try_get("signed_event_id")?,
            signed_event_bytes: row.try_get("signed_event_bytes")?,
        })
    }
}

/// What a row must have pinned for a request or event to be allowed on it.
struct ExpectedIntent<'a> {
    run_id: Uuid,
    fingerprint: IntentFingerprint,
    employee_id: &'a EmployeeId,
    employee_revision_id: Uuid,
    public_key: &'a OfficePublicKey,
}

impl<'a> ExpectedIntent<'a> {
    fn of_authorized(authorized: &'a AuthorizedOfficePublish) -> Self {
        Self {
            run_id: authorized.run_id(),
            fingerprint: authorized.fingerprint(),
            employee_id: authorized.employee_id(),
            employee_revision_id: authorized.employee_revision_id(),
            public_key: authorized.public_key(),
        }
    }

    fn of_event(event: &'a FrozenSignedEvent) -> Self {
        Self {
            run_id: event.run_id(),
            fingerprint: event.fingerprint(),
            employee_id: event.employee_id(),
            employee_revision_id: event.employee_revision_id(),
            public_key: event.public_key(),
        }
    }
}

fn invalid_row(outbox_id: Uuid, detail: impl ToString) -> OfficeDeliveryError {
    OfficeDeliveryError::InvalidRow {
        outbox_id,
        detail: detail.to_string(),
    }
}

/// Checks kind, run, and pinned intent, then reports whether the lease is live.
fn check_row(
    outbox_id: Uuid,
    row: &PublishRow,
    lease: &OutboxLease,
    expected: &ExpectedIntent<'_>,
) -> Result<(OfficePublishPayload, bool)> {
    let kind = OutboxKind::parse(&row.kind)
        .ok_or_else(|| invalid_row(outbox_id, format!("outbox.kind holds {:?}", row.kind)))?;
    if kind != OutboxKind::OfficePublish {
        return Err(OfficeDeliveryError::WrongKind { found: kind });
    }
    if row.run_id != Some(expected.run_id) || lease.run_id != Some(expected.run_id) {
        return Err(OfficeDeliveryError::WrongRun {
            expected: expected.run_id,
            found: row.run_id,
        });
    }
    let payload: OfficePublishPayload = serde_json::from_value(row.payload.clone())
        .map_err(|error| invalid_row(outbox_id, format!("payload: {error}")))?;
    if payload.intent_fingerprint != expected.fingerprint
        || &payload.employee_id != expected.employee_id
        || payload.employee_revision_id != expected.employee_revision_id
        || &payload.public_key != expected.public_key
    {
        return Err(OfficeDeliveryError::IntentMismatch { outbox_id });
    }
    let live =
        row.state == "pending" && row.lease_token == Some(lease.lease_token) && row.lease_live;
    Ok((payload, live))
}

fn read_frozen(
    scope: &CompanyScope,
    outbox_id: Uuid,
    run_id: Uuid,
    payload: &OfficePublishPayload,
    event_id: &[u8],
    bytes: &[u8],
) -> Result<FrozenSignedEvent> {
    FrozenSignedEvent::from_stored(
        &StoredSignedEvent {
            company_id: scope.company_id(),
            run_id,
            event_id,
            signed_bytes: bytes,
        },
        payload,
    )
    .map_err(|error| invalid_row(outbox_id, format!("frozen event: {error}")))
}

/// Enqueues inside the caller's transaction, preserving its atomic output job update.
/// The caller must hold the shared Office authority fence before any row locks.
/// This helper never commits and performs no network work.
pub async fn enqueue_office_publish_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    draft: &OfficePublishDraft,
) -> Result<EnqueueOutcome> {
    draft.validate()?;
    if draft.company_id != scope.company_id() {
        return Err(OfficeDeliveryError::CompanyMismatch {
            expected: scope.company_id(),
            found: draft.company_id,
        });
    }
    // Authorize before touching the outbox; the row id is attached once
    // the insert (or the replayed row) is known.
    let provisional = authorize(connection, scope, Uuid::nil(), draft).await?;
    let payload = provisional.payload();
    let dedup_key = provisional.dedup_key();
    let with_row = |outbox_id: Uuid| {
        AuthorizedOfficePublish::new(
            outbox_id,
            provisional.intent().clone(),
            provisional.binding_id(),
            provisional.signer_ref().clone(),
            *provisional.public_key(),
        )
    };
    let inserted = sqlx::query(
        "INSERT INTO outbox (company_id, kind, dedup_key, run_id, payload)
             VALUES ($1, 'office_publish', $2, $3, $4)
             ON CONFLICT (company_id, dedup_key) DO NOTHING
             RETURNING id",
    )
    .bind(scope.company_id())
    .bind(&dedup_key)
    .bind(draft.run_id)
    .bind(serde_json::to_value(&payload).map_err(ortak_control::ControlError::from)?)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(row) = inserted {
        return Ok(EnqueueOutcome::Enqueued(with_row(row.try_get("id")?)));
    }

    let existing = sqlx::query(
        "SELECT id, kind, run_id, payload FROM outbox
              WHERE company_id = $1 AND dedup_key = $2",
    )
    .bind(scope.company_id())
    .bind(&dedup_key)
    .fetch_one(&mut *connection)
    .await?;
    let outbox_id: Uuid = existing.try_get("id")?;
    let kind: String = existing.try_get("kind")?;
    let run_id: Option<Uuid> = existing.try_get("run_id")?;
    let stored: serde_json::Value = existing.try_get("payload")?;
    let stored: OfficePublishPayload = serde_json::from_value(stored)
        .map_err(|error| invalid_row(outbox_id, format!("payload: {error}")))?;
    if kind != OutboxKind::OfficePublish.as_str()
        || run_id != Some(draft.run_id)
        || stored != payload
    {
        return Err(OfficeDeliveryError::IntentMismatch { outbox_id });
    }
    Ok(EnqueueOutcome::Existing(with_row(outbox_id)))
}

impl OfficeDeliveryRepository for PgControlPlane {
    async fn enqueue_office_publish(
        &self,
        scope: &CompanyScope,
        draft: &OfficePublishDraft,
    ) -> Result<EnqueueOutcome> {
        let mut tx = self.pool().begin().await?;
        authority::lock(&mut tx, scope).await?;
        let result = enqueue_office_publish_on(&mut tx, scope, draft).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn frozen_event(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authorized: &AuthorizedOfficePublish,
    ) -> Result<FrozenLookup> {
        authorized.intent().validate()?;
        if authorized.company_id() != scope.company_id() {
            return Err(OfficeDeliveryError::CompanyMismatch {
                expected: scope.company_id(),
                found: authorized.company_id(),
            });
        }
        if lease.kind != OutboxKind::OfficePublish {
            return Err(OfficeDeliveryError::WrongKind { found: lease.kind });
        }
        if authorized.outbox_id() != lease.id {
            return Err(OfficeDeliveryError::WrongRow {
                expected: authorized.outbox_id(),
                found: lease.id,
            });
        }
        // Re-derive provenance now: the presented object must still be what
        // the control plane would authorize for this row, and the binding
        // must still be verified and in-window before any signing.
        let mut tx = self.pool().begin().await?;
        authority::lock(&mut tx, scope).await?;
        let current = authorize(&mut tx, scope, lease.id, &draft_of(authorized)).await?;
        if current != *authorized {
            return Err(OfficeDeliveryError::IntentMismatch {
                outbox_id: lease.id,
            });
        }
        let row = sqlx::query(
            "SELECT kind, run_id, state, lease_token, COALESCE(lease_expires_at>clock_timestamp(),false) AS lease_live, payload, signed_event_id, signed_event_bytes
               FROM outbox WHERE company_id = $1 AND id = $2",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Err(OfficeDeliveryError::NotFound {
                outbox_id: lease.id,
            });
        };
        let row = PublishRow::from_row(&row)?;
        let (payload, live) = check_row(
            lease.id,
            &row,
            lease,
            &ExpectedIntent::of_authorized(authorized),
        )?;
        if !live {
            return Ok(FrozenLookup::StaleLease);
        }
        let result = match (&row.signed_event_id, &row.signed_event_bytes) {
            (Some(event_id), Some(bytes)) => Ok(FrozenLookup::Frozen(Box::new(read_frozen(
                scope,
                lease.id,
                authorized.run_id(),
                &payload,
                event_id,
                bytes,
            )?))),
            (None, None) => Ok(FrozenLookup::Unfrozen),
            _ => Err(invalid_row(
                lease.id,
                "signed_event_id and signed_event_bytes are not set together",
            )),
        };
        tx.commit().await?;
        result
    }

    async fn freeze_signed_event(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        event: &FrozenSignedEvent,
    ) -> Result<FreezeOutcome> {
        if event.company_id() != scope.company_id() {
            return Err(OfficeDeliveryError::CompanyMismatch {
                expected: scope.company_id(),
                found: event.company_id(),
            });
        }
        if lease.kind != OutboxKind::OfficePublish {
            return Err(OfficeDeliveryError::WrongKind { found: lease.kind });
        }

        let mut tx = self.pool().begin().await?;
        authority::lock(&mut tx, scope).await?;
        let current = authorize(&mut tx, scope, lease.id, &authority::draft(event)?).await?;
        if !authority::matches(&current, event) {
            return Err(authority::denied());
        }
        let row = sqlx::query(
            "SELECT kind, run_id, state, lease_token, COALESCE(lease_expires_at>clock_timestamp(),false) AS lease_live, payload, signed_event_id, signed_event_bytes
               FROM outbox WHERE company_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Err(OfficeDeliveryError::NotFound {
                outbox_id: lease.id,
            });
        };
        let row = PublishRow::from_row(&row)?;
        let (payload, live) = check_row(lease.id, &row, lease, &ExpectedIntent::of_event(event))?;
        if !live {
            return Ok(FreezeOutcome::StaleLease);
        }

        match (&row.signed_event_id, &row.signed_event_bytes) {
            (Some(stored_id), Some(stored_bytes)) => {
                if stored_id.as_slice() != event.event_id().as_bytes()
                    || stored_bytes.as_slice() != event.signed_bytes()
                {
                    return Err(OfficeDeliveryError::FrozenPayloadConflict {
                        outbox_id: lease.id,
                    });
                }
                let frozen = read_frozen(
                    scope,
                    lease.id,
                    event.run_id(),
                    &payload,
                    stored_id,
                    stored_bytes,
                )?;
                tx.commit().await?;
                Ok(FreezeOutcome::Frozen(Box::new(frozen)))
            }
            (None, None) => {
                let written = sqlx::query(
                    "UPDATE outbox
                        SET signed_event_id = $4, signed_event_bytes = $5, updated_at = now()
                      WHERE company_id = $1 AND id = $2 AND lease_token = $3
                        AND state = 'pending' AND kind = 'office_publish'
                        AND lease_expires_at > clock_timestamp()
                        AND signed_event_id IS NULL
                      RETURNING signed_event_id, signed_event_bytes",
                )
                .bind(scope.company_id())
                .bind(lease.id)
                .bind(lease.lease_token)
                .bind(event.event_id().as_bytes().as_slice())
                .bind(event.signed_bytes())
                .fetch_optional(&mut *tx)
                .await
                .map_err(|error| {
                    if error
                        .as_database_error()
                        .is_some_and(|database| database.is_unique_violation())
                    {
                        OfficeDeliveryError::DuplicateEventId {
                            event_id: event.event_id(),
                        }
                    } else {
                        error.into()
                    }
                })?;
                let Some(written) = written else {
                    return Ok(FreezeOutcome::StaleLease);
                };
                let stored_id: Vec<u8> = written.try_get("signed_event_id")?;
                let stored_bytes: Vec<u8> = written.try_get("signed_event_bytes")?;
                let frozen = read_frozen(
                    scope,
                    lease.id,
                    event.run_id(),
                    &payload,
                    &stored_id,
                    &stored_bytes,
                )?;
                if frozen.event_id() != event.event_id()
                    || frozen.signed_bytes() != event.signed_bytes()
                {
                    return Err(invalid_row(
                        lease.id,
                        "frozen bytes read back differ from the bytes written",
                    ));
                }
                tx.commit().await?;
                Ok(FreezeOutcome::Frozen(Box::new(frozen)))
            }
            _ => Err(invalid_row(
                lease.id,
                "signed_event_id and signed_event_bytes are not set together",
            )),
        }
    }
}
