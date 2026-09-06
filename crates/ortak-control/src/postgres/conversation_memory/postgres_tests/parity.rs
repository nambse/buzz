use super::*;
use sqlx::{PgConnection, Row};

/// Every regular fixture resolve passes through the actual installed SQL
/// function. Denial equality is as important as success/hash equality.
pub(super) async fn compare(
    connection: &mut PgConnection,
    request: &ConversationReadRequest<'_>,
    rust: Option<&ConversationObservation>,
) {
    let kind = match request.audience_kind {
        ConversationAudienceKind::Channel => "channel",
        ConversationAudienceKind::Thread => "thread",
    };
    let rows = sqlx::query(
        "SELECT * FROM ortak_conversation_source_observation($1,$2,$3,$4,$5,$6) LIMIT 2",
    )
    .bind(request.scope.company_id())
    .bind(request.project_id)
    .bind(request.employee_id.as_str())
    .bind(request.human_public_key.as_slice())
    .bind(request.source_message_id.as_bytes().as_slice())
    .bind(kind)
    .fetch_all(connection)
    .await
    .expect("SQL source resolver requires source75 installed on the disposable 74 database");
    assert!(
        rows.len() <= 1,
        "SQL observation must return at most one row"
    );
    assert_eq!(
        rust.is_some(),
        !rows.is_empty(),
        "Rust/SQL access or malformed-source refusal differs"
    );
    let Some(rust) = rust else { return };
    let row = &rows[0];
    let audience = rust.audience();
    let provenance = rust.provenance();
    assert_eq!(
        row.try_get::<Uuid, _>("community_id").unwrap(),
        audience.community_id()
    );
    assert_eq!(
        row.try_get::<Uuid, _>("channel_id").unwrap(),
        audience.channel_id()
    );
    assert_eq!(
        row.try_get::<DateTime<Utc>, _>("source_event_created_at")
            .unwrap(),
        provenance.source().created_at()
    );
    assert_eq!(
        row.try_get::<Option<Vec<u8>>, _>("thread_root_event_id")
            .unwrap(),
        audience
            .thread_root()
            .map(|root| root.event_id().as_bytes().to_vec())
    );
    assert_eq!(
        row.try_get::<Option<DateTime<Utc>>, _>("thread_root_event_created_at")
            .unwrap(),
        audience.thread_root().map(|root| root.created_at())
    );
    assert_eq!(
        row.try_get::<Option<DateTime<Utc>>, _>("valid_before")
            .unwrap(),
        rust.valid_before()
    );
    // These are two statements. The observation timestamp is deliberately not
    // compared for equality; exact source/root times and expiry above are.
    let _: DateTime<Utc> = row.try_get("observed_at").unwrap();
    for (column, expected) in [
        ("audience_bytes", audience.canonical_bytes().unwrap()),
        (
            "audience_hash",
            audience.audience_hash().unwrap().as_bytes().to_vec(),
        ),
        (
            "source_evidence_hash",
            provenance.source_evidence_hash().as_bytes().to_vec(),
        ),
        (
            "source_hash",
            provenance.source_hash().unwrap().as_bytes().to_vec(),
        ),
        ("provenance_bytes", provenance.canonical_bytes().unwrap()),
    ] {
        let actual: Vec<u8> = row.try_get(column).unwrap();
        assert!(
            actual == expected,
            "Rust/SQL canonical {column} bytes differ"
        );
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn encoding_boundaries_non_utc_and_native_direct_reply_match_sql() {
    let mut f = Fixture::new().await;
    sqlx::query("SET LOCAL TIME ZONE 'Pacific/Chatham'")
        .execute(&mut *f.tx)
        .await
        .unwrap();
    let root = f.event(0, None).await;
    let child = f.event(1, Some((root, 0, root, 0, 1))).await;
    // Native first reply uses only an e/reply marker. Uppercase hexadecimal
    // reference text must still resolve to the lowercase canonical identity.
    let controls = "\u{1}\u{8}\t\n\u{c}\r\u{1f}Ö İ 界 🧭 \"\\\u{2028}";
    let tags = json!([
        ["h", f.channel.to_string()],
        ["e", root.to_hex().to_uppercase(), "", "reply"],
        ["x", controls]
    ]);
    let content = format!(
        "{controls}{}",
        "\u{1}".repeat(MAX_CONVERSATION_SOURCE_BYTES - controls.len())
    );
    assert_eq!(content.len(), MAX_CONVERSATION_SOURCE_BYTES);
    sqlx::query("UPDATE events SET tags=$3,content=$4 WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(child.as_bytes().as_slice())
        .bind(tags)
        .bind(&content)
        .execute(&mut *f.tx)
        .await
        .unwrap();
    let thread = f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .unwrap();
    assert_eq!(thread.audience().thread_root().unwrap().event_id(), root);
    assert_eq!(thread.provenance().source().created_at(), f.time(1));
    assert!(
        String::from_utf8(thread.provenance().canonical_bytes().unwrap())
            .unwrap()
            .contains("2026-09-06T11:59:59.123456Z")
    );
    assert!(f
        .resolve(child, ConversationAudienceKind::Channel)
        .await
        .is_some());

    // Build the exact tag boundary from PostgreSQL's own encoded representation
    // because that is the production cap, not Rust's compact serialization size.
    let prefix = json!([
        ["h", f.channel.to_string()],
        ["e", root.to_hex().to_uppercase(), "", "reply"]
    ]);
    sqlx::query("UPDATE events SET tags=$3::jsonb||jsonb_build_array(jsonb_build_array('x',repeat('q',16384-octet_length(($3::jsonb||'[[\"x\",\"\"]]'::jsonb)::text)))) WHERE community_id=$1 AND id=$2")
        .bind(f.community).bind(child.as_bytes().as_slice()).bind(prefix)
        .execute(&mut *f.tx).await.unwrap();
    let tag_size: i32 = sqlx::query_scalar(
        "SELECT octet_length(tags::text) FROM events WHERE community_id=$1 AND id=$2",
    )
    .bind(f.community)
    .bind(child.as_bytes().as_slice())
    .fetch_one(&mut *f.tx)
    .await
    .unwrap();
    assert_eq!(tag_size as usize, MAX_CONVERSATION_TAG_BYTES);
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_some());

    sqlx::query("UPDATE events SET content=content||'x' WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(child.as_bytes().as_slice())
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    sqlx::query("UPDATE events SET content=$3,tags=jsonb_set(tags,'{2,1}',to_jsonb((tags#>>'{2,1}')||'q')) WHERE community_id=$1 AND id=$2")
        .bind(f.community).bind(child.as_bytes().as_slice()).bind(content).execute(&mut *f.tx).await.unwrap();
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.tx.rollback().await.unwrap();
}
