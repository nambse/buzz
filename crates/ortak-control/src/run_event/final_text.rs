//! Shared complete final-turn assembly for Office output and Work text artifacts.
use super::RunEventPayload;

/// Closed refusal codes for a complete, bounded final assistant turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalTextRefusal {
    /// Too many fragments or encoded bytes.
    FragmentLimit,
    /// At least one fragment is not a normalized assistant delta.
    InvalidDelta,
    /// At least one fragment carries truncation provenance.
    Truncated,
    /// Complete text exceeds the output ceiling.
    Oversized,
    /// No non-whitespace text was produced.
    Empty,
}

/// Assemble normalized deltas without truncating or accepting partial output.
/// At most 4096 fragments, 1 MiB encoded payload and 32 KiB final text are accepted.
pub fn assemble_final_text(payloads: Vec<serde_json::Value>) -> Result<String, FinalTextRefusal> {
    if payloads.len() > 4096 {
        return Err(FinalTextRefusal::FragmentLimit);
    }
    let mut text = String::new();
    let mut encoded_bytes = 0usize;
    for payload in payloads {
        encoded_bytes += payload.to_string().len();
        if encoded_bytes > 1024 * 1024 {
            return Err(FinalTextRefusal::FragmentLimit);
        }
        let RunEventPayload::AssistantDelta { delta, .. } =
            serde_json::from_value(payload).map_err(|_| FinalTextRefusal::InvalidDelta)?
        else {
            return Err(FinalTextRefusal::InvalidDelta);
        };
        if delta.truncated || delta.original_bytes.is_some() || delta.original_sha256.is_some() {
            return Err(FinalTextRefusal::Truncated);
        }
        if text.len() + delta.text.len() > 32768 {
            return Err(FinalTextRefusal::Oversized);
        }
        text.push_str(&delta.text);
    }
    if text.trim().is_empty() {
        return Err(FinalTextRefusal::Empty);
    }
    Ok(text)
}
