//! Native-owned, NIP-98-authenticated current pair selection. IPC supplies no URL
//! or authorization assertion. An absent operator mapping/endpoint is closed.

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use nostr::Keys;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};

use super::{Error, Result};
use crate::app_state::AppState;

/// Current canonical pair metadata returned only by the configured central API.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    pub format: String,
    pub company_id: String,
    pub community_id: String,
    pub channel_id: String,
    pub employee_id: String,
    pub human_public_key: String,
    pub employee_public_key: String,
    pub pair_hash: String,
    pub selection_id: String,
    pub selection_generation: String,
    pub office_binding_id: String,
    pub key_version: String,
    pub office_generation: String,
    pub authority_epoch: String,
    pub observed_at: DateTime<Utc>,
    pub valid_before: DateTime<Utc>,
}

pub(super) fn uuid(value: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| Error::Encoding)?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(Error::Encoding);
    }
    Ok(())
}

pub(super) fn hex(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Encoding);
    }
    Ok(())
}

impl Pair {
    pub(super) fn validate(&self, channel: &str, human: &str, now: DateTime<Utc>) -> Result<()> {
        for id in [
            &self.company_id,
            &self.community_id,
            &self.channel_id,
            &self.selection_id,
            &self.office_binding_id,
        ] {
            uuid(id)?;
        }
        for key in [
            &self.human_public_key,
            &self.employee_public_key,
            &self.pair_hash,
        ] {
            hex(key)?;
        }
        let human_key =
            nostr::PublicKey::from_hex(&self.human_public_key).map_err(|_| Error::Encoding)?;
        let employee_key =
            nostr::PublicKey::from_hex(&self.employee_public_key).map_err(|_| Error::Encoding)?;
        let mut participants = [human_key.to_bytes(), employee_key.to_bytes()];
        participants.sort();
        if hex::encode(Sha256::digest(participants.concat())) != self.pair_hash {
            return Err(Error::Encoding);
        }
        if self.format != "ortak-native-encrypted-dm-authority/1"
            || self.channel_id != channel
            || self.human_public_key != human
            || self.human_public_key == self.employee_public_key
            || self.employee_id.is_empty()
            || self.employee_id.len() > 128
            || !self
                .employee_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !decimal(&self.selection_generation, false)
            || !decimal(&self.key_version, true)
            || !decimal(&self.office_generation, true)
            || self.authority_epoch != self.office_generation
            || self.observed_at > now + chrono::Duration::seconds(2)
            || self.valid_before <= now
            || self.valid_before <= self.observed_at
            || self.valid_before - self.observed_at > chrono::Duration::seconds(15)
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    /// Observation timestamps are freshness, not a new durable scope identity.
    pub(super) fn scope(&self) -> Result<String> {
        let mut value = serde_json::to_value(self).map_err(|_| Error::Encoding)?;
        let obj = value.as_object_mut().ok_or(Error::Encoding)?;
        obj.remove("observed_at");
        obj.remove("valid_before");
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(&value).map_err(|_| Error::Encoding)?,
        )))
    }
}

fn decimal(value: &str, zero: bool) -> bool {
    value
        .parse::<i64>()
        .is_ok_and(|n| (n > 0 || (zero && n == 0)) && n.to_string() == value)
}

pub(super) struct Session {
    pub keys: Keys,
    pub relay: String,
    origin: String,
}

fn origin(raw: &str, relay: &str) -> Result<String> {
    if raw.len() > 8192 {
        return Err(Error::Unavailable);
    }
    let bindings: BTreeMap<String, String> =
        serde_json::from_str(raw).map_err(|_| Error::Unavailable)?;
    if bindings.len() > 16 {
        return Err(Error::Unavailable);
    }
    let mut ws = url::Url::parse(relay).map_err(|_| Error::Unavailable)?;
    if !matches!(ws.scheme(), "ws" | "wss")
        || !ws.username().is_empty()
        || ws.password().is_some()
        || ws.path() != "/"
        || ws.query().is_some()
        || ws.fragment().is_some()
    {
        return Err(Error::Unavailable);
    }
    let scheme = if ws.scheme() == "ws" { "http" } else { "https" };
    ws.set_scheme(scheme).map_err(|_| Error::Unavailable)?;
    let selected = bindings
        .get(&ws.origin().ascii_serialization())
        .ok_or(Error::Unavailable)?;
    let api = url::Url::parse(selected).map_err(|_| Error::Unavailable)?;
    let local = matches!(api.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(api.scheme() == "https" || (api.scheme() == "http" && local))
        || api.origin().ascii_serialization() != *selected
        || !api.username().is_empty()
        || api.password().is_some()
    {
        return Err(Error::Unavailable);
    }
    Ok(selected.clone())
}

impl Session {
    #[cfg(test)]
    pub(super) fn for_transport_test(keys: Keys, relay: String) -> Self {
        Self {
            keys,
            relay,
            origin: String::new(),
        }
    }
    pub(super) fn current(
        state: &AppState,
        expected_human: &str,
        expected_relay: &str,
    ) -> Result<Self> {
        hex(expected_human)?;
        let keys = state.signing_keys().map_err(|_| Error::Revoked)?;
        let relay = crate::relay::relay_ws_url_with_override(state);
        if keys.public_key().to_hex() != expected_human || relay != expected_relay {
            return Err(Error::Revoked);
        }
        // This is operator process/build configuration, never an IPC argument.
        let raw = std::env::var("ORTAK_ENCRYPTED_DM_API_BINDINGS")
            .ok()
            .or_else(|| std::env::var("VITE_ORTAK_API_BINDINGS_JSON").ok())
            .or_else(|| option_env!("VITE_ORTAK_API_BINDINGS_JSON").map(str::to_owned))
            .ok_or(Error::Unavailable)?;
        Ok(Self {
            keys,
            origin: origin(&raw, &relay)?,
            relay,
        })
    }

    pub(super) fn check(&self, state: &AppState) -> Result<()> {
        if state
            .signing_keys()
            .map_err(|_| Error::Revoked)?
            .public_key()
            != self.keys.public_key()
            || crate::relay::relay_ws_url_with_override(state) != self.relay
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    pub(super) async fn pair(&self, state: &AppState, channel: &str) -> Result<Pair> {
        self.check(state)?;
        uuid(channel)?;
        let url = format!(
            "{}/api/v1/channels/{channel}/encrypted-dm/authority",
            self.origin
        );
        let auth = crate::relay::build_nip98_auth_header_for_keys(
            &self.keys,
            &reqwest::Method::GET,
            &url,
            &[],
        )
        .map_err(|_| Error::Unavailable)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::Unavailable)?;
        let response = client
            .get(url)
            .header("Authorization", auth)
            .header("Cache-Control", "no-store")
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if !response.status().is_success() {
            return Err(Error::Revoked);
        }
        if response.content_length().is_some_and(|n| n > 8192) {
            return Err(Error::Bounds);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::Unavailable)?;
            if body.len() + chunk.len() > 8192 {
                return Err(Error::Bounds);
            }
            body.extend_from_slice(&chunk);
        }
        if body
            .iter()
            .copied()
            .find(|b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
            != Some(b'{')
        {
            return Err(Error::Encoding);
        }
        let pair: Pair = serde_json::from_slice(&body).map_err(|_| Error::Encoding)?;
        self.check(state)?;
        pair.validate(channel, &self.keys.public_key().to_hex(), Utc::now())?;
        Ok(pair)
    }

    pub(super) async fn unchanged(&self, state: &AppState, pair: &Pair) -> Result<()> {
        if self.pair(state, &pair.channel_id).await?.scope()? != pair.scope()? {
            return Err(Error::Revoked);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deployment_selection_rejects_browser_origin_and_unbound_relay() {
        let raw = r#"{"http://127.0.0.1:8080":"http://127.0.0.1:8640"}"#;
        assert_eq!(
            origin(raw, "ws://127.0.0.1:8080").ok().as_deref(),
            Some("http://127.0.0.1:8640")
        );
        assert!(origin(raw, "ws://127.0.0.1:9999").is_err());
        assert!(origin(raw, "ws://127.0.0.1:8080/path").is_err());
        assert!(origin(
            r#"{"http://127.0.0.1:8080":"http://untrusted.example"}"#,
            "ws://127.0.0.1:8080"
        )
        .is_err());
    }

    #[test]
    fn canonical_zero_key_version_large_decimal_generation_and_epoch_retirement() {
        let human = nostr::Keys::generate().public_key();
        let employee = nostr::Keys::generate().public_key();
        let mut keys = [human.to_bytes(), employee.to_bytes()];
        keys.sort();
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().to_string();
        let mut pair = Pair {
            format: "ortak-native-encrypted-dm-authority/1".into(),
            company_id: id.clone(),
            community_id: id.clone(),
            channel_id: id.clone(),
            employee_id: "deniz-private".into(),
            human_public_key: human.to_hex(),
            employee_public_key: employee.to_hex(),
            pair_hash: hex::encode(Sha256::digest(keys.concat())),
            selection_id: id.clone(),
            office_binding_id: id.clone(),
            selection_generation: i64::MAX.to_string(),
            key_version: "0".into(),
            office_generation: "0".into(),
            authority_epoch: "0".into(),
            observed_at: now,
            valid_before: now + chrono::Duration::seconds(5),
        };
        assert!(pair.validate(&id, &human.to_hex(), now).is_ok());
        let original = pair.scope().unwrap();
        pair.observed_at += chrono::Duration::milliseconds(1);
        pair.valid_before += chrono::Duration::milliseconds(1);
        assert_eq!(pair.scope().unwrap(), original);
        pair.office_generation = "1".into();
        pair.authority_epoch = "1".into();
        assert_ne!(pair.scope().unwrap(), original);
        pair.key_version = "00".into();
        assert!(pair.validate(&id, &human.to_hex(), now).is_err());
        pair.key_version = "0".into();
        pair.valid_before = now;
        assert!(pair.validate(&id, &human.to_hex(), now).is_err());
    }
}
