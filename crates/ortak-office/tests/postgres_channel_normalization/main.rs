//! Production-seam Postgres tests for the channel `MessageNormalizer`, the
//! disabled semantic scorer, and the DM refusal decision, driven through
//! `InboxRoutingService` over `PgControlPlane`.
//!
//! Run with a disposable database that can receive the embedded migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-office --test postgres_channel_normalization -- --ignored`

mod cohort;
#[path = "../../../ortak-control/tests/cohort_support.rs"]
mod cohort_support;
mod direct;
mod fencing;
mod fixture;

use ortak_control::inbox::InboxEvent;
use ortak_control::inbox::{InboxInsertOutcome, InboxState};
use ortak_control::ports::{InboxRepository, RoutingRepository};
use ortak_control::routing::CommittedDecision;
use ortak_control::{ControlError, ServiceOutcome};
use ortak_domain::{RecipientAction, RoutingMode, RoutingReason};
use uuid::Uuid;

use fixture::{
    add_channel_member, generated_key, EventSpec, Fixture, KIND_GIFT_WRAP, KIND_STREAM_MESSAGE,
    KIND_STREAM_MESSAGE_V2,
};

fn committed(outcome: ServiceOutcome) -> CommittedDecision {
    match outcome {
        ServiceOutcome::Committed(decision) => decision,
        other => panic!("expected a committed decision, got {other:?}"),
    }
}

fn assert_silent(decision: &CommittedDecision, reason: RoutingReason) {
    assert_eq!(decision.mode, RoutingMode::Silent);
    assert_eq!(decision.summary_reason, reason);
    assert_eq!(decision.wake_count, 0);
    assert!(!decision.hop_consumed);
    assert!(decision.dispatches.is_empty());
    assert!(decision
        .recipients
        .iter()
        .all(|recipient| recipient.action == RecipientAction::Drop));
}

fn tags_with_mention(channel_id: Uuid, mentioned: &[u8; 32]) -> serde_json::Value {
    serde_json::json!([["h", channel_id.to_string()], ["p", hex::encode(mentioned)]])
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn direct_name_from_a_channel_member_wakes_only_cem() {
    let fixture = Fixture::new().await;

    let (event, outcome) = fixture.route_human_text("Cem, selam nasılsın?").await;
    let decision = committed(outcome);
    assert_eq!(decision.mode, RoutingMode::Deterministic);
    assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
    assert_eq!(decision.wake_count, 1);
    assert_eq!(decision.recipients.len(), 1);
    assert_eq!(decision.recipients[0].employee_id.as_str(), "cem");
    assert_eq!(decision.recipients[0].action, RecipientAction::Wake);
    assert_eq!(decision.dispatches.len(), 1);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 1);

    let (origin_type, origin_id, root) = fixture.decision_provenance(event.id).await;
    assert_eq!(origin_type, "human");
    assert_eq!(
        origin_id.as_deref(),
        Some(hex::encode(fixture.human_key).as_str())
    );
    assert_eq!(root, event.id, "a human message roots its own chain");

    // The v2 channel kind takes the same path.
    let v2 = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE_V2,
            author: fixture.human_key,
            content: "@cem bir bakar mısın?",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&v2, KIND_STREAM_MESSAGE_V2, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&v2).await.expect("route v2"));
    assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn untargeted_human_message_records_a_disabled_scorer_silence() {
    let fixture = Fixture::new().await;

    let (event, outcome) = fixture
        .route_human_text("Herkese merhaba, bugün ne yapıyoruz?")
        .await;
    let decision = committed(outcome);
    assert_silent(&decision, RoutingReason::SemanticScorerDisabled);
    assert_eq!(
        decision.recipients.len(),
        2,
        "both eligible candidates are explained"
    );
    assert!(decision.recipients.iter().all(|recipient| {
        recipient.reason == RoutingReason::SemanticScorerDisabled && recipient.score.is_none()
    }));
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);

    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, event.id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert_eq!(stored.mode, RoutingMode::Silent);
    assert_eq!(stored.scorer_adapter.as_deref(), Some("disabled"));
    assert_eq!(stored.wake_count, 0);
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM delivery_chain_visits WHERE company_id = $1 AND root_message_id = $2",
                event.id
            )
            .await,
        0
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn gift_wrap_dm_is_refused_once_with_no_dispatch_and_no_replay_duplicate() {
    let fixture = Fixture::new().await;
    let wrap_key = generated_key();
    let event = fixture
        .store_event(EventSpec {
            kind: KIND_GIFT_WRAP,
            author: wrap_key,
            content: "AtN3f0ciphertextThatMustNeverBeRead==",
            tags: serde_json::json!([["p", hex::encode(fixture.cem_key)]]),
            channel_id: None,
            parent: None,
        })
        .await;
    fixture.accept(&event, KIND_GIFT_WRAP, wrap_key).await;

    let decision = committed(fixture.route(&event).await.expect("route wrap"));
    assert_silent(&decision, RoutingReason::DmNormalizationPending);
    assert!(
        decision.recipients.is_empty(),
        "a refusal names no candidate"
    );
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                event.id
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM runs WHERE company_id = $1 AND message_id = $2",
                event.id
            )
            .await,
        0
    );
    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, event.id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert_eq!(stored.summary_reason, RoutingReason::DmNormalizationPending);
    assert_eq!(stored.scorer_adapter, None, "no scorer ran for a refusal");
    assert!(stored.candidate_revision_ids.is_empty());
    // The outer wrap key is a transport artifact: it is attributed as a
    // closed integration label, never described as a verified human.
    let (origin_type, origin_id, root) = fixture.decision_provenance(event.id).await;
    assert_eq!(origin_type, "integration");
    assert_eq!(
        origin_id.as_deref(),
        Some(format!("gift-wrap-transport:{}", hex::encode(wrap_key)).as_str())
    );
    assert_eq!(root, event.id);

    // Replay: the inbox row is terminal, cannot be reclaimed, and the
    // service finds nothing due, so a second decision is impossible.
    let replay = fixture
        .control
        .insert_accepted_event(
            fixture.community_id,
            &InboxEvent {
                event_id: event.id,
                event_created_at: event.created_at,
                event_kind: KIND_GIFT_WRAP,
                author_pubkey: wrap_key,
                channel_id: None,
            },
        )
        .await
        .expect("replay insert");
    assert_eq!(replay, InboxInsertOutcome::AlreadyPresent);
    let inbox = fixture
        .control
        .inbox_row(&fixture.scope, event.id)
        .await
        .expect("read inbox")
        .expect("inbox row");
    assert_eq!(inbox.state, InboxState::Decided);
    assert!(fixture
        .service()
        .claim_and_route(&fixture.scope)
        .await
        .expect("route")
        .is_none());
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                event.id
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM outbox WHERE company_id = $1 AND $2::bytea IS NOT NULL",
                event.id
            )
            .await,
        0
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn historical_employee_key_is_an_employee_origin_with_persisted_root_and_never_a_human() {
    let fixture = Fixture::new().await;

    // Hop 1: the human wakes Cem.
    let (root, outcome) = fixture
        .route_human_text("Cem, bunu Zeynep ile planla")
        .await;
    let hop_one = committed(outcome);
    assert_eq!(hop_one.dispatches[0].employee_id, "cem");

    // Cem's reply is signed with a retired, unverified key that also sits
    // in the relay roster: if bindings were not consulted first, this key
    // would pass as a human and "Zeynep, ..." would start a fresh chain.
    let retired_key = fixture
        .add_retired_binding("cem", fixture.cem_revision)
        .await;
    fixture.add_relay_member(&retired_key).await;

    let reply = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: retired_key,
            content: "Zeynep, mobil tarafı sen devral",
            tags: tags_with_mention(fixture.channel_id, &fixture.zeynep_key),
            channel_id: Some(fixture.channel_id),
            parent: Some(&root),
        })
        .await;
    fixture
        .record_published_run(
            hop_one.decision_id,
            "cem",
            fixture.cem_revision,
            &root,
            &root,
            &reply,
        )
        .await;
    fixture
        .accept(&reply, KIND_STREAM_MESSAGE, retired_key)
        .await;

    let hop_two = committed(fixture.route(&reply).await.expect("route reply"));
    assert_eq!(hop_two.mode, RoutingMode::Deterministic);
    assert_eq!(hop_two.summary_reason, RoutingReason::StructuredMention);
    assert_eq!(hop_two.wake_count, 1);
    assert_eq!(hop_two.dispatches[0].employee_id, "zeynep");
    assert_eq!(
        hop_two.chain.hop_count, 2,
        "the reply extends the human root chain"
    );
    assert_eq!(hop_two.chain.wake_count, 2);
    let (origin_type, origin_id, decided_root) = fixture.decision_provenance(reply.id).await;
    assert_eq!(origin_type, "employee");
    assert_eq!(origin_id.as_deref(), Some("cem"));
    assert_eq!(
        decided_root, root.id,
        "root comes from runs.root_message_id"
    );

    // The same historical key without persisted publish provenance is an
    // employee whose root cannot be trusted: explicit refusal, no wake, even
    // with a raw vocative and a key mention for Zeynep.
    let orphan = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: retired_key,
            content: "Zeynep, hemen başla",
            tags: tags_with_mention(fixture.channel_id, &fixture.zeynep_key),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&orphan, KIND_STREAM_MESSAGE, retired_key)
        .await;
    let refused = committed(fixture.route(&orphan).await.expect("route orphan"));
    assert_silent(&refused, RoutingReason::UnresolvedProvenance);
    assert_eq!(fixture.run_dispatch_rows(orphan.id).await, 0);
    let (origin_type, origin_id, _) = fixture.decision_provenance(orphan.id).await;
    assert_eq!(origin_type, "employee");
    assert_eq!(origin_id.as_deref(), Some("cem"));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn reply_parent_resolves_in_the_same_channel_and_fails_closed_elsewhere() {
    let fixture = Fixture::new().await;

    // A stored Cem message in the channel (its own routing is irrelevant here).
    let cem_message = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.cem_key,
            content: "Taslak hazır, yorumlarınızı bekliyorum.",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;

    // Same channel, server-persisted parent, no key mention: reply routing.
    let reply = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Teşekkürler, ikinci bölümü genişletir misin?",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: Some(&cem_message),
        })
        .await;
    fixture
        .accept(&reply, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&reply).await.expect("route reply"));
    assert_eq!(decision.summary_reason, RoutingReason::ReplyToEmployee);
    assert_eq!(decision.dispatches.len(), 1);
    assert_eq!(decision.dispatches[0].employee_id, "cem");

    // Cross-channel parent: a persisted parent in another channel of the
    // same community is refused, not followed.
    let other_channel: Uuid = sqlx::query_scalar(
        "INSERT INTO channels (community_id, name, created_by) VALUES ($1, 'elsewhere', $2) RETURNING id",
    )
    .bind(fixture.community_id)
    .bind(fixture.human_key.as_slice())
    .fetch_one(&fixture.pool)
    .await
    .expect("other channel");
    let elsewhere = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.cem_key,
            content: "Başka kanaldan mesaj",
            tags: serde_json::json!([["h", other_channel.to_string()]]),
            channel_id: Some(other_channel),
            parent: None,
        })
        .await;
    let cross_channel = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Buna cevap veriyorum",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: Some(&elsewhere),
        })
        .await;
    fixture
        .accept(&cross_channel, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let refused = committed(fixture.route(&cross_channel).await.expect("route"));
    assert_silent(&refused, RoutingReason::UnresolvedProvenance);
    assert_eq!(fixture.run_dispatch_rows(cross_channel.id).await, 0);

    // Cross-company parent: the parent id belongs to another community's
    // event store, so it does not resolve inside this company at all.
    let foreign = Fixture::new().await;
    let foreign_message = foreign
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: foreign.cem_key,
            content: "Yabancı şirket mesajı",
            tags: serde_json::json!([["h", foreign.channel_id.to_string()]]),
            channel_id: Some(foreign.channel_id),
            parent: None,
        })
        .await;
    let cross_company = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, bunu gördün mü?",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .store_thread_parent(
            &cross_company.id,
            cross_company.created_at,
            fixture.channel_id,
            &foreign_message,
        )
        .await;
    fixture
        .accept(&cross_company, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let refused = committed(fixture.route(&cross_company).await.expect("route"));
    assert_silent(&refused, RoutingReason::UnresolvedProvenance);
    assert_eq!(
        fixture.run_dispatch_rows(cross_company.id).await,
        0,
        "even a leading vocative cannot wake Cem when provenance is unresolved"
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn spoofed_tags_and_unknown_authors_never_wake_or_escape_scope() {
    let fixture = Fixture::new().await;
    let other_root = fixture::message_id();

    // System, dispatch, assignment, and root claims in tags are ignored: the
    // message is an ordinary untargeted human text with its own root.
    let spoofed = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "merhaba",
            tags: serde_json::json!([
                ["h", fixture.channel_id.to_string()],
                ["e", other_root.to_hex(), "", "root"],
                ["origin", "system"],
                ["dispatch", "zeynep"],
                ["assign", "zeynep"],
                ["p", hex::encode(fixture.human_key)],
            ]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&spoofed, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&spoofed).await.expect("route"));
    assert_silent(&decision, RoutingReason::SemanticScorerDisabled);
    let (origin_type, _, root) = fixture.decision_provenance(spoofed.id).await;
    assert_eq!(origin_type, "human");
    assert_eq!(root, spoofed.id);
    assert_ne!(root, other_root);

    // A client reply marker without a relay-persisted parent is refused.
    let fake_parent = fixture::message_id();
    let claimed_reply = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, cevabına dönüyorum",
            tags: serde_json::json!([
                ["h", fixture.channel_id.to_string()],
                ["e", fake_parent.to_hex(), "", "reply"],
                ["p", hex::encode(fixture.cem_key)],
            ]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&claimed_reply, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let refused = committed(fixture.route(&claimed_reply).await.expect("route"));
    assert_silent(&refused, RoutingReason::UnresolvedProvenance);
    assert_eq!(fixture.run_dispatch_rows(claimed_reply.id).await, 0);

    // A key that is neither an employee binding nor a channel/relay member
    // is refused before any content-derived routing.
    let stranger = generated_key();
    let unknown = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: stranger,
            content: "Cem, benimle konuş",
            tags: tags_with_mention(fixture.channel_id, &fixture.cem_key),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&unknown, KIND_STREAM_MESSAGE, stranger)
        .await;
    let refused = committed(fixture.route(&unknown).await.expect("route"));
    assert_silent(&refused, RoutingReason::UnknownOrigin);
    assert_eq!(fixture.run_dispatch_rows(unknown.id).await, 0);

    // Once the same key is a channel member, the identical message routes.
    add_channel_member(
        &fixture.pool,
        fixture.community_id,
        fixture.channel_id,
        &stranger,
    )
    .await;
    let member_now = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: stranger,
            content: "Cem, benimle konuş",
            tags: tags_with_mention(fixture.channel_id, &fixture.cem_key),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&member_now, KIND_STREAM_MESSAGE, stranger)
        .await;
    let decision = committed(fixture.route(&member_now).await.expect("route"));
    assert_eq!(decision.summary_reason, RoutingReason::StructuredMention);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn inbox_facts_that_disagree_with_the_canonical_event_are_a_typed_retryable_error() {
    let fixture = Fixture::new().await;
    let event = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, selam",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    // The inbox row claims a different kind than the signed row carries.
    fixture
        .accept(&event, KIND_STREAM_MESSAGE_V2, fixture.human_key)
        .await;

    let error = fixture.route(&event).await.expect_err("mismatch must fail");
    assert!(
        matches!(
            &error,
            ControlError::InboxFactMismatch { field: "kind", .. }
        ),
        "expected InboxFactMismatch on kind, got {error:?}"
    );
    let inbox = fixture
        .control
        .inbox_row(&fixture.scope, event.id)
        .await
        .expect("read inbox")
        .expect("inbox row");
    assert_eq!(inbox.state, InboxState::Pending);
    assert!(inbox.last_error.is_some());
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                event.id
            )
            .await,
        0
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn known_employee_outside_the_channel_is_a_visible_drop_and_never_semantic_fanout() {
    let fixture = Fixture::new().await;
    // Cem is active with a verified binding but left the channel.
    fixture.remove_channel_member(&fixture.cem_key).await;

    // Raw vocative alias for the absent employee: silent, explained, no
    // scorer, no dispatch. Zeynep is not woken by fan-out.
    let (event, outcome) = fixture.route_human_text("Cem, selam nasılsın?").await;
    let decision = committed(outcome);
    assert_silent(&decision, RoutingReason::TargetNotChannelMember);
    assert_eq!(decision.recipients.len(), 1);
    assert_eq!(decision.recipients[0].employee_id.as_str(), "cem");
    assert_eq!(
        decision.recipients[0].reason,
        RoutingReason::TargetNotChannelMember
    );
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, event.id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert_eq!(stored.scorer_adapter, None, "no semantic scoring ran");

    // Structured key mention of the absent employee: same visible drop.
    let mentioned = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "bir bakar mısın?",
            tags: tags_with_mention(fixture.channel_id, &fixture.cem_key),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&mentioned, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&mentioned).await.expect("route mention"));
    assert_silent(&decision, RoutingReason::TargetNotChannelMember);
    assert_eq!(decision.recipients[0].employee_id.as_str(), "cem");
    assert_eq!(fixture.run_dispatch_rows(mentioned.id).await, 0);

    // A mixed message wakes the present employee and explains the absent one.
    let (mixed, outcome) = fixture
        .route_human_text("Cem ve @zeynep, mobil planı konuşalım")
        .await;
    let decision = committed(outcome);
    assert_eq!(decision.mode, RoutingMode::Deterministic);
    assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
    assert_eq!(decision.wake_count, 1);
    assert_eq!(decision.dispatches.len(), 1);
    assert_eq!(decision.dispatches[0].employee_id, "zeynep");
    let cem = decision
        .recipients
        .iter()
        .find(|recipient| recipient.employee_id.as_str() == "cem")
        .expect("cem is explained");
    assert_eq!(cem.action, RecipientAction::Drop);
    assert_eq!(cem.reason, RoutingReason::TargetNotChannelMember);
    assert_eq!(fixture.run_dispatch_rows(mixed.id).await, 1);

    // The semantic roster excludes the absent employee entirely.
    let (untargeted, outcome) = fixture.route_human_text("Herkese merhaba").await;
    let decision = committed(outcome);
    assert_silent(&decision, RoutingReason::SemanticScorerDisabled);
    assert_eq!(decision.recipients.len(), 1);
    assert_eq!(decision.recipients[0].employee_id.as_str(), "zeynep");
    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, untargeted.id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert_eq!(stored.candidate_revision_ids.len(), 1);

    // A retired (expired, unverified) binding for a present key does not
    // make the employee eligible either: the current verified binding must
    // be the member.
    fixture.remove_channel_member(&fixture.zeynep_key).await;
    let zeynep_revision: Uuid = sqlx::query_scalar(
        "SELECT active_revision_id FROM employees WHERE company_id = $1 AND id = 'zeynep'",
    )
    .bind(fixture.company_id())
    .fetch_one(&fixture.pool)
    .await
    .expect("zeynep revision");
    let retired = fixture.add_retired_binding("zeynep", zeynep_revision).await;
    add_channel_member(
        &fixture.pool,
        fixture.community_id,
        fixture.channel_id,
        &retired,
    )
    .await;
    let (event, outcome) = fixture.route_human_text("Zeynep, orada mısın?").await;
    let decision = committed(outcome);
    assert_silent(&decision, RoutingReason::TargetNotChannelMember);
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn legacy_automation_deactivated_users_and_dead_channels_are_refused() {
    let fixture = Fixture::new().await;

    async fn route_from(fixture: &Fixture, author: [u8; 32], content: &str) -> CommittedDecision {
        let event = fixture
            .store_event(EventSpec {
                kind: KIND_STREAM_MESSAGE,
                author,
                content,
                tags: tags_with_mention(fixture.channel_id, &fixture.cem_key),
                channel_id: Some(fixture.channel_id),
                parent: None,
            })
            .await;
        fixture.accept(&event, KIND_STREAM_MESSAGE, author).await;
        let decision = committed(fixture.route(&event).await.expect("route"));
        if decision.mode == RoutingMode::Silent {
            assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
        }
        let (origin_type, origin_id, _) = fixture.decision_provenance(event.id).await;
        match decision.summary_reason {
            RoutingReason::LegacyAutomationOrigin => {
                assert_eq!(origin_type, "integration");
                assert_eq!(
                    origin_id.as_deref(),
                    Some(format!("legacy-automation:{}", hex::encode(author)).as_str())
                );
            }
            _ => assert_eq!(origin_type, "human"),
        }
        decision
    }

    // A live channel member whose membership carries the relay `bot` role is
    // legacy automation, not a human, even with a key mention of Cem.
    let bot = generated_key();
    fixture.add_bot_member(&bot).await;
    let decision = route_from(&fixture, bot, "Cem, şunu yap").await;
    assert_silent(&decision, RoutingReason::LegacyAutomationOrigin);

    // A relay-registered agent (users.agent_owner_pubkey) that is an
    // ordinary channel member is refused the same way.
    let agent = generated_key();
    add_channel_member(
        &fixture.pool,
        fixture.community_id,
        fixture.channel_id,
        &agent,
    )
    .await;
    // The owner must exist as a user before the agent row can reference it.
    fixture.add_user(&fixture.human_key, None, false).await;
    fixture
        .add_user(&agent, Some(&fixture.human_key), false)
        .await;
    let decision = route_from(&fixture, agent, "Cem, şunu yap").await;
    assert_silent(&decision, RoutingReason::LegacyAutomationOrigin);

    // A deactivated human member is refused; a plain human member routes.
    let gone = generated_key();
    add_channel_member(
        &fixture.pool,
        fixture.community_id,
        fixture.channel_id,
        &gone,
    )
    .await;
    fixture.add_user(&gone, None, true).await;
    let decision = route_from(&fixture, gone, "Cem, şunu yap").await;
    assert_silent(&decision, RoutingReason::OriginDeactivated);
    let decision = route_from(&fixture, fixture.human_key, "Cem, şunu yap").await;
    assert_eq!(decision.summary_reason, RoutingReason::StructuredMention);
    assert_eq!(decision.wake_count, 1);

    // Oversized mention sets are refused outright, not truncated.
    let mut tags = vec![serde_json::json!(["h", fixture.channel_id.to_string()])];
    for index in 0..=ortak_office::normalizer::MAX_MENTION_KEYS {
        tags.push(serde_json::json!(["p", format!("{index:064x}")]));
    }
    tags.push(serde_json::json!(["p", hex::encode(fixture.cem_key)]));
    let oversized = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, herkesi etiketledim",
            tags: serde_json::Value::Array(tags),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&oversized, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&oversized).await.expect("route"));
    assert_silent(&decision, RoutingReason::TagBoundsExceeded);
    assert_eq!(fixture.run_dispatch_rows(oversized.id).await, 0);

    // A reply whose persisted parent was deleted is not a reply to anyone.
    let parent = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.cem_key,
            content: "Silinecek mesaj",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    sqlx::query("UPDATE events SET deleted_at = now() WHERE community_id = $1 AND id = $2")
        .bind(fixture.community_id)
        .bind(parent.id.as_bytes().as_slice())
        .execute(&fixture.pool)
        .await
        .expect("delete parent");
    let reply = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Buna cevap",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: Some(&parent),
        })
        .await;
    fixture
        .accept(&reply, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    let decision = committed(fixture.route(&reply).await.expect("route"));
    assert_silent(&decision, RoutingReason::UnresolvedProvenance);

    // Archived, then DM-typed, channels are not routable text channels.
    fixture
        .set_channel_state(
            "UPDATE channels SET archived_at = now() WHERE community_id = $1 AND id = $2",
        )
        .await;
    let (event, outcome) = fixture.route_human_text("Cem, arşivde misin?").await;
    assert_silent(&committed(outcome), RoutingReason::ChannelNotRoutable);
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
    fixture
        .set_channel_state(
            "UPDATE channels SET archived_at = NULL, channel_type = 'dm'
              WHERE community_id = $1 AND id = $2",
        )
        .await;
    let (event, outcome) = fixture.route_human_text("Cem, özelden yazıyorum").await;
    assert_silent(&committed(outcome), RoutingReason::ChannelNotRoutable);
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn private_channel_requires_live_membership_of_the_author_and_open_channels_do_not() {
    let fixture = Fixture::new().await;
    // The human is a relay member as well as a channel member, so leaving the
    // channel keeps the key a known identity without channel access.
    fixture.add_relay_member(&fixture.human_key).await;
    fixture.set_visibility("private").await;

    // A live member of the private channel routes normally.
    let (root, outcome) = fixture.route_human_text("Cem, bunu planla").await;
    let hop_one = committed(outcome);
    assert_eq!(hop_one.dispatches[0].employee_id, "cem");

    // The message is accepted (queued) while the author is still a member,
    // and the author is removed before the worker routes it: refused, no
    // wake, origin still recorded as the known human.
    let queued = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, hâlâ orada mısın?",
            tags: tags_with_mention(fixture.channel_id, &fixture.cem_key),
            channel_id: Some(fixture.channel_id),
            parent: None,
        })
        .await;
    fixture
        .accept(&queued, KIND_STREAM_MESSAGE, fixture.human_key)
        .await;
    fixture.remove_channel_member(&fixture.human_key).await;
    let refused = committed(fixture.route(&queued).await.expect("route queued"));
    assert_silent(&refused, RoutingReason::OriginNotChannelMember);
    assert!(
        refused.recipients.is_empty(),
        "a refusal names no candidate"
    );
    assert_eq!(fixture.run_dispatch_rows(queued.id).await, 0);
    let (origin_type, origin_id, _) = fixture.decision_provenance(queued.id).await;
    assert_eq!(origin_type, "human");
    assert_eq!(
        origin_id.as_deref(),
        Some(hex::encode(fixture.human_key).as_str())
    );

    // Open-channel control: the same non-member relay member may write, as
    // the relay's ingest gate allows, and the vocative wakes Cem.
    fixture.set_visibility("open").await;
    let (open, outcome) = fixture.route_human_text("Cem, açık kanaldan").await;
    let decision = committed(outcome);
    assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
    assert_eq!(fixture.run_dispatch_rows(open.id).await, 1);

    // Employee authors are held to the same rule: Cem's published reply
    // with trusted provenance is refused once Cem has left the private
    // channel, even with a key mention of Zeynep.
    fixture.set_visibility("private").await;
    let reply = fixture
        .store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.cem_key,
            content: "Zeynep, mobil tarafı sen devral",
            tags: tags_with_mention(fixture.channel_id, &fixture.zeynep_key),
            channel_id: Some(fixture.channel_id),
            parent: Some(&root),
        })
        .await;
    fixture
        .record_published_run(
            hop_one.decision_id,
            "cem",
            fixture.cem_revision,
            &root,
            &root,
            &reply,
        )
        .await;
    fixture
        .accept(&reply, KIND_STREAM_MESSAGE, fixture.cem_key)
        .await;
    fixture.remove_channel_member(&fixture.cem_key).await;
    let refused = committed(fixture.route(&reply).await.expect("route reply"));
    assert_silent(&refused, RoutingReason::OriginNotChannelMember);
    assert_eq!(fixture.run_dispatch_rows(reply.id).await, 0);
    let (origin_type, origin_id, decided_root) = fixture.decision_provenance(reply.id).await;
    assert_eq!(origin_type, "employee");
    assert_eq!(origin_id.as_deref(), Some("cem"));
    assert_eq!(
        decided_root, reply.id,
        "a refusal is rooted at its own id and never extends the human chain"
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM delivery_chain_visits WHERE company_id = $1 AND root_message_id = $2",
                root.id
            )
            .await,
        1,
        "only hop one visited the chain"
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn eligibility_follows_the_active_manifest_key_not_the_introducing_revision() {
    let fixture = Fixture::new().await;

    // Revision B keeps the key and signer revision A introduced; provisioning
    // does not rewrite the binding, so its revision_id still names A.
    let revision_b = fixture
        .activate_manifest_revision("cem", |manifest| {
            manifest["biography"] = serde_json::json!("Revised biography, same Office key");
        })
        .await;
    assert_ne!(revision_b, fixture.cem_revision);
    assert_eq!(
        fixture.binding_revision(&fixture.cem_key).await,
        fixture.cem_revision,
        "the binding keeps its introducing revision"
    );
    let (event, outcome) = fixture.route_human_text("Cem, selam nasılsın?").await;
    let decision = committed(outcome);
    assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 1);
    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, event.id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert!(
        stored.candidate_revision_ids.contains(&revision_b)
            && !stored
                .candidate_revision_ids
                .contains(&fixture.cem_revision),
        "the decision pins the active revision, not the introducing one"
    );

    // A revision whose manifest names a key with no matching binding is not
    // eligible, whatever the introduced binding still says.
    fixture
        .activate_manifest_revision("cem", |manifest| {
            manifest["office"]["public_key"] = serde_json::json!(hex::encode(generated_key()));
        })
        .await;
    let (event, outcome) = fixture.route_human_text("Cem, selam nasılsın?").await;
    assert_silent(&committed(outcome), RoutingReason::TargetNotChannelMember);
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);

    // Same key, different signer reference: the binding no longer matches.
    fixture
        .activate_manifest_revision("cem", |manifest| {
            manifest["office"]["public_key"] = serde_json::json!(hex::encode(fixture.cem_key));
            manifest["office"]["signer_ref"] =
                serde_json::json!("credential://ortak-runtime/cem/rotated-signer");
        })
        .await;
    let (event, outcome) = fixture.route_human_text("Cem, selam nasılsın?").await;
    assert_silent(&committed(outcome), RoutingReason::TargetNotChannelMember);
    assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
    assert_eq!(
        fixture.binding_revision(&fixture.cem_key).await,
        fixture.cem_revision,
        "no revision change rewrote the binding"
    );
}
