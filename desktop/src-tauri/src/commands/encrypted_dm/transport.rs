//! One bounded authenticated connection per action. No event logging, detached
//! reader, unbounded queue, HTTP publication or optimistic ACK inference.

use super::{authority::Session, codec, Error, Result};
use futures_util::{SinkExt, StreamExt};
use nostr::JsonUtil;
use serde_json::{json, value::RawValue};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};

pub(super) struct Socket {
    ws: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    frames: usize,
}
impl Socket {
    pub(super) async fn authenticated(session: &Session) -> Result<Self> {
        let config = WebSocketConfig::default()
            .max_message_size(Some(66560))
            .max_frame_size(Some(66560))
            .max_write_buffer_size(262144)
            .write_buffer_size(0);
        let (ws, _) = connect_async_with_config(session.relay.as_str(), Some(config), true)
            .await
            .map_err(|_| Error::Unavailable)?;
        let mut socket = Self { ws, frames: 0 };
        let mut auth_id = None;
        for _ in 0..16 {
            let bytes = socket.next().await?;
            let values = frame(&bytes)?;
            match name(&values)? {
                "AUTH" if auth_id.is_none() && values.len() == 2 => {
                    let challenge: String =
                        serde_json::from_str(values[1].get()).map_err(|_| Error::Encoding)?;
                    if challenge.is_empty() || challenge.len() > 1024 {
                        return Err(Error::Bounds);
                    }
                    let event = buzz_ws_client_pkg::build_auth_event(
                        &challenge,
                        &session.relay,
                        &session.keys,
                        None,
                    )
                    .map_err(|_| Error::Unavailable)?;
                    auth_id = Some(event.id.to_hex());
                    socket
                        .send(format!("[\"AUTH\",{}]", event.as_json()))
                        .await?;
                }
                "OK" if auth_id
                    .as_deref()
                    .is_some_and(|id| accepted(&values, id).is_ok()) =>
                {
                    return Ok(socket)
                }
                "CLOSED" => return Err(Error::Revoked),
                _ => {}
            }
        }
        Err(Error::Unavailable)
    }
    async fn send(&mut self, text: String) -> Result<()> {
        self.ws
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| Error::Unavailable)
    }
    async fn next(&mut self) -> Result<String> {
        loop {
            self.frames += 1;
            if self.frames > 96 {
                return Err(Error::Bounds);
            }
            match tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .map_err(|_| Error::Unavailable)?
                .ok_or(Error::Unavailable)?
                .map_err(|_| Error::Unavailable)?
            {
                Message::Text(text) => return Ok(text.to_string()),
                Message::Ping(data) => self
                    .ws
                    .send(Message::Pong(data))
                    .await
                    .map_err(|_| Error::Unavailable)?,
                Message::Pong(_) => {}
                _ => return Err(Error::Unavailable),
            }
        }
    }
    /// The stored outer string is embedded verbatim, never rebuilt or re-signed.
    pub(super) async fn publish(&mut self, exact_json: &str, expected_id: &str) -> Result<()> {
        let event = codec::outer(exact_json.as_bytes())?;
        if event.id.to_hex() != expected_id {
            return Err(Error::Encoding);
        }
        self.send(format!("[\"EVENT\",{exact_json}]")).await?;
        for _ in 0..16 {
            let bytes = self.next().await?;
            let values = frame(&bytes)?;
            if name(&values)? == "OK"
                && values
                    .get(1)
                    .is_some_and(|v| v.get() == format!("\"{expected_id}\""))
            {
                return accepted(&values, expected_id);
            }
        }
        Err(Error::Unavailable)
    }
    pub(super) async fn read(&mut self, human: &str) -> Result<Vec<String>> {
        let id = uuid::Uuid::new_v4().to_string();
        self.send(json!(["REQ", id, {"kinds":[1059],"#p":[human],"limit":32}]).to_string())
            .await?;
        let mut events = Vec::new();
        for _ in 0..64 {
            let bytes = self.next().await?;
            let values = frame(&bytes)?;
            let selected = values
                .get(1)
                .is_some_and(|v| v.get() == format!("\"{id}\""));
            match name(&values)? {
                "EVENT" if selected && values.len() == 3 => {
                    if events.len() == 32 {
                        return Err(Error::Bounds);
                    }
                    codec::outer(values[2].get().as_bytes())?;
                    events.push(values[2].get().to_owned());
                }
                "EOSE" if selected && values.len() == 2 => {
                    self.send(json!(["CLOSE", id]).to_string()).await?;
                    return Ok(events);
                }
                "CLOSED" if selected => return Err(Error::Revoked),
                _ => {}
            }
        }
        Err(Error::Bounds)
    }
}
fn frame(bytes: &str) -> Result<Vec<&RawValue>> {
    if bytes.len() > 66560 {
        return Err(Error::Bounds);
    }
    let value: Vec<&RawValue> = serde_json::from_str(bytes).map_err(|_| Error::Encoding)?;
    if value.is_empty() || value.len() > 4 {
        return Err(Error::Encoding);
    }
    Ok(value)
}
fn name<'a>(values: &'a [&RawValue]) -> Result<&'a str> {
    serde_json::from_str(values[0].get()).map_err(|_| Error::Encoding)
}
fn accepted(values: &[&RawValue], id: &str) -> Result<()> {
    if values.len() != 4
        || values[0].get() != "\"OK\""
        || values[1].get() != format!("\"{id}\"")
        || values[2].get() != "true"
    {
        return Err(Error::Unavailable);
    }
    let _: &str = serde_json::from_str(values[3].get()).map_err(|_| Error::Encoding)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ack_requires_exact_id_and_boolean_and_preserves_raw_duplicate_keys() {
        assert!(accepted(&frame("[\"OK\",\"a\",true,\"\"]").unwrap(), "a").is_ok());
        for raw in [
            "[\"OK\",\"b\",true,\"\"]",
            "[\"OK\",\"a\",false,\"\"]",
            "[\"OK\",\"a\",\"true\",\"\"]",
        ] {
            assert!(accepted(&frame(raw).unwrap(), "a").is_err());
        }
        let raw = "[\"EVENT\",\"s\",{\"id\":\"a\",\"id\":\"b\"}]";
        assert_eq!(frame(raw).unwrap()[2].get(), "{\"id\":\"a\",\"id\":\"b\"}");
    }

    #[tokio::test]
    async fn actual_nip42_socket_lost_ack_retries_identical_frozen_bytes() {
        let human = nostr::Keys::generate();
        let employee = nostr::Keys::generate();
        let frozen = codec::freeze(
            &human,
            &employee.public_key().to_hex(),
            "socket-only synthetic text",
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let expected = frozen.outer_json[0].clone();
        let id = frozen.outer_ids[0].clone();
        let public = human.public_key();
        let server = tokio::spawn(async move {
            let mut frames = Vec::new();
            for attempt in 0..2 {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(Message::Text("[\"AUTH\",\"bounded-fixture\"]".into()))
                    .await
                    .unwrap();
                let auth = ws.next().await.unwrap().unwrap().into_text().unwrap();
                let values = frame(&auth).unwrap();
                assert_eq!(name(&values).unwrap(), "AUTH");
                let event = nostr::Event::from_json(values[1].get()).unwrap();
                event.verify().unwrap();
                assert_eq!(event.pubkey, public);
                assert_eq!(event.kind.as_u16(), 22242);
                ws.send(Message::Text(
                    json!(["OK", event.id.to_hex(), true, ""])
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
                let outgoing = ws
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .into_text()
                    .unwrap()
                    .to_string();
                assert_eq!(outgoing, format!("[\"EVENT\",{expected}]"));
                frames.push(outgoing);
                if attempt == 1 {
                    ws.send(Message::Text(
                        json!(["OK", id, true, ""]).to_string().into(),
                    ))
                    .await
                    .unwrap();
                }
                // First delivery deliberately loses its ACK without altering bytes.
            }
            frames
        });
        let session = Session::for_transport_test(human, url);
        let first = tokio::time::timeout(Duration::from_secs(3), async {
            let mut socket = Socket::authenticated(&session).await.unwrap();
            socket
                .publish(&frozen.outer_json[0], &frozen.outer_ids[0])
                .await
        })
        .await
        .unwrap();
        assert!(first.is_err());
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut socket = Socket::authenticated(&session).await.unwrap();
            socket
                .publish(&frozen.outer_json[0], &frozen.outer_ids[0])
                .await
                .unwrap();
        })
        .await
        .unwrap();
        let frames = server.await.unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], frames[1]);
    }
}
