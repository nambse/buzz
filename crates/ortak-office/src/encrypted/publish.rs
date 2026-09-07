//! One bounded NIP-42 WebSocket publication. No history subscription, unrelated
//! frame queue, ambient credentials, automatic retry or raw frame logging.
use super::key_provider::{DmKeySelection, EnvDmKeyProvider};
use crate::encrypted::{wire, MAX_OUTER_BYTES};
use futures_util::{SinkExt, StreamExt};
use nostr::{EventId, JsonUtil, RelayUrl};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};
use url::Url;

/// Exact configured relay origin. Construction performs no network operation.
pub struct EncryptedDmPublisher {
    origin: Url,
    relay: RelayUrl,
}
/// Closed publication errors never include challenge, endpoint response or bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DmPublishError {
    /// Invalid selected origin or frozen signed outer.
    #[error("encrypted DM publication refused")]
    Refused,
    /// Bounded network/authentication/publication attempt failed or expired.
    #[error("encrypted DM publication unavailable")]
    Unavailable,
}
impl EncryptedDmPublisher {
    /// Requires WSS or explicitly selected loopback WS and a bare origin.
    pub fn new(origin: Url) -> Result<Self, DmPublishError> {
        if !matches!(origin.scheme(), "ws" | "wss")
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || origin.scheme() == "ws"
                && !matches!(
                    origin.host_str(),
                    Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
                )
        {
            return Err(DmPublishError::Refused);
        }
        let relay = RelayUrl::parse(origin.as_str()).map_err(|_| DmPublishError::Refused)?;
        Ok(Self { origin, relay })
    }
    /// Sends the exact already-frozen outer and requires its matching accepted
    /// OK. Caller holds current authority/deadline and persists each copy's ACK.
    /// A timeout leaves an uncertain result: the next lease reuses these bytes.
    pub async fn publish(
        &self,
        keys: &EnvDmKeyProvider,
        selection: &DmKeySelection,
        ordinal: u8,
        event_id: &[u8; 32],
        bytes: &[u8],
        budget: std::time::Duration,
    ) -> Result<(), DmPublishError> {
        if budget.is_zero() || bytes.len() > MAX_OUTER_BYTES {
            return Err(DmPublishError::Refused);
        }
        keys.validate_reply_copy(selection, ordinal, bytes)
            .map_err(|_| DmPublishError::Refused)?;
        let event =
            wire::signed(bytes, MAX_OUTER_BYTES, 1059).map_err(|_| DmPublishError::Refused)?;
        if event.id.to_bytes() != *event_id {
            return Err(DmPublishError::Refused);
        }
        let task = async {
            let config = WebSocketConfig::default()
                .max_message_size(Some(8192))
                .max_frame_size(Some(8192))
                .write_buffer_size(0)
                .max_write_buffer_size(128 * 1024);
            let (mut socket, _) =
                connect_async_with_config(self.origin.as_str(), Some(config), false)
                    .await
                    .map_err(|_| DmPublishError::Unavailable)?;
            let mut auth_id: Option<EventId> = None;
            let mut authenticated = false;
            let mut sent = false;
            // No more than 16 frames, including pings/notices. No unrelated
            // frame is retained and there is no unbounded reconnect path.
            for _ in 0..16 {
                if authenticated && !sent {
                    let mut body = Vec::with_capacity(bytes.len() + 12);
                    body.extend_from_slice(b"[\"EVENT\",");
                    body.extend_from_slice(bytes);
                    body.push(b']');
                    let body = String::from_utf8(body).map_err(|_| DmPublishError::Refused)?;
                    socket
                        .send(Message::Text(body.into()))
                        .await
                        .map_err(|_| DmPublishError::Unavailable)?;
                    sent = true;
                }
                let frame = socket
                    .next()
                    .await
                    .ok_or(DmPublishError::Unavailable)?
                    .map_err(|_| DmPublishError::Unavailable)?;
                let text = match frame {
                    Message::Text(t) => t,
                    Message::Ping(p) => {
                        socket
                            .send(Message::Pong(p))
                            .await
                            .map_err(|_| DmPublishError::Unavailable)?;
                        continue;
                    }
                    _ => return Err(DmPublishError::Unavailable),
                };
                let value: serde_json::Value =
                    serde_json::from_str(&text).map_err(|_| DmPublishError::Unavailable)?;
                let values = value.as_array().ok_or(DmPublishError::Unavailable)?;
                match values.first().and_then(|v| v.as_str()) {
                    Some("AUTH") if values.len() == 2 && auth_id.is_none() => {
                        let challenge = values[1].as_str().ok_or(DmPublishError::Unavailable)?;
                        let auth = keys
                            .auth_challenge(selection, &self.relay, challenge)
                            .map_err(|_| DmPublishError::Refused)?;
                        auth_id = Some(auth.id);
                        socket
                            .send(Message::Text(
                                format!("[\"AUTH\",{}]", auth.as_json()).into(),
                            ))
                            .await
                            .map_err(|_| DmPublishError::Unavailable)?;
                    }
                    Some("OK") if values.len() == 4 => {
                        let id = values[1].as_str().ok_or(DmPublishError::Unavailable)?;
                        if values[2].as_bool() != Some(true) || values[3].as_str().is_none() {
                            return Err(DmPublishError::Unavailable);
                        }
                        if !authenticated && auth_id.is_some_and(|v| v.to_hex() == id) {
                            authenticated = true;
                        } else if sent && id == hex::encode(event_id) {
                            // Drop closes the owned socket even if a peer would
                            // stall the WebSocket close handshake.
                            return Ok(());
                        } else {
                            return Err(DmPublishError::Unavailable);
                        }
                    }
                    Some("NOTICE") if values.len() == 2 => {}
                    _ => return Err(DmPublishError::Unavailable),
                }
            }
            Err(DmPublishError::Unavailable)
        };
        tokio::time::timeout(budget.min(std::time::Duration::from_secs(5)), task)
            .await
            .map_err(|_| DmPublishError::Unavailable)?
    }
}
