//! Repository seam for `office_publish` outbox rows.
//!
//! The existing [`OutboxRepository`](ortak_control::ports::OutboxRepository)
//! owns leasing, completion, bounded retry, and terminal failure. This seam
//! adds only what Office delivery needs on top of it: authorizing and
//! enqueueing a row for a run, reading a frozen event back, and freezing a
//! verified signed event under the current lease token.
//!
//! Identity provenance is never caller-asserted. A caller submits an
//! [`OfficePublishDraft`], which names only the company, the run, and the
//! message. The repository derives the employee and pinned revision from the
//! company-scoped `runs` row, and the signer reference and public key from
//! the verified, in-window `employee_office_bindings` row that the pinned
//! revision declares. The result is an [`AuthorizedOfficePublish`], the only
//! object signing, freezing, and retry accept, and the only way to build one
//! is through the repository.

use chrono::{DateTime, Utc};
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::outbox::OutboxLease;
use ortak_control::CompanyScope;
use ortak_domain::{CredentialRef, EmployeeId};
use uuid::Uuid;

use crate::error::Result;
use crate::event::{
    FrozenSignedEvent, IntentFingerprint, OfficeEventError, OfficePublishIntent,
    OfficePublishPayload, UnsignedOfficeEvent,
};
use crate::signer::SigningRequest;

/// What a caller may say about a pending Office publish: which run produced
/// the message and what the message is. Who signs it, under which key, and
/// as which employee revision is decided by the control plane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficePublishDraft {
    /// Company boundary; must equal the resolved scope.
    pub company_id: Uuid,
    /// Run that produced the content.
    pub run_id: Uuid,
    /// Nostr event kind; must satisfy
    /// [`is_allowed_publish_kind`](crate::event::is_allowed_publish_kind).
    pub kind: u16,
    /// Tags as string lists (`[name, value, ...]`).
    pub tags: Vec<Vec<String>>,
    /// Event content.
    pub content: String,
}

impl OfficePublishDraft {
    /// Checks the kind policy and the message bounds without any identity.
    pub fn validate(&self) -> std::result::Result<(), OfficeEventError> {
        self.placeholder_intent().validate()
    }

    /// The draft's message fields on a throwaway identity, used only to run
    /// the identity-independent bounds checks before any database access.
    fn placeholder_intent(&self) -> OfficePublishIntent {
        OfficePublishIntent {
            company_id: self.company_id,
            run_id: self.run_id,
            employee_id: EmployeeId::parse("draft").expect("static id is valid"),
            employee_revision_id: Uuid::nil(),
            kind: self.kind,
            tags: self.tags.clone(),
            content: self.content.clone(),
        }
    }

    /// Attaches control-plane-derived provenance. Crate-private: only the
    /// repository seam may turn a draft into an intent.
    pub(crate) fn into_intent(
        self,
        employee_id: EmployeeId,
        employee_revision_id: Uuid,
    ) -> OfficePublishIntent {
        OfficePublishIntent {
            company_id: self.company_id,
            run_id: self.run_id,
            employee_id,
            employee_revision_id,
            kind: self.kind,
            tags: self.tags,
            content: self.content,
        }
    }
}

/// A publish the control plane authorized: the intent with the employee and
/// revision read from the run row, plus the signer reference and public key
/// read from that revision's verified Office binding.
///
/// There is no public constructor. Instances come only from
/// [`OfficeDeliveryRepository::enqueue_office_publish`], and every field is
/// read-only, so a caller cannot present arbitrary identity, key, or signer
/// values to signing, freezing, or retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedOfficePublish {
    outbox_id: Uuid,
    intent: OfficePublishIntent,
    binding_id: Uuid,
    signer_ref: CredentialRef,
    public_key: OfficePublicKey,
}

impl AuthorizedOfficePublish {
    /// Crate-private constructor for the repository seam.
    pub(crate) fn new(
        outbox_id: Uuid,
        intent: OfficePublishIntent,
        binding_id: Uuid,
        signer_ref: CredentialRef,
        public_key: OfficePublicKey,
    ) -> Self {
        Self {
            outbox_id,
            intent,
            binding_id,
            signer_ref,
            public_key,
        }
    }

    /// Outbox row that pins this publish.
    pub fn outbox_id(&self) -> Uuid {
        self.outbox_id
    }

    /// Validated intent with derived provenance.
    pub fn intent(&self) -> &OfficePublishIntent {
        &self.intent
    }

    /// Company boundary.
    pub fn company_id(&self) -> Uuid {
        self.intent.company_id
    }

    /// Run the publish belongs to.
    pub fn run_id(&self) -> Uuid {
        self.intent.run_id
    }

    /// Authoring employee, from the run row.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.intent.employee_id
    }

    /// Employee revision pinned by the run row.
    pub fn employee_revision_id(&self) -> Uuid {
        self.intent.employee_revision_id
    }

    /// Office binding row that supplied the signer and key.
    pub fn binding_id(&self) -> Uuid {
        self.binding_id
    }

    /// Opaque signer reference from the binding.
    pub fn signer_ref(&self) -> &CredentialRef {
        &self.signer_ref
    }

    /// Public key from the binding; the signature must verify under it.
    pub fn public_key(&self) -> &OfficePublicKey {
        &self.public_key
    }

    /// Intent fingerprint.
    pub fn fingerprint(&self) -> IntentFingerprint {
        self.intent.fingerprint()
    }

    /// Payload pinned on the outbox row.
    pub fn payload(&self) -> OfficePublishPayload {
        OfficePublishPayload::new(&self.intent, self.public_key)
    }

    /// Company-unique idempotency key.
    pub fn dedup_key(&self) -> String {
        OfficePublishPayload::dedup_key(self.intent.run_id)
    }

    /// Builds the signing request for a fresh attempt at `created_at`.
    pub fn signing_request(
        &self,
        created_at: DateTime<Utc>,
    ) -> std::result::Result<SigningRequest, OfficeEventError> {
        let unsigned = UnsignedOfficeEvent::new(self.intent.clone(), self.public_key, created_at)?;
        Ok(SigningRequest::new(unsigned, self.signer_ref.clone()))
    }
}

/// Result of authorizing and enqueueing a publish row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnqueueOutcome {
    /// A new pending row was inserted.
    Enqueued(AuthorizedOfficePublish),
    /// A row for the run with the identical pinned intent already existed.
    Existing(AuthorizedOfficePublish),
}

impl EnqueueOutcome {
    /// Outbox row id.
    pub fn outbox_id(&self) -> Uuid {
        self.authorized().outbox_id()
    }

    /// The canonical authorized publish for the row, whether new or replayed.
    pub fn authorized(&self) -> &AuthorizedOfficePublish {
        match self {
            Self::Enqueued(authorized) | Self::Existing(authorized) => authorized,
        }
    }

    /// Consumes the outcome into the authorized publish.
    pub fn into_authorized(self) -> AuthorizedOfficePublish {
        match self {
            Self::Enqueued(authorized) | Self::Existing(authorized) => authorized,
        }
    }
}

/// Result of reading the frozen event of a leased row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrozenLookup {
    /// The row already holds a frozen event; publish these bytes.
    Frozen(Box<FrozenSignedEvent>),
    /// Nothing is frozen yet; the caller may sign and freeze.
    Unfrozen,
    /// The lease token no longer matches or the row is not pending.
    StaleLease,
}

/// Result of freezing a signed event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FreezeOutcome {
    /// The durable frozen event, read back from the row.
    Frozen(Box<FrozenSignedEvent>),
    /// The lease token no longer matches or the row is not pending; nothing changed.
    StaleLease,
}

/// `office_publish` outbox persistence.
#[allow(async_fn_in_trait)]
pub trait OfficeDeliveryRepository {
    /// Authorizes `draft` against the control plane and inserts one pending
    /// `office_publish` row per run, pinning the derived intent fingerprint,
    /// employee, revision, and key in `payload`.
    ///
    /// Authorization fails closed unless the company-scoped run is
    /// `completed` with a `reply` or `channel` delivery intent and the
    /// revision it pins declares a verified Office binding whose validity
    /// window contains now. Replaying the same draft returns the existing row
    /// and the same authorized publish; a different draft for the same run is
    /// refused.
    async fn enqueue_office_publish(
        &self,
        scope: &CompanyScope,
        draft: &OfficePublishDraft,
    ) -> Result<EnqueueOutcome>;

    /// Re-authorizes `authorized` against the control plane, then reads and
    /// re-verifies the frozen event of the leased row, checking company,
    /// kind, run, pinned intent, and lease token first. A binding that was
    /// retired or un-verified since enqueue fails closed here, before any
    /// signing.
    async fn frozen_event(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authorized: &AuthorizedOfficePublish,
    ) -> Result<FrozenLookup>;

    /// Freezes the verified event into the leased row before its first
    /// publish. Never overwrites: a row that already holds byte-identical
    /// bytes returns them, a row holding different bytes is a conflict, and
    /// a stale lease changes nothing.
    async fn freeze_signed_event(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        event: &FrozenSignedEvent,
    ) -> Result<FreezeOutcome>;
}
