use ortak_control::inbox::InboxEvent;
use ortak_control::ports::Normalization;
#[cfg(test)]
use ortak_control::run_event::RunEventPayload;
use ortak_control::service::office_input_hash;
use ortak_control::{CompanyScope, MessageId};
use ortak_domain::EmployeeId;
use ortak_office::event::MAX_CONTENT_BYTES;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{sql, OutputFailure};

pub(super) struct Target {
    pub kind: u16,
    pub tags: Vec<Vec<String>>,
    pub source_facts: serde_json::Value,
}

pub(super) async fn target(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<Target, OutputFailure> {
    // Shared Office authority is already held. Lock the reviewed source before
    // SOURCE locks the run, including post-ACK admission into memory.
    let current: bool = sqlx::query_scalar("SELECT ortak_lock_run_reviewed_memory($1,$2)")
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_one(&mut *connection)
        .await?;
    if !current {
        return Err(OutputFailure::Permanent("office_output_authority_changed"));
    }
    let row = sqlx::query(sql::SOURCE)
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(OutputFailure::Permanent("office_output_source_missing"))?;
    if row.try_get::<bool, _>("cancelled")? {
        return Err(OutputFailure::Permanent("office_output_cancel_requested"));
    }
    let intent: Option<String> = row.try_get("delivery_intent")?;
    if row.try_get::<String, _>("status")? != "completed"
        || !matches!(intent.as_deref(), Some("reply" | "channel"))
    {
        return Err(OutputFailure::Permanent("office_output_not_publishable"));
    }
    if row.try_get::<String, _>("company_status")? != "active"
        || row.try_get::<Option<bool>, _>("pinned")? != Some(true)
        || row.try_get::<Option<bool>, _>("same_identity")? != Some(true)
        || row.try_get::<Option<String>, _>("inbox_state")?.as_deref() != Some("decided")
    {
        return Err(OutputFailure::Permanent("office_output_authority_changed"));
    }
    let message = row
        .try_get::<Option<Vec<u8>>, _>("message_id")?
        .ok_or(OutputFailure::Permanent("office_output_source_missing"))?;
    let root = row
        .try_get::<Option<Vec<u8>>, _>("root_message_id")?
        .ok_or(OutputFailure::Permanent("office_output_source_missing"))?;
    let message = MessageId::try_from_slice(&message)?;
    let root = MessageId::try_from_slice(&root)?;
    let employee = EmployeeId::parse(row.try_get::<&str, _>("employee_id")?)
        .map_err(|_| OutputFailure::Permanent("office_output_source_invalid"))?;
    let channel: Uuid = row
        .try_get::<Option<Uuid>, _>("channel_id")?
        .ok_or(OutputFailure::Permanent("office_output_source_missing"))?;
    let kind: i32 = row.try_get("event_kind")?;
    let author: Vec<u8> = row.try_get("author_pubkey")?;
    let inbox = InboxEvent {
        event_id: message,
        event_kind: kind,
        event_created_at: row.try_get("event_created_at")?,
        author_pubkey: author
            .as_slice()
            .try_into()
            .map_err(|_| OutputFailure::Permanent("office_output_source_invalid"))?,
        channel_id: Some(channel),
    };
    let Normalization::Message(normalized) =
        ortak_office::PgChannelNormalizer::normalize_on(connection, scope, &inbox).await?
    else {
        return Err(OutputFailure::Permanent("office_output_authority_changed"));
    };
    let hash = office_input_hash(
        &normalized.envelope,
        normalized.root_message_id,
        &normalized.eligible_employee_ids,
    );
    if row
        .try_get::<Option<Vec<u8>>, _>("office_input_hash")?
        .as_deref()
        != Some(hash.as_slice())
        || normalized.root_message_id != root
        || !normalized.eligible_employee_ids.contains(&employee)
    {
        return Err(OutputFailure::Permanent("office_output_authority_changed"));
    }
    let kind = u16::try_from(kind)
        .map_err(|_| OutputFailure::Permanent("office_output_source_invalid"))?;
    if !ortak_office::event::is_allowed_publish_kind(kind) {
        return Err(OutputFailure::Permanent("office_output_source_invalid"));
    }
    // A human reply begins a new delivery chain even inside an older Office
    // thread. Keep that chain pin above; NIP-10 needs the separate thread root.
    let reply = intent.as_deref() == Some("reply");
    let thread_root = if reply {
        ortak_office::postgres::reply_root_on(connection, scope, &inbox)
            .await?
            .ok_or(OutputFailure::Permanent("office_output_authority_changed"))?
    } else {
        message
    };
    Ok(Target {
        source_facts: serde_json::json!({
            "employee_id": employee.as_str(),
            "employee_revision_id": row.try_get::<Uuid,_>("employee_revision_id")?,
            "routing_decision_id": row.try_get::<Uuid,_>("routing_decision_id")?,
            "message_id": message.to_hex(), "root_message_id": root.to_hex(),
            "delivery_intent": intent.as_deref(), "office_input_hash": hex::encode(hash),
        }),
        kind,
        tags: canonical_tags(channel, message, thread_root, reply),
    })
}

fn canonical_tags(
    channel: Uuid,
    parent: MessageId,
    root: MessageId,
    reply: bool,
) -> Vec<Vec<String>> {
    let mut tags = vec![vec!["h".to_owned(), channel.to_string()]];
    if reply {
        // Retained NIP-10 convention: a direct reply needs only the reply tag.
        if root != parent {
            tags.push(vec![
                "e".to_owned(),
                root.to_hex(),
                String::new(),
                "root".to_owned(),
            ]);
        }
        tags.push(vec![
            "e".to_owned(),
            parent.to_hex(),
            String::new(),
            "reply".to_owned(),
        ]);
    }
    tags
}

pub(super) async fn final_text(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<String, OutputFailure> {
    let turn:Option<serde_json::Value>=sqlx::query_scalar("SELECT payload->'turn' FROM run_events
        WHERE company_id=$1 AND run_id=$2 AND event_type='assistant.delta' ORDER BY sequence DESC LIMIT 1")
        .bind(scope.company_id()).bind(run_id).fetch_optional(&mut *connection).await?;
    let turn = turn.ok_or(OutputFailure::Permanent("office_output_empty"))?;
    let stats=sqlx::query("SELECT count(*) AS fragments,
        COALESCE(sum(octet_length(payload->'delta'->>'text')),0)::bigint AS bytes,
        COALESCE(sum(octet_length(payload::text)),0)::bigint AS payload_bytes
        FROM run_events WHERE company_id=$1 AND run_id=$2 AND event_type='assistant.delta' AND payload->'turn'=$3")
        .bind(scope.company_id()).bind(run_id).bind(&turn).fetch_one(&mut *connection).await?;
    if stats.try_get::<i64, _>("fragments")? > 4096
        || stats.try_get::<i64, _>("payload_bytes")? > 1024 * 1024
    {
        return Err(OutputFailure::Permanent("office_output_fragment_limit"));
    }
    if stats.try_get::<i64, _>("bytes")? > MAX_CONTENT_BYTES as i64 {
        return Err(OutputFailure::Permanent("office_output_oversized"));
    }
    // Fetch only after bounding both the fragment count and total text size.
    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM run_events
        WHERE company_id=$1 AND run_id=$2 AND event_type='assistant.delta' AND payload->'turn'=$3
        ORDER BY sequence LIMIT 4097",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .bind(turn)
    .fetch_all(connection)
    .await?;
    assemble_text(payloads)
}

fn assemble_text(payloads: Vec<serde_json::Value>) -> Result<String, OutputFailure> {
    use ortak_control::run_event::{assemble_final_text, FinalTextRefusal};
    assemble_final_text(payloads).map_err(|reason| {
        OutputFailure::Permanent(match reason {
            FinalTextRefusal::FragmentLimit => "office_output_fragment_limit",
            FinalTextRefusal::InvalidDelta => "office_output_invalid_delta",
            FinalTextRefusal::Truncated => "office_output_truncated",
            FinalTextRefusal::Oversized => "office_output_oversized",
            FinalTextRefusal::Empty => "office_output_empty",
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ortak_control::run_event::BoundedText;

    #[test]
    fn retained_reply_tags_do_not_duplicate_a_direct_root() {
        let channel = Uuid::new_v4();
        let parent = MessageId::from_bytes([1; 32]);
        let root = MessageId::from_bytes([2; 32]);
        assert_eq!(
            canonical_tags(channel, parent, parent, true),
            vec![
                vec!["h".to_owned(), channel.to_string()],
                vec![
                    "e".to_owned(),
                    parent.to_hex(),
                    "".to_owned(),
                    "reply".to_owned()
                ]
            ]
        );
        let nested = canonical_tags(channel, parent, root, true);
        assert_eq!(nested.len(), 3);
        assert_eq!(nested[1][3], "root");
        assert_eq!(nested[2][3], "reply");
        assert_eq!(canonical_tags(channel, parent, root, false).len(), 1);
    }

    #[test]
    fn final_reply_rejects_truncation_empty_and_oversized_text() {
        let payload = |delta| {
            serde_json::to_value(RunEventPayload::AssistantDelta { turn: 1, delta })
                .expect("serialize")
        };
        let mut truncated = BoundedText::raw("kept");
        truncated.truncated = true;
        assert!(matches!(
            assemble_text(vec![payload(truncated)]),
            Err(OutputFailure::Permanent("office_output_truncated"))
        ));
        assert!(matches!(
            assemble_text(vec![payload(BoundedText::raw(" \n"))]),
            Err(OutputFailure::Permanent("office_output_empty"))
        ));
        assert!(matches!(
            assemble_text(vec![
                payload(BoundedText::raw("a".repeat(MAX_CONTENT_BYTES))),
                payload(BoundedText::raw("b"))
            ]),
            Err(OutputFailure::Permanent("office_output_oversized"))
        ));
        assert_eq!(
            assemble_text(vec![
                payload(BoundedText::raw("First ")),
                payload(BoundedText::raw("second"))
            ])
            .expect("valid text"),
            "First second"
        );
    }
}
