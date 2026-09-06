//! Source-only regression fixture: disposable encrypted execution SQL and the
//! existing explicit synthetic env key. No actual provider or deployment.
use super::*;
use futures_util::{SinkExt, StreamExt};
use ortak_control::confidential::PayloadPurpose;
use ortak_office::encrypted::key_provider::DmKeySelection;
use ortak_runtime::cancellation::RuntimeCancellationRepository;
use ortak_runtime::{
    confidential::{seal, ConfidentialMasterKey},
    encrypted::{EncryptedExecution, ExecutionProgress},
    hermes::HermesAdapter,
    postgres::confidential::PgConfidentialExecution,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "execution/late_ack.rs"]
mod late_ack;
#[path = "execution/unbound.rs"]
mod unbound;
#[path = "execution/deletion.rs"]
mod deletion;

fn provider(x: &EncryptedFixture) -> EnvDmKeyProvider {
    EnvDmKeyProvider::new(vec![DmOfficeKeyBinding {
        signer: OfficeSignerBinding {
            company_id: x.f.scope.company_id(),
            employee_id: x.f.employee.id.clone(),
            signer_ref: x.pair.decrypt_ref.clone(),
            public_key: x.pair.employee_public_key,
            secret_env: "ORTAK_TEST_CONFIDENTIAL_SYNTHETIC_KEY".into(),
        },
        office_binding_id: x.pair.office_binding_id,
        key_version: 0,
        purposes: vec![
            OfficeKeyPurpose::WrapMaster,
            OfficeKeyPurpose::UnwrapMaster,
            OfficeKeyPurpose::DmDecrypt,
            OfficeKeyPurpose::DmSeal,
        ],
    }])
    .unwrap()
}
enum Reply {
    Absent,
    Lost,
    Json(Value),
}
async fn server(
    replies: Vec<(&'static str, Reply)>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut paths = Vec::new();
        for (expected, response) in replies {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let work = async {
                let mut bytes = Vec::new();
                let boundary = loop {
                    let mut chunk = [0u8; 2048];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() <= 128 * 1024);
                    if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
                let first = head.lines().next().unwrap();
                assert!(first.contains(expected));
                let size = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                assert!(size <= 112 * 1024);
                while bytes.len() < boundary + size {
                    let mut chunk = [0u8; 2048];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() <= 128 * 1024);
                }
                if size > 0 {
                    let body: Value =
                        serde_json::from_slice(&bytes[boundary..boundary + size]).unwrap();
                    if expected == "POST /v1/confidential/runs " {
                        assert_eq!(body["keys"].as_object().unwrap().len(), 2);
                        assert!(body.get("master").is_none());
                    } else {
                        assert!(body.get("keys").is_none());
                    }
                }
                paths.push(first.to_owned());
                let (status, body) = match response {
                    Reply::Lost => return,
                    Reply::Absent => ("404 Not Found", b"{}".to_vec()),
                    Reply::Json(v) => ("200 OK", serde_json::to_vec(&v).unwrap()),
                };
                let headers=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(3), work)
                .await
                .unwrap();
        }
        paths
    });
    (format!("http://{addr}"), task)
}
async fn admitted(x: &EncryptedFixture) -> Uuid {
    let outer = x.outer(x.rumor()).await;
    let claim = x.accept(&outer).await;
    // Exercises the newly purpose-selected production decoder, not a test parser.
    assert_eq!(
        provider(x)
            .decrypt_claim(&claim)
            .unwrap()
            .source()
            .outer_id(),
        outer.id
    );
    let protected = x.prepare(&claim).await;
    x.protected
        .commit(&x.f.scope, &claim, &protected)
        .await
        .unwrap()
        .run_id
}
async fn relay() -> (
    ortak_office::encrypted::publish::EncryptedDmPublisher,
    tokio::task::JoinHandle<Vec<Vec<u8>>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("ws://{address}/");
    let expected_origin = origin.clone();
    let task = tokio::spawn(async move {
        let mut frozen = Vec::new();
        for attempt in 0..3 {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let work = async {
                let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        "[\"AUTH\",\"synthetic-challenge\"]".into(),
                    ))
                    .await
                    .unwrap();
                let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let auth: Value = serde_json::from_str(&message).unwrap();
                assert_eq!(auth[0], "AUTH");
                let auth = Event::from_json(serde_json::to_vec(&auth[1]).unwrap()).unwrap();
                auth.verify().unwrap();
                assert_eq!(auth.kind.as_u16(), 22242);
                let tags: Vec<_> = auth.tags.iter().map(|t| t.as_slice().to_vec()).collect();
                assert!(tags.contains(&vec!["relay".to_owned(), expected_origin.clone()]));
                assert!(tags.contains(&vec![
                    "challenge".to_owned(),
                    "synthetic-challenge".to_owned()
                ]));
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        json!(["OK", auth.id.to_hex(), true, ""]).to_string().into(),
                    ))
                    .await
                    .unwrap();
                let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let event: Value = serde_json::from_str(&message).unwrap();
                assert_eq!(event[0], "EVENT");
                let event = Event::from_json(serde_json::to_vec(&event[1]).unwrap()).unwrap();
                event.verify().unwrap();
                frozen.push(message.as_bytes().to_vec());
                if attempt != 0 {
                    socket
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            json!(["OK", event.id.to_hex(), true, ""])
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                }
            };
            tokio::time::timeout(Duration::from_secs(3), work)
                .await
                .unwrap();
        }
        frozen
    });
    (
        ortak_office::encrypted::publish::EncryptedDmPublisher::new(origin.parse().unwrap())
            .unwrap(),
        task,
    )
}

#[tokio::test]
#[ignore = "disposable encrypted execution SQL + explicit synthetic key env; finite loopback HTTP"]
async fn encrypted_execution_recovers_lost_start_and_freezes_exact_two_copy_reply() {
    let x = EncryptedFixture::new().await;
    let run = admitted(&x).await;
    let keys = provider(&x);
    let mut tx = x.f.pool.begin().await.unwrap();
    let current = PgConfidentialRuns::load_current_on(&mut tx, &x.f.scope, run)
        .await
        .unwrap()
        .unwrap();
    let id = current.identity().clone();
    let selected = DmKeySelection::from_expected_claims(&id, current.signer_ref().clone());
    let master = ConfidentialMasterKey::from_owned(
        keys.unwrap_master(&selected, current.wrapped_master())
            .unwrap()
            .into_owned(),
    );
    tx.rollback().await.unwrap();
    let identity: Value = serde_json::from_slice(id.canonical_bytes()).unwrap();
    let at = Utc
        .timestamp_opt(Utc::now().timestamp(), 0)
        .single()
        .unwrap();
    let reference = format!("ortak:{}:{run}", x.f.scope.company_id());
    let receipt = json!({"runtime_run_ref":reference,"started_at":at,"status":"completed"});
    let payloads = [
        json!({"event_type":"run.started","runtime_run_ref":reference}),
        json!({"event_type":"assistant.delta","turn":0,"delta":{"text":"Protected answer\nİ 🧭"}}),
        json!({"event_type":"delivery.intent","intent":"reply"}),
        json!({"event_type":"run.completed","delivery_intent":"reply"}),
    ];
    let envelopes:Vec<_>=payloads.iter().enumerate().map(|(i,p)|{
        let ordinal=i as u32+1;let inner=json!({"format":"ortak-confidential-event/1","identity":identity,"sequence":ordinal,"occurred_at":at,"payload":p});
        seal(&master,&id,PayloadPurpose::RuntimeEvent,ordinal,&serde_json::to_vec(&inner).unwrap()).unwrap()
    }).collect();
    let events:Vec<_>=envelopes.iter().enumerate().map(|(i,e)|json!({"cursor":(i+1).to_string(),"occurred_at":at,"envelope":serde_json::from_slice::<Value>(e.canonical_bytes()).unwrap()})).collect();
    let batch = json!({"events":events,"status":"completed","failure":null,"terminal":true});
    let (origin, http) = server(vec![
        ("POST /v1/confidential/runs/lookup ", Reply::Absent),
        ("POST /v1/confidential/runs ", Reply::Lost),
        (
            "POST /v1/confidential/runs/lookup ",
            Reply::Json(receipt.clone()),
        ),
        ("POST /v1/confidential/runs/replay ", Reply::Json(receipt)),
        ("/events?after=0&limit=4", Reply::Json(batch)),
    ])
    .await;
    let adapter = HermesAdapter::new(x.f.scope.company_id(), &origin, "synthetic-bearer").unwrap();
    let repo = PgConfidentialExecution::new(x.f.pool.clone());
    let execute = EncryptedExecution::new(&x.f.scope, &repo, &adapter, &keys);
    assert_eq!(
        execute.dispatch_once().await.unwrap(),
        ExecutionProgress::Deferred
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        execute.dispatch_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    assert_eq!(
        execute.observe_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        execute.seal_reply_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    let paths = http.await.unwrap();
    assert_eq!(
        paths
            .iter()
            .filter(|p| p.contains("POST /v1/confidential/runs "))
            .count(),
        1
    );
    let retained:Vec<Vec<u8>>=sqlx::query_scalar("SELECT envelope_bytes FROM confidential_run_payloads WHERE company_id=$1 AND run_id=$2 AND purpose='runtime_event' ORDER BY ordinal").bind(x.f.scope.company_id()).bind(run).fetch_all(&x.f.pool).await.unwrap();
    assert_eq!(
        retained,
        envelopes
            .iter()
            .map(|e| e.canonical_bytes().to_vec())
            .collect::<Vec<_>>()
    );
    let row=sqlx::query("SELECT recipient_bytes,history_bytes FROM confidential_reply_bundles WHERE company_id=$1 AND run_id=$2").bind(x.f.scope.company_id()).bind(run).fetch_one(&x.f.pool).await.unwrap();
    let recipient =
        Event::from_json(row.try_get::<Vec<u8>, _>("recipient_bytes").unwrap()).unwrap();
    let history = Event::from_json(row.try_get::<Vec<u8>, _>("history_bytes").unwrap()).unwrap();
    let a = nostr::nips::nip59::UnwrappedGift::from_gift_wrap(&x.human, &recipient)
        .await
        .unwrap();
    let b = nostr::nips::nip59::UnwrappedGift::from_gift_wrap(&x.employee, &history)
        .await
        .unwrap();
    assert_eq!(a.rumor, b.rumor);
    assert_eq!(a.rumor.content, "Protected answer\nİ 🧭");
    let counts:(i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM confidential_reply_outbox WHERE company_id=$1 AND run_id=$2 AND state='pending'),(SELECT count(*) FROM run_events WHERE company_id=$1),(SELECT count(*) FROM run_context_snapshots WHERE company_id=$1),(SELECT count(*) FROM runtime_office_outputs WHERE company_id=$1)")
        .bind(x.f.scope.company_id()).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(counts, (2, 0, 0, 0));
    assert!(x
        .f
        .control
        .run_cursor_state(&x.f.scope, run)
        .await
        .unwrap()
        .is_none());
    late_ack::prove(&x, run).await;
    let (publisher, relay) = relay().await;
    assert_eq!(
        execute.publish_once(&publisher).await.unwrap(),
        ExecutionProgress::Deferred
    );
    assert_eq!(
        execute.publish_once(&publisher).await.unwrap(),
        ExecutionProgress::Recorded
    );
    let partial:Vec<(i32,String,i32)>=sqlx::query_as("SELECT copy,state,attempts FROM confidential_reply_outbox WHERE company_id=$1 AND run_id=$2 ORDER BY copy")
        .bind(x.f.scope.company_id()).bind(run).fetch_all(&x.f.pool).await.unwrap();
    assert_eq!(
        partial,
        vec![(0, "pending".into(), 1), (1, "acked".into(), 1)]
    );
    tokio::time::sleep(Duration::from_millis(5100)).await;
    assert_eq!(
        execute.publish_once(&publisher).await.unwrap(),
        ExecutionProgress::Recorded
    );
    let transmitted = relay.await.unwrap();
    assert_eq!(transmitted[0], transmitted[2]);
    assert_ne!(transmitted[0], transmitted[1]);
    let acked:i64=sqlx::query_scalar("SELECT count(*) FROM confidential_reply_outbox WHERE company_id=$1 AND run_id=$2 AND state='acked'")
        .bind(x.f.scope.company_id()).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(acked, 2);
}

#[tokio::test]
#[ignore = "disposable encrypted execution SQL + explicit synthetic key env; keyless loopback cancel"]
async fn encrypted_revocation_isolates_ordinary_claims_and_preserves_keyless_stop() {
    let x = EncryptedFixture::new().await;
    let run = admitted(&x).await;
    x.remove_human(true).await;
    x.protected.cancel(&x.f.scope, run).await.unwrap();
    assert!(x
        .f
        .control
        .run_cursor_state(&x.f.scope, run)
        .await
        .unwrap()
        .is_none());
    assert!(x
        .f
        .control
        .claim_cancellations(&x.f.scope, "hermes", Duration::from_secs(5), 1)
        .await
        .unwrap()
        .is_empty());
    let(origin,http)=server(vec![("POST /v1/confidential/runs/cancel ",Reply::Json(json!({"runtime_run_ref":format!("ortak:{}:{run}",x.f.scope.company_id()),"outcome":"cancelled"})))]).await;
    let adapter = HermesAdapter::new(x.f.scope.company_id(), &origin, "synthetic-bearer").unwrap();
    let repo = PgConfidentialExecution::new(x.f.pool.clone());
    // This provider cannot unwrap or decrypt and names a separate unavailable
    // test env leaf. Keyless cancellation must not resolve any key at all.
    let keys = EnvDmKeyProvider::new(vec![DmOfficeKeyBinding {
        signer: OfficeSignerBinding {
            company_id: x.f.scope.company_id(),
            employee_id: x.f.employee.id.clone(),
            signer_ref: x.pair.decrypt_ref.clone(),
            public_key: x.pair.employee_public_key,
            secret_env: "ORTAK_TEST_CONFIDENTIAL_UNAVAILABLE_KEY".into(),
        },
        office_binding_id: x.pair.office_binding_id,
        key_version: 0,
        purposes: vec![OfficeKeyPurpose::DmSeal],
    }])
    .unwrap();
    let execute = EncryptedExecution::new(&x.f.scope, &repo, &adapter, &keys);
    assert_eq!(
        execute.dispatch_once().await.unwrap(),
        ExecutionProgress::Idle
    );
    assert_eq!(
        execute.observe_once().await.unwrap(),
        ExecutionProgress::Recorded
    );
    assert_eq!(http.await.unwrap().len(), 1);
    let state:(String,String)=sqlx::query_as("SELECT c.state,x.state FROM runtime_cancellations c JOIN confidential_execution_leases x USING(company_id,run_id) WHERE c.company_id=$1 AND c.run_id=$2")
        .bind(x.f.scope.company_id()).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(state, ("acknowledged".into(), "stopped".into()));
}
