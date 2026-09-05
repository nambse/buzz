//! Memory projection is exercised only through signed production HTTP routes.
use super::*;

const PRIVATE_PROMPT: &str = "PRIVATE_SNAPSHOT_PROMPT_CANARY";
const PRIVATE_CONFIG: &str = "PRIVATE_RUNTIME_CONFIG_CANARY";
const PRIVATE_CREDENTIAL_REF: &str = "credential://private-snapshot-only/canary";
const SECRET: &str = "sk-live-abcdef1234567890";

/// Add real relational pins to the parent fixture's intentionally minimal run.
/// The full stored wire contains private configuration to falsify broad JSON
/// serialization, but only its narrow source/provenance projection is public.
async fn snapshot(f: &Fixture, run: Uuid, channel: Uuid) -> Value {
    let message: Vec<u8> =
        sqlx::query_scalar("SELECT message_id FROM runs WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .expect("run message");
    sqlx::query("INSERT INTO delivery_chains (company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes,hop_count,wake_count) VALUES ($1,$2,'api-memory-v1',$3,8,64,1,1)")
        .bind(f.company).bind(&message).bind(format!("sha256:{}", "0".repeat(64)))
        .execute(&f.pool).await.expect("root counters and limits");
    let decision = Uuid::new_v4();
    sqlx::query("INSERT INTO routing_decisions (company_id,id,message_id,root_message_id,inbox_claim_generation,origin_type,origin_id,mode,summary_reason,policy_version,policy_fingerprint,input_hash,office_input_hash,wake_count,hop_consumed,chain_hop_count,chain_wake_count) VALUES ($1,$2,$3,$3,0,'human','api-test','deterministic','structured_dispatch','api-memory-v1',$4,$5,$5,1,true,1,1)")
        .bind(f.company).bind(decision).bind(&message)
        .bind(format!("sha256:{}", "0".repeat(64))).bind([0u8;32].as_slice())
        .execute(&f.pool).await.expect("snapshot decision");
    sqlx::query("INSERT INTO routing_recipients (company_id,routing_decision_id,employee_id,position,action,reason,employee_revision_id) VALUES ($1,$2,'cem',0,'wake','structured_dispatch',$3)")
        .bind(f.company).bind(decision).bind(f.revision).execute(&f.pool).await.expect("snapshot recipient");
    sqlx::query("INSERT INTO delivery_chain_visits (company_id,root_message_id,employee_id,routing_decision_id,recipient_action,batch_hop) VALUES ($1,$2,'cem',$3,'wake',1)")
        .bind(f.company).bind(&message).bind(decision).execute(&f.pool).await.expect("employee root reservation");
    sqlx::query("UPDATE runs SET routing_decision_id=$3,root_message_id=message_id WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(run).bind(decision).execute(&f.pool).await.expect("pin source");
    let message = hex::encode(message);
    let now = Utc::now().to_rfc3339();
    json!({
        "version": 1,
        "company_id": f.company,
        "routing_decision_id": decision,
        "message_id": message,
        "root_message_id": message,
        "event_kind": 9,
        "input_truncated": false,
        "memory_binding": {
            "adapter":"honcho", "endpoint_ref":PRIVATE_CONFIG,
            "workspace":"private-test", "user_peer":"operator", "employee_peer":"cem", "options":{}
        },
        "recall": { "truncated": true, "records": [
            { "record_ref":"record-one", "scope":{"scope":"run_scratch","run_id":run},
              "content":format!("A scoped fact; token={SECRET}\u{0}"),
              "provenance":{"employee_id":"cem","run_id":run,"source":"run_scratch","recorded_at":now} },
            { "record_ref":"record-two", "scope":{"scope":"run_scratch","run_id":run},
              "content":"An already [redacted] scoped fact.",
              "provenance":{"employee_id":"cem","run_id":run,"source":"run_scratch","recorded_at":now} }
        ]},
        "spec": {
            "run_id":run, "employee_id":"cem", "revision_id":f.revision,
            "binding":{"adapter":"hermes","profile_ref":PRIVATE_CONFIG,"model":PRIVATE_CONFIG,
                "workspace_ref":PRIVATE_CONFIG,"credential_refs":[PRIVATE_CREDENTIAL_REF],"options":{}},
            "permissions":{"private_test_marker":PRIVATE_CONFIG},
            "input":PRIVATE_PROMPT,
            "context":{"conversation_ref":channel,"reply_to_message_id":message,"work_item_id":null,
                "memory_context":[PRIVATE_PROMPT]},
            "idempotency_key":format!("ortak-run:{}:{run}",f.company)
        }
    })
}

async fn store(f: &Fixture, run: Uuid, value: &Value, correct_hash: bool) {
    let bytes = serde_json::to_vec(value).expect("snapshot bytes");
    let hash = if correct_hash {
        Sha256::digest(&bytes).to_vec()
    } else {
        vec![0u8; 32]
    };
    sqlx::query("INSERT INTO run_context_snapshots(company_id,run_id,spec_bytes,spec_hash) VALUES ($1,$2,$3,$4)")
        .bind(f.company).bind(run).bind(bytes).bind(hash).execute(&f.pool).await.expect("immutable snapshot");
}

/// Seed only persisted projection rows; the separate runtime PG tests exercise
/// the actual delivery-to-memory scheduling service and immutable receipt path.
async fn pending_write(f: &Fixture, run: Uuid, wire: &Value, channel: Uuid) {
    let content = format!("Published scoped answer; token={SECRET}");
    let event = EventBuilder::new(Kind::Custom(9), content.clone())
        .tags([Tag::parse(["h", &channel.to_string()]).expect("channel tag")])
        .sign_with_keys(&Keys::generate())
        .expect("fresh signed fixture event");
    sqlx::query("UPDATE runs SET status='completed',delivery_intent='reply',finished_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(run).execute(&f.pool).await.expect("terminal projection fixture");
    let outbox = Uuid::new_v4();
    sqlx::query("INSERT INTO outbox(company_id,id,kind,dedup_key,run_id,state,signed_event_id,signed_event_bytes,delivered_at) VALUES ($1,$2,'office_publish',$3,$4,'delivered',$5,$6,clock_timestamp())")
        .bind(f.company).bind(outbox).bind(format!("projection:{run}")).bind(run)
        .bind(event.id.to_bytes().as_slice()).bind(serde_json::to_vec(&event).expect("signed bytes"))
        .execute(&f.pool).await.expect("acknowledged Office projection row");
    let source = json!({"employee_id":"cem","employee_revision_id":f.revision,
        "routing_decision_id":wire["routing_decision_id"],"message_id":wire["message_id"],
        "root_message_id":wire["root_message_id"],"delivery_intent":"reply",
        "office_input_hash":"0".repeat(64)});
    sqlx::query("INSERT INTO runtime_memory_writes(company_id,run_id,employee_id,employee_revision_id,channel_id,outbox_id,signed_event_id,binding,source_facts,content,recorded_at,idempotency_key) VALUES ($1,$2,'cem',$3,$4,$5,$6,$7,$8,$9,clock_timestamp(),$10)")
        .bind(f.company).bind(run).bind(f.revision).bind(channel).bind(outbox)
        .bind(event.id.to_bytes().as_slice()).bind(&wire["memory_binding"]).bind(source)
        .bind(content).bind(format!("office-output:{run}"))
        .execute(&f.pool).await.expect("pending immutable memory write projection");
}

async fn read(f: &Fixture, keys: &Keys, run: Uuid) -> (StatusCode, Value) {
    response(
        &f.app,
        signed(keys, "GET", &format!("/api/v1/runs/{run}"), "", false),
    )
    .await
}

fn unavailable(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body, &json!({"error":{"code":"service_unavailable"}}));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn signed_memory_projection_is_bounded_redacted_audience_scoped_and_fail_closed() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let (status, body) = read(&f, &f.operator, run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["memory"],
        json!({"scope":"run_scratch","run_id":run,
        "recall":{"status":"not_prepared","records":[],"truncated":false,"prepared_at":null},"write":null})
    );

    let wire = snapshot(&f, run, f.channel).await;
    store(&f, run, &wire, true).await;
    let (status, body) = read(&f, &f.reader, run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let recall = &body["memory"]["recall"];
    assert_eq!(recall["status"], "prepared");
    assert_eq!(recall["truncated"], true);
    assert!(recall["prepared_at"].is_string());
    let records = recall["records"].as_array().expect("records");
    assert_eq!(records.len(), 2);
    for record in records {
        assert_eq!(record["source"], "run_scratch");
        assert_eq!(
            record["content"]["redacted"], true,
            "canonical persisted redaction is visible"
        );
        assert_eq!(record["content"]["truncated"], false);
        assert!(record["content"]["text"]
            .as_str()
            .expect("text")
            .contains("[redacted]"));
    }
    let response_text = body.to_string();
    for private in [
        SECRET,
        PRIVATE_PROMPT,
        PRIVATE_CONFIG,
        PRIVATE_CREDENTIAL_REF,
        "credential_refs",
    ] {
        assert!(
            !response_text.contains(private),
            "private snapshot data leaked: {private}"
        );
    }
    assert!(body["memory"].get("spec").is_none());
    assert!(body["memory"].get("memory_binding").is_none());

    let hidden = f.run(f.hidden).await;
    let hidden_wire = snapshot(&f, hidden, f.hidden).await;
    store(&f, hidden, &hidden_wire, true).await;
    let foreign = Fixture::new().await;
    let foreign_run = foreign.run(foreign.channel).await;
    let foreign_wire = snapshot(&foreign, foreign_run, foreign.channel).await;
    store(&foreign, foreign_run, &foreign_wire, false).await;
    for denied in [hidden, foreign_run] {
        let (status, body) = read(&f, &f.operator, denied).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "audience check must precede snapshot decoding: {body}"
        );
        assert_eq!(body, json!({"error":{"code":"not_found"}}));
    }
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice())
        .execute(&f.pool).await.expect("revoke current membership");
    let (status, body) = read(&f, &f.operator, run).await;
    assert!(
        matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
        "{body}"
    );
    assert!(body.get("memory").is_none());
    assert_eq!(read(&f, &f.reader, run).await.0, StatusCode::OK);

    // A run retargeted into the reader's visible channel must not expose the
    // snapshot frozen for its previous private channel.
    sqlx::query("UPDATE runs SET message_id=(SELECT message_id FROM runs WHERE company_id=$1 AND id=$3),root_message_id=(SELECT message_id FROM runs WHERE company_id=$1 AND id=$3) WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(hidden).bind(run).execute(&f.pool).await.expect("retarget hidden run");
    let (status, body) = read(&f, &f.reader, hidden).await;
    unavailable(status, &body);

    let write_run = f.run(f.channel).await;
    let write_wire = snapshot(&f, write_run, f.channel).await;
    pending_write(&f, write_run, &write_wire, f.channel).await;
    let (status, body) = read(&f, &f.reader, write_run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["memory"]["recall"]["status"], "not_prepared");
    assert_eq!(body["memory"]["write"]["status"], "pending");
    assert_eq!(body["memory"]["write"]["content"]["redacted"], true);
    assert!(body["memory"]["write"]["receipt"].is_null());
    for private in [SECRET, PRIVATE_CONFIG, PRIVATE_CREDENTIAL_REF] {
        assert!(
            !body.to_string().contains(private),
            "private write projection data leaked"
        );
    }
    // Same visible channel and employee, but a different original message. Only
    // the immutable write source-facts guard can prevent this historical leak.
    sqlx::query("UPDATE runs SET message_id=(SELECT message_id FROM runs WHERE company_id=$1 AND id=$3),root_message_id=(SELECT message_id FROM runs WHERE company_id=$1 AND id=$3) WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(write_run).bind(run).execute(&f.pool).await.expect("retarget write source");
    let (status, body) = read(&f, &f.reader, write_run).await;
    unavailable(status, &body);

    for case in 0..5 {
        let malformed = f.run(f.channel).await;
        let mut wire = snapshot(&f, malformed, f.channel).await;
        match case {
            0 => {} // Hash mismatch even when the body has a valid shape.
            1 => wire["company_id"] = json!(Uuid::new_v4()),
            2 => wire["spec"]["run_id"] = json!(Uuid::new_v4()),
            3 => {
                let template = wire["recall"]["records"][0].clone();
                wire["recall"]["records"] = json!((0..9)
                    .map(|index| {
                        let mut record = template.clone();
                        record["record_ref"] = json!(format!("record-{index}"));
                        record
                    })
                    .collect::<Vec<_>>());
            }
            _ => wire["spec"]["revision_id"] = json!(Uuid::new_v4()),
        }
        store(&f, malformed, &wire, case != 0).await;
        let (status, body) = read(&f, &f.reader, malformed).await;
        unavailable(status, &body);
    }
}
