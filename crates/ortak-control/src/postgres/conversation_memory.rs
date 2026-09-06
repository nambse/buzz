//! Current-read canonical D4 conversation identity resolution.
//!
//! A single SQL statement observes the source, project/Office readers and at
//! most 32 parent edges. The result is neither a retained epoch nor permission
//! to approve, publish, recall or dispatch. A caller performing such work must
//! re-resolve under the shared Office fence and its own current project/run
//! authority at the final persist. In particular, migration 48 does not yet
//! fence every thread root/depth mutation. No routing root is consulted here.

use chrono::{DateTime, Utc};
use ortak_domain::EmployeeId;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::memory::conversation::{
    ConversationAudienceKind, ConversationAudienceV1, ConversationProvenanceV1,
};
use crate::{CompanyScope, MessageId, Result};

mod query;
mod resolve;

/// Maximum parent edges, allowing the source plus 32 ancestors.
pub const MAX_CONVERSATION_ANCESTRY: usize = 32;
/// Source text is hashed privately and never returned by this resolver.
pub const MAX_CONVERSATION_SOURCE_BYTES: usize = 65_536;
/// Per-event encoded tags bound, enforced before fetching them from PostgreSQL.
pub const MAX_CONVERSATION_TAG_BYTES: usize = 16_384;

/// Server-authenticated read selection. Grant slices are the caller's current
/// signed/configured ceiling, not values decoded from an untrusted request.
/// Database membership and project read grants are checked independently.
pub struct ConversationReadRequest<'a> {
    /// Company resolved from the authenticated Office community.
    pub scope: &'a CompanyScope,
    /// Explicit project; it is never inferred from a message's channel.
    pub project_id: Uuid,
    /// Selected durable employee identity, independent of runtime/model.
    pub employee_id: &'a EmployeeId,
    /// Authenticated human requesting this observation.
    pub human_public_key: &'a [u8; 32],
    /// Current caller channel ceiling. At most 128 entries are accepted.
    pub channel_grants: &'a [Uuid],
    /// Current caller employee ceiling. At most 128 entries are accepted.
    pub employee_grants: &'a [EmployeeId],
    /// Source ID only; its partition timestamp comes from the decided inbox.
    pub source_message_id: MessageId,
    /// Explicit channel or canonical thread; no fallback between them.
    pub audience_kind: ConversationAudienceKind,
}

/// A bounded database observation. Private fields prevent callers from
/// manufacturing this resolver's result; even a genuine result is not durable
/// authority and has no serialization or permission-granting conversion.
#[derive(Clone, Debug)]
pub struct ConversationObservation {
    provenance: ConversationProvenanceV1,
    observed_at: DateTime<Utc>,
    valid_before: Option<DateTime<Utc>>,
}

impl ConversationObservation {
    /// Canonical audience resolved from current storage.
    pub fn audience(&self) -> &ConversationAudienceV1 {
        self.provenance.audience()
    }

    /// Exact source locator and server-computed evidence digest.
    pub fn provenance(&self) -> &ConversationProvenanceV1 {
        &self.provenance
    }

    /// Database statement time at which this read observed the selected scope.
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    /// Earliest selected channel/Office-key expiry. This is only an upper bound,
    /// never a cache lease: grants or canonical rows may change sooner.
    pub fn valid_before(&self) -> Option<DateTime<Utc>> {
        self.valid_before
    }
}

/// Resolves a currently visible decided plaintext stream source and canonical
/// ancestry from one database snapshot. Unknown, inaccessible, oversized and
/// inconsistent sources all return `None`; database failures propagate.
///
/// No credential, provider, runtime, routing root or memory store is consulted.
/// This performs one read statement and acquires no enduring authority. Use a
/// caller-owned statement deadline; do not retain this value as a future grant.
pub async fn resolve_conversation_on(
    connection: &mut PgConnection,
    request: &ConversationReadRequest<'_>,
) -> Result<Option<ConversationObservation>> {
    let Some(community) = request.scope.community_id() else {
        return Ok(None);
    };
    if request.project_id.is_nil()
        || request.channel_grants.is_empty()
        || request.channel_grants.len() > 128
        || request.employee_grants.len() > 128
        || !request.employee_grants.contains(request.employee_id)
    {
        return Ok(None);
    }
    let rows = sqlx::query(query::RESOLVE)
        .bind(request.scope.company_id())
        .bind(community)
        .bind(request.project_id)
        .bind(request.employee_id.as_str())
        .bind(request.human_public_key.as_slice())
        .bind(request.channel_grants)
        .bind(request.source_message_id.as_bytes().as_slice())
        .bind(MAX_CONVERSATION_SOURCE_BYTES as i32)
        .bind(MAX_CONVERSATION_TAG_BYTES as i32)
        .bind(MAX_CONVERSATION_ANCESTRY as i32)
        .fetch_all(connection)
        .await?;
    resolve::observation(request, &rows)
}

#[cfg(test)]
mod postgres_tests;
