//! Reply ancestry is Office thread metadata, never a routing delivery-chain root.

use ortak_control::inbox::InboxEvent;
use ortak_control::{CompanyScope, MessageId, Result};
use sqlx::PgConnection;

/// Read the canonical NIP-10 root of a reply to this exact accepted source.
///
/// Callers must hold the shared Office authority fence and separately validate
/// current normalization/employee authority. This observation grants no signing
/// authority. Missing, deleted, cross-channel, or inconsistent ancestry refuses;
/// a top-level source (including a neutral counter stub) is its own root.
pub async fn reply_root_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    source: &InboxEvent,
) -> Result<Option<MessageId>> {
    let root: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT root.id
         FROM office_company_bindings cb
         JOIN communities cm ON cm.id=cb.community_id
           AND cm.deletion_state='active' AND cm.deleted_at IS NULL
         JOIN events ev ON ev.community_id=cb.community_id
           AND ev.id=$3 AND ev.created_at=$4 AND ev.channel_id=$5
           AND ev.kind=$6 AND ev.pubkey=$7 AND ev.deleted_at IS NULL
         LEFT JOIN thread_metadata tm ON tm.community_id=ev.community_id
           AND tm.event_id=ev.id AND tm.event_created_at=ev.created_at
         JOIN events root ON root.community_id=ev.community_id
           AND root.id=COALESCE(tm.root_event_id,ev.id)
           AND root.created_at=COALESCE(tm.root_event_created_at,ev.created_at)
           AND root.channel_id=ev.channel_id AND root.deleted_at IS NULL
           AND root.kind IN (9,40002)
         WHERE cb.company_id=$1 AND ($2::uuid IS NULL OR cb.community_id=$2)
           AND (tm.event_id IS NULL OR (tm.channel_id=ev.channel_id AND (
             (tm.parent_event_id IS NULL AND tm.parent_event_created_at IS NULL
               AND tm.depth=0 AND (
                 (tm.root_event_id IS NULL AND tm.root_event_created_at IS NULL)
                 OR (tm.root_event_id=ev.id AND tm.root_event_created_at=ev.created_at)))
             OR (tm.parent_event_id IS NOT NULL AND tm.parent_event_created_at IS NOT NULL
               AND tm.depth BETWEEN 1 AND 99 AND tm.root_event_id IS NOT NULL
               AND tm.root_event_created_at IS NOT NULL AND tm.root_event_id<>ev.id))))",
    )
    .bind(scope.company_id())
    .bind(scope.community_id())
    .bind(source.event_id.as_bytes().as_slice())
    .bind(source.event_created_at)
    .bind(source.channel_id)
    .bind(source.event_kind)
    .bind(source.author_pubkey.as_slice())
    .fetch_optional(connection)
    .await?;
    root.as_deref().map(MessageId::try_from_slice).transpose()
}
