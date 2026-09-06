use super::*;
pub(super) const CANARY: &str = "PRIVATE_ROUTING_CANARY";
pub(super) fn path(channel: Uuid, message: EventId) -> String {
    format!("/api/v1/channels/{channel}/messages/{message}/routing")
}
pub(crate) async fn read(
    app: &Router,
    key: &Keys,
    channel: Uuid,
    message: EventId,
) -> (StatusCode, Value) {
    response(app, signed(key, "GET", &path(channel, message), "", false)).await
}
pub(super) async fn source(f: &Fixture, channel: Uuid) -> EventId {
    let event = EventBuilder::new(Kind::Custom(9), "Private source content")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(&f.operator)
        .unwrap();
    let created = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,$6,$7,$8)")
        .bind(f.community).bind(event.id.to_bytes().as_slice()).bind(event.pubkey.to_bytes().as_slice()).bind(created)
        .bind(serde_json::to_value(&event.tags).unwrap()).bind(&event.content).bind(event.sig.serialize().as_slice()).bind(channel)
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at) VALUES($1,$2,$3,9,$4,$5,'decided',now())")
        .bind(f.company).bind(event.id.to_bytes().as_slice()).bind(created).bind(event.pubkey.to_bytes().as_slice()).bind(channel)
        .execute(&f.pool).await.unwrap();
    event.id
}
pub(crate) async fn record(f: &Fixture, channel: Uuid, wake: bool) -> EventId {
    let message = source(f, channel).await;
    // Historical NULL admission pins are deliberately inert. This fixture tests
    // only production signed projection; actual dispatch uses the control suite.
    let id:Uuid=sqlx::query_scalar("INSERT INTO routing_decisions(company_id,message_id,root_message_id,inbox_claim_generation,origin_type,mode,summary_reason,policy_version,policy_fingerprint,input_hash,excluded_targets,scorer_adapter,scorer_model,scorer_prompt_version,scorer_version,scorer_latency_ms,scorer_usage,wake_count,hop_consumed)
        VALUES($1,$2,$2,0,'human',$3,$4,'ortak-routing-v1',$5,$6,$7,'hermes-codex','gpt-5.6-sol','relevance-v1','ortak-hermes-semantic-v1',123,$8,$9,$10) RETURNING id")
        .bind(f.company).bind(message.to_bytes().as_slice()).bind(if wake {"semantic"} else {"silent"})
        .bind(if wake {"semantic_match"} else {"no_relevant_employee"}).bind(format!("sha256:{}","a".repeat(64))).bind([0_u8;32].as_slice())
        .bind(json!([{"target":CANARY,"reason":"unknown_target"}]))
        .bind(json!({"reasoning_effort":"high","cache_hit":false,"prompt_tokens":20,"completion_tokens":"credential://secret",
            "failure_code":CANARY,"provider_body":CANARY,"request":CANARY,"binding_sha256":CANARY}))
        .bind(if wake {1_i32} else {0}).bind(wake).fetch_one(&f.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO employees(company_id,id) VALUES($1,'hidden-private') ON CONFLICT DO NOTHING",
    )
    .bind(f.company)
    .execute(&f.pool)
    .await
    .unwrap();
    for (position, employee) in ["cem", "hidden-private"].into_iter().enumerate() {
        sqlx::query("INSERT INTO routing_recipients(company_id,routing_decision_id,employee_id,position,action,reason,score,evidence)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(f.company).bind(id).bind(employee).bind(position as i16)
            .bind(if wake && position==0 {"wake"} else {"drop"})
            .bind(if wake && position==0 {"semantic_match"} else {"below_semantic_threshold"})
            .bind(if wake {0.9_f32} else {0.1_f32}).bind(json!(["domain_match",CANARY,"credential://secret"]))
            .execute(&f.pool).await.unwrap();
    }
    message
}
