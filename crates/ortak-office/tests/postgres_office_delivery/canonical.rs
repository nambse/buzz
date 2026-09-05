use super::*;
use ortak_control::inbox::InboxEvent;
use ortak_control::ports::{InboxRepository, MessageNormalizer, Normalization, RoutingRepository};
use ortak_control::routing::{
    CandidateRevision, RosterScope, RoutingCommitOutcome, RoutingProposal,
};
use ortak_control::MessageId;
use ortak_domain::{
    EmployeeId, RecipientAction, RecipientDecision, RoutingDecision, RoutingMode, RoutingReason,
};

pub(super) async fn insert_canonical_run(
    pool: &PgPool,
    company_id: Uuid,
    revision_id: Uuid,
    content: &str,
) -> Uuid {
    let control = PgControlPlane::new(pool.clone());
    let community: Uuid =
        sqlx::query_scalar("SELECT community_id FROM office_company_bindings WHERE company_id=$1")
            .bind(company_id)
            .fetch_one(pool)
            .await
            .expect("community");
    let scope = control
        .resolve_company_for_community(community)
        .await
        .expect("scope");
    let key: Vec<u8> = sqlx::query_scalar(
        "SELECT public_key FROM employee_office_bindings WHERE company_id=$1 AND revision_id=$2",
    )
    .bind(company_id)
    .bind(revision_id)
    .fetch_one(pool)
    .await
    .expect("employee key");
    let channel = Uuid::new_v4();
    sqlx::query("INSERT INTO channels(community_id,id,name,created_by) VALUES ($1,$2,$3,$4)")
        .bind(community)
        .bind(channel)
        .bind(format!("source-{channel}"))
        .bind([7u8; 32].as_slice())
        .execute(pool)
        .await
        .expect("channel");
    for member in [key, [7u8; 32].to_vec()] {
        sqlx::query(
            "INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES ($1,$2,$3)",
        )
        .bind(community)
        .bind(channel)
        .bind(member)
        .execute(pool)
        .await
        .expect("canonical channel member");
    }
    let mut raw = [0u8; 32];
    raw[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    raw[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    let id = MessageId::from_bytes(raw);
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
        VALUES ($1,$2,$3,$4,9,'[]','Cem, please reply',$5,$6)",
    )
    .bind(community)
    .bind(id.as_bytes().as_slice())
    .bind([7u8; 32].as_slice())
    .bind(now)
    .bind([9u8; 64].as_slice())
    .bind(channel)
    .execute(pool)
    .await
    .expect("canonical accepted event");
    control
        .insert_accepted_event(
            community,
            &InboxEvent {
                event_id: id,
                event_kind: 9,
                event_created_at: now,
                author_pubkey: [7; 32],
                channel_id: Some(channel),
            },
        )
        .await
        .expect("canonical inbox");
    let claim = control
        .claim_message(&scope, id, "office-fixture", Duration::from_secs(60), 5)
        .await
        .expect("claim")
        .expect("available");
    let snapshot = control
        .routing_snapshot(&scope, id)
        .await
        .expect("snapshot")
        .expect("inbox");
    let Normalization::Message(normalized) = ortak_office::PgChannelNormalizer::new(pool.clone())
        .normalize(&scope, &snapshot.inbox)
        .await
        .expect("canonical normalization")
    else {
        panic!("normalization refused")
    };
    let hash = ortak_control::service::office_input_hash(
        &normalized.envelope,
        normalized.root_message_id,
        &normalized.eligible_employee_ids,
    );
    let employee = EmployeeId::parse("cem").expect("id");
    assert!(normalized.eligible_employee_ids.contains(&employee));
    let policy = RoutingPolicy::default();
    let proposal = RoutingProposal {
        company_id,
        office_authority: snapshot.office_authority,
        office_input_hash: hash,
        message_id: id,
        root_message_id: id,
        claim_generation: claim.claim_generation,
        origin: normalized.envelope.origin,
        input_hash: [3; 32],
        candidates: vec![CandidateRevision {
            employee_id: employee.clone(),
            revision_id,
        }],
        roster_scope: RosterScope::Targets,
        eligible_employee_ids: normalized.eligible_employee_ids,
        decision: RoutingDecision {
            message_id: id.to_hex(),
            mode: RoutingMode::Deterministic,
            summary_reason: RoutingReason::StructuredDispatch,
            policy_version: policy.version.clone(),
            policy_fingerprint: policy.fingerprint(),
            recipients: vec![RecipientDecision {
                employee_id: employee,
                action: RecipientAction::Wake,
                reason: RoutingReason::StructuredDispatch,
                score: None,
                evidence: vec![],
            }],
        },
        scorer: None,
    };
    let RoutingCommitOutcome::Committed(decision) = control
        .commit_routing(&scope, &proposal)
        .await
        .expect("production routing commit")
    else {
        panic!("not committed")
    };
    let run_id:Uuid=sqlx::query_scalar("INSERT INTO runs(company_id,employee_id,employee_revision_id,routing_decision_id,message_id,root_message_id,
        runtime_adapter,status,delivery_intent,started_at,finished_at) VALUES ($1,'cem',$2,$3,$4,$4,'fake-runtime','completed','reply',clock_timestamp(),clock_timestamp()) RETURNING id")
        .bind(company_id).bind(revision_id).bind(decision.decision_id).bind(id.as_bytes().as_slice()).fetch_one(pool).await.expect("canonical completed run");
    // These tests start at the Office seam. Freeze exactly the canonical draft
    // that the runtime output scheduler supplies, under its production guards.
    let facts = serde_json::json!({"employee_id":"cem","employee_revision_id":revision_id,
        "routing_decision_id":decision.decision_id,"message_id":id.to_hex(),"root_message_id":id.to_hex(),
        "delivery_intent":"reply","office_input_hash":hex::encode(hash)});
    let tags = serde_json::json!([["h", channel.to_string()], ["e", id.to_hex(), "", "reply"]]);
    let mut tx = pool.begin().await.expect("freeze fixture draft");
    let witness = ortak_control::postgres::lock_office_authority_on(&mut tx, &scope)
        .await
        .expect("Office fence");
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company_id)
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .expect("run lock");
    sqlx::query("UPDATE runtime_office_outputs SET draft_kind=9,draft_tags=$3,draft_content=$4,
        draft_created_at=clock_timestamp(),source_facts=$5,office_authority_generation=$6,
        office_authority_valid_before=$7,office_authority_token=$8 WHERE company_id=$1 AND run_id=$2")
        .bind(company_id).bind(run_id).bind(tags).bind(content).bind(facts).bind(witness.generation())
        .bind(witness.valid_before()).bind(Uuid::new_v4()).execute(&mut *tx).await.expect("canonical frozen draft");
    tx.commit().await.expect("freeze committed");
    run_id
}
