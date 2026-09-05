//! Bounded, trust-scoped reads of a signed event's tag array.
//!
//! Only two tag shapes are consulted, and neither is trusted on its own:
//!
//! - `["p", <64-hex>]` mention keys are candidates that become structured
//!   mentions only when the key resolves through `employee_office_bindings`.
//!   A name is never remapped to a key here; the editor's key tag is the
//!   accepted mention and the binding table is the authority.
//! - `["e", <64-hex>, <relay>, "reply"]` markers are read only to detect a
//!   client-claimed reply that the relay did not persist in
//!   `thread_metadata`; such a claim is a refusal, never a parent. The
//!   marker grammar mirrors `buzz_core::nip10::parse_thread_markers` (marker
//!   at index 3, id exactly 64 ASCII hex); drift can only make this stricter
//!   or looser about *refusing*, never about waking, because wakes through a
//!   parent come solely from `thread_metadata`.
//!
//! Every other tag (`h`, `root`, `broadcast`, anything spelling "system",
//! "dispatch", or "assign") is ignored: origin, loop root, structured
//! dispatch, and Work assignment come from server rows or not at all.
//!
//! The scan is bounded and **refuses instead of truncating**: an event with
//! more tags than [`MAX_TAGS_EXAMINED`] or more distinct mention keys than
//! [`MAX_MENTION_KEYS`] yields [`TagBoundsExceeded`], and the normalizer
//! records that as an explicit silent decision. Routing on a partial view of
//! the tags could turn a requested mention into semantic fan-out.

use std::collections::BTreeSet;

use ortak_control::office_identity::OfficePublicKey;

/// Maximum tags examined, matching `MAX_TAGS` for signed Office events.
pub const MAX_TAGS_EXAMINED: usize = 64;
/// Maximum distinct mention keys carried forward for binding lookup.
pub const MAX_MENTION_KEYS: usize = 16;

/// What the bounded tag scan found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TagFacts {
    /// Distinct, well-formed `p` keys in first-appearance order, at most
    /// [`MAX_MENTION_KEYS`] of them.
    pub mention_keys: Vec<OfficePublicKey>,
    /// True when any `e` tag carries a well-formed `reply` marker.
    pub claims_reply: bool,
}

/// The tag array exceeds a scan bound; the event must be refused, not
/// routed on a truncated view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagBoundsExceeded {
    /// More tags than [`MAX_TAGS_EXAMINED`].
    TooManyTags {
        /// Tags present.
        count: usize,
    },
    /// More distinct well-formed mention keys than [`MAX_MENTION_KEYS`].
    TooManyMentionKeys,
}

/// Scans the tags for mention keys and reply claims, refusing oversized sets.
pub fn scan_tags(tags: &[Vec<String>]) -> Result<TagFacts, TagBoundsExceeded> {
    if tags.len() > MAX_TAGS_EXAMINED {
        return Err(TagBoundsExceeded::TooManyTags { count: tags.len() });
    }
    let mut facts = TagFacts::default();
    let mut seen = BTreeSet::new();
    for tag in tags {
        match tag.first().map(String::as_str) {
            Some("p") => {
                let Some(key) = tag
                    .get(1)
                    .and_then(|value| OfficePublicKey::parse_hex(value).ok())
                else {
                    continue;
                };
                if seen.insert(*key.as_bytes()) {
                    if facts.mention_keys.len() >= MAX_MENTION_KEYS {
                        return Err(TagBoundsExceeded::TooManyMentionKeys);
                    }
                    facts.mention_keys.push(key);
                }
            }
            Some("e") => {
                let well_formed_id = tag
                    .get(1)
                    .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()));
                if well_formed_id && tag.get(3).map(String::as_str) == Some("reply") {
                    facts.claims_reply = true;
                }
            }
            _ => {}
        }
    }
    Ok(facts)
}

/// Removes control characters other than newline, carriage return, and tab
/// so the envelope passes routing validation on the same text the runtime
/// would later receive.
pub fn strip_control_characters(content: &str) -> String {
    content
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn mention_keys_are_deduplicated_and_validated() {
        let key_a = "a".repeat(64);
        let key_b = "b".repeat(64);
        let tags = vec![
            tag(&["p", &key_a]),
            tag(&["p", &key_b]),
            tag(&["p", &key_a]),
            tag(&["p", "not-a-key"]),
            tag(&["p"]),
        ];
        let facts = scan_tags(&tags).expect("bounded");
        assert_eq!(facts.mention_keys.len(), 2);
        assert_eq!(facts.mention_keys[0].to_hex(), key_a);
        assert_eq!(facts.mention_keys[1].to_hex(), key_b);
        assert!(!facts.claims_reply);
    }

    #[test]
    fn oversized_mention_sets_are_refused_not_truncated() {
        let mut tags = Vec::new();
        for index in 0..MAX_MENTION_KEYS {
            tags.push(tag(&["p", &format!("{index:064x}")]));
        }
        // Repeats of already-seen keys do not count against the bound.
        tags.push(tag(&["p", &format!("{:064x}", 0)]));
        assert_eq!(
            scan_tags(&tags)
                .expect("exactly at the bound")
                .mention_keys
                .len(),
            MAX_MENTION_KEYS
        );
        tags.push(tag(&["p", &format!("{:064x}", MAX_MENTION_KEYS)]));
        assert_eq!(scan_tags(&tags), Err(TagBoundsExceeded::TooManyMentionKeys));
    }

    #[test]
    fn reply_claim_requires_a_well_formed_marker_and_nothing_else_is_read() {
        let id = "c".repeat(64);
        let claims = |tags: &[Vec<String>]| scan_tags(tags).expect("bounded").claims_reply;
        assert!(claims(&[tag(&["e", &id, "", "reply"])]));
        assert!(!claims(&[tag(&["e", &id, "", "root"])]));
        assert!(!claims(&[tag(&["e", "bad", "", "reply"])]));
        assert!(!claims(&[tag(&["e", &id])]));
        let ignored = scan_tags(&[
            tag(&["origin", "system"]),
            tag(&["dispatch", "zeynep"]),
            tag(&["assign", "zeynep"]),
            tag(&["root", &id]),
        ])
        .expect("bounded");
        assert_eq!(ignored, TagFacts::default());
    }

    #[test]
    fn oversized_tag_arrays_are_refused_whole() {
        let id = "d".repeat(64);
        let mut tags = vec![tag(&["h", "general"]); MAX_TAGS_EXAMINED];
        assert!(scan_tags(&tags).is_ok());
        tags.push(tag(&["e", &id, "", "reply"]));
        assert_eq!(
            scan_tags(&tags),
            Err(TagBoundsExceeded::TooManyTags {
                count: MAX_TAGS_EXAMINED + 1
            })
        );
    }

    #[test]
    fn control_characters_are_stripped_but_whitespace_survives() {
        assert_eq!(
            strip_control_characters("Cem,\u{0} selam\n\tnasılsın?\u{7}"),
            "Cem, selam\n\tnasılsın?"
        );
    }
}
