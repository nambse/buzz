use super::*;
use chrono::{Datelike, SecondsFormat};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Experience,
    Relationship,
}
impl Kind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Experience => "experience",
            Self::Relationship => "relationship",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewRequest {
    pub source_event_id: String,
    pub destination_channel_id: Uuid,
    pub kind: Kind,
    // Required even for experience: omission must not silently choose a scope.
    #[serde(deserialize_with = "explicit_human")]
    pub human_public_key: Option<String>,
}
fn explicit_human<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
impl PreviewRequest {
    pub(super) fn validate(&self) -> Result<MessageId> {
        let id = hex_id(&self.source_event_id)?;
        nonnil(self.destination_channel_id)?;
        match (self.kind, self.human_public_key.as_deref()) {
            (Kind::Experience, None) => {}
            (Kind::Relationship, Some(human)) => {
                hex_id(human)?;
            }
            _ => return Err(ApiError::invalid()),
        }
        Ok(id)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Draft {
    pub source_event_id: String,
    pub source_event_created_at: DateTime<Utc>,
    pub destination_channel_id: Uuid,
    pub kind: Kind,
    #[serde(deserialize_with = "explicit_human")]
    pub human_public_key: Option<String>,
    pub expected_audience_hash: String,
    pub content: String,
    pub expires_at: DateTime<Utc>,
    pub reviewed: bool,
}
impl Draft {
    pub(super) fn request(&self) -> PreviewRequest {
        PreviewRequest {
            source_event_id: self.source_event_id.clone(),
            destination_channel_id: self.destination_channel_id,
            kind: self.kind,
            human_public_key: self.human_public_key.clone(),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.request().validate()?;
        timestamp(self.source_event_created_at)?;
        timestamp(self.expires_at)?;
        hex_id(&self.expected_audience_hash)?;
        if !self.reviewed
            || self.content.trim().is_empty()
            || self.content.len() > 4096
            || self
                .content
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            || ortak_control::run_event::RedactionPolicy::new().redact(&self.content)
                != self.content
        {
            return Err(ApiError::invalid());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Approval {
    pub operation_id: Uuid,
    pub fact: Draft,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stop {
    pub operation_id: Uuid,
    pub expected_version: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    pub after: Option<Uuid>,
}

pub(super) fn hex_id(value: &str) -> Result<MessageId> {
    EmployeeMemoryDigest::parse_hex(value)
        .map(|d| MessageId::from_bytes(*d.as_bytes()))
        .map_err(|_| ApiError::invalid())
}
pub(super) fn nonnil(value: Uuid) -> Result<()> {
    if value.is_nil() {
        Err(ApiError::invalid())
    } else {
        Ok(())
    }
}
pub(super) fn timestamp(value: DateTime<Utc>) -> Result<String> {
    if !(1970..=9999).contains(&value.year())
        || value.timestamp_subsec_nanos() >= 1_000_000_000
        || value.timestamp_subsec_nanos() % 1000 != 0
    {
        return Err(ApiError::invalid());
    }
    Ok(value.to_rfc3339_opts(SecondsFormat::Micros, true))
}
pub(super) fn expiry_limit(
    now: DateTime<Utc>,
    before: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>> {
    let limit = now
        .checked_add_signed(chrono::Duration::days(90))
        .ok_or_else(ApiError::unavailable)?;
    timestamp(limit)?;
    if before.is_some_and(|before| before <= now) {
        return Err(forbidden());
    }
    Ok(before.map_or(limit, |before| before.min(limit)))
}
