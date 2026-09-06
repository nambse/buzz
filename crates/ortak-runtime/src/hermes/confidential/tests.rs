use super::*;
use crate::confidential::{ConfidentialMasterKey, prepare_start_body, seal};
use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

fn fixture() -> (Value, ValidatedIdentity, ConfidentialMasterKey) {
    let vector: Value = serde_json::from_str(include_str!(
        "../../../../ortak-control/src/confidential/vector.json"
    ))
    .unwrap();
    let identity = ValidatedIdentity::parse(
        vector["expected"]["identity_utf8"]
            .as_str()
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let master = ConfidentialMasterKey::from_owned(Zeroizing::new(
        hex::decode(vector["master_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
    ));
    (vector, identity, master)
}

#[test]
fn protected_start_body_contains_only_two_exact_derived_keys_and_bound_snapshot() {
    let (v, identity, master) = fixture();
    let snapshot =
        ConfidentialEnvelope::parse(v["expected"]["envelope_utf8"].as_str().unwrap().as_bytes())
            .unwrap();
    let body = prepare_start_body(&master, &identity, &snapshot).unwrap();
    let parsed: Value = serde_json::from_slice(&body.bytes).unwrap();
    assert_eq!(parsed.as_object().unwrap().len(), 3);
    assert_eq!(parsed["keys"].as_object().unwrap().len(), 2);
    assert_eq!(
        STANDARD
            .decode(parsed["keys"]["snapshot"].as_str().unwrap())
            .unwrap(),
        hex::decode(v["expected"]["derived_key_hex"].as_str().unwrap()).unwrap()
    );
    assert_ne!(parsed["keys"]["snapshot"], parsed["keys"]["runtime_event"]);
    assert_eq!(
        parsed["snapshot"],
        serde_json::from_slice::<Value>(snapshot.canonical_bytes()).unwrap()
    );
    let wrong = seal(&master, &identity, PayloadPurpose::RuntimeEvent, 1, b"{}").unwrap();
    assert!(prepare_start_body(&master, &identity, &wrong).is_err());
    let changed = identity.canonical_bytes().to_vec();
    let changed = String::from_utf8(changed)
        .unwrap()
        .replace("\"authority_epoch\":\"3\"", "\"authority_epoch\":\"4\"");
    assert!(
        prepare_start_body(
            &master,
            &ValidatedIdentity::parse(changed.as_bytes()).unwrap(),
            &snapshot
        )
        .is_err()
    );
}

#[test]
fn protected_event_page_rejects_gaps_swaps_and_fabricated_terminal_status() {
    let (_, identity, master) = fixture();
    let envelope = seal(&master, &identity, PayloadPurpose::RuntimeEvent, 1, b"{}").unwrap();
    let good = json!({"events":[{"cursor":"1","occurred_at":"2026-09-06T00:00:00Z",
        "envelope":serde_json::from_slice::<Value>(envelope.canonical_bytes()).unwrap()}],
        "status":"running","failure":null,"terminal":false});
    let parse =
        |v: &Value| serde_json::from_slice::<WireBatch>(&serde_json::to_vec(v).unwrap()).unwrap();
    assert!(validate_batch(&identity, 0, parse(&good)).is_ok());
    for mode in 0..5 {
        let mut value = good.clone();
        match mode {
            0 => value["events"][0]["cursor"] = json!("2"),
            1 => {
                value["events"][0]["envelope"]["header"]["identity"]["authority_epoch"] = json!("4")
            }
            2 => value["terminal"] = json!(true),
            3 => value["status"] = json!("failed"),
            _ => {
                value["status"] = json!("cancelled");
                value["failure"] =
                    json!({"code":"provider_failed","occurred_at":"2026-09-06T00:00:00Z"});
            }
        }
        assert!(validate_batch(&identity, 0, parse(&value)).is_err());
    }
}

#[tokio::test]
#[ignore = "explicit bounded loopback protected HTTP contract"]
async fn protected_http_uses_distinct_routes_and_keyless_recovery() {
    let (_, identity, master) = fixture();
    let selected: SelectedIdentity = serde_json::from_slice(identity.canonical_bytes()).unwrap();
    let snapshot = seal(&master, &identity, PayloadPurpose::Snapshot, 0, b"{}").unwrap();
    let body = prepare_start_body(&master, &identity, &snapshot).unwrap();
    let expected_body: Value = serde_json::from_slice(&body.bytes).unwrap();
    let event = seal(&master, &identity, PayloadPurpose::RuntimeEvent, 1, b"{}").unwrap();
    let reference = format!("ortak:{}:{}", selected.company_id, selected.run_id);
    let receipt = json!({"runtime_run_ref":reference,"started_at":"2026-09-06T00:00:00Z","status":"accepted"});
    let responses = [
        receipt.clone(),
        receipt.clone(),
        receipt,
        json!({"events":[{"cursor":"1","occurred_at":"2026-09-06T00:00:00Z","envelope":serde_json::from_slice::<Value>(event.canonical_bytes()).unwrap()}],"status":"running","failure":null,"terminal":false}),
        json!({"runtime_run_ref":reference,"outcome":"cancelled"}),
    ];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut observed = Vec::new();
        for response in responses {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let work = async {
                let mut bytes = Vec::new();
                let boundary = loop {
                    let mut chunk = [0; 2048];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() <= 128 * 1024);
                    if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
                assert!(
                    headers
                        .to_lowercase()
                        .contains("authorization: bearer synthetic-transport-token")
                );
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|n| n.parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                assert!(length <= 112 * 1024);
                while bytes.len() < boundary + length {
                    let mut chunk = [0; 2048];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let request = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap()
                };
                observed.push((headers.lines().next().unwrap().to_string(), request));
                let data = serde_json::to_vec(&response).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()).as_bytes()).await.unwrap();
                socket.write_all(&data).await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(3), work)
                .await
                .unwrap();
        }
        observed
    });
    let adapter = HermesAdapter::new(
        selected.company_id,
        &format!("http://{address}"),
        "synthetic-transport-token",
    )
    .unwrap();
    let key = crate::run_idempotency_key(selected.company_id, selected.run_id);
    adapter.start_confidential(body).await.unwrap();
    adapter
        .replay_confidential(&identity, &snapshot)
        .await
        .unwrap()
        .unwrap();
    adapter.lookup_confidential(&key).await.unwrap().unwrap();
    assert_eq!(
        adapter
            .confidential_events(&identity, 0)
            .await
            .unwrap()
            .events
            .len(),
        1
    );
    adapter.cancel_confidential(&key).await.unwrap();
    let observed = server.await.unwrap();
    assert_eq!(observed[0].0, "POST /v1/confidential/runs HTTP/1.1");
    assert_eq!(observed[0].1, expected_body);
    assert_eq!(observed[1].0, "POST /v1/confidential/runs/replay HTTP/1.1");
    assert_eq!(observed[2].0, "POST /v1/confidential/runs/lookup HTTP/1.1");
    assert_eq!(observed[4].0, "POST /v1/confidential/runs/cancel HTTP/1.1");
    for (_, request) in &observed[1..] {
        assert!(request.get("keys").is_none());
        assert!(request.get("spec").is_none());
    }
}
