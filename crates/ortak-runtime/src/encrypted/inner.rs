mod json;
use super::{EncryptedExecutionError as Error, Result};
use chrono::{DateTime, Utc};
use json::Node;
use ortak_control::confidential::ValidatedIdentity;
use ortak_domain::RuntimeBinding;
use zeroize::Zeroizing;

fn identity<'a>(node: &'a Node, expected: &ValidatedIdentity) -> Result<&'a Node> {
    let observed =
        ValidatedIdentity::parse(&node.field("identity")?.bytes()?).map_err(|_| Error::Protocol)?;
    if &observed != expected {
        return Err(Error::Protocol);
    }
    Ok(node.field("identity")?)
}
pub(super) fn snapshot(
    bytes: &[u8],
    expected: &ValidatedIdentity,
    binding: &RuntimeBinding,
    expected_reply: Option<&str>,
) -> Result<()> {
    let n = Node::parse(bytes, 48 * 1024)?;
    n.keys(&["format", "identity", "spec"])?;
    if n.field("format")?.text()? != "ortak-confidential-run/1" {
        return Err(Error::Protocol);
    }
    let id = identity(&n, expected)?;
    let s = n.field("spec")?;
    s.keys(&[
        "binding",
        "context",
        "employee_id",
        "idempotency_key",
        "input",
        "permissions",
        "revision_id",
        "run_id",
    ])?;
    for (field, claim) in [
        ("employee_id", "employee_id"),
        ("revision_id", "employee_revision_id"),
        ("run_id", "run_id"),
    ] {
        if s.field(field)?.text()? != id.field(claim)?.text()? {
            return Err(Error::Protocol);
        }
    }
    let start = format!(
        "ortak-run:{}:{}",
        id.field("company_id")?.text()?,
        id.field("run_id")?.text()?
    );
    if s.field("idempotency_key")?.text()? != start {
        return Err(Error::Protocol);
    }
    let input = s.field("input")?.text()?;
    if input.is_empty() || input.len() > 8192 || input.contains('\0') {
        return Err(Error::Protocol);
    }
    let context = s.field("context")?;
    context.keys(&["conversation_ref", "reply_to_message_id"])?;
    if context.field("conversation_ref")?.text()? != id.field("conversation_id")?.text()? {
        return Err(Error::Protocol);
    }
    let reply = context.field("reply_to_message_id")?;
    if reply.is_null() != expected_reply.is_none()
        || (!reply.is_null() && Some(reply.text()?) != expected_reply)
    {
        return Err(Error::Protocol);
    }
    let policy = s.field("permissions")?;
    let fields = [
        "allowed_networks",
        "allowed_tools",
        "allowed_workspaces",
        "approval_required",
    ];
    policy.keys(&fields)?;
    for field in fields {
        if !policy.field(field)?.empty_array() {
            return Err(Error::Protocol);
        }
    }
    // Only metadata is deserialized into the ordinary binding type. Plaintext
    // input/context never becomes an ordinary RunSpec or serde Value.
    let observed: RuntimeBinding =
        serde_json::from_slice(&s.field("binding")?.bytes()?).map_err(|_| Error::Protocol)?;
    if &observed != binding {
        return Err(Error::Protocol);
    }
    Ok(())
}

/// Initial toolless bridge grammar is exactly started, optional one final text,
/// intent, completed. This deliberately rejects arbitrary ordinary event kinds.
pub(super) struct Fold {
    phase: u8,
    count: u32,
    text: Zeroizing<String>,
    intent: Option<bool>,
    last: Option<DateTime<Utc>>,
}
impl Fold {
    pub(super) fn new() -> Self {
        Self {
            phase: 0,
            count: 0,
            text: Zeroizing::new(String::new()),
            intent: None,
            last: None,
        }
    }
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        expected: &ValidatedIdentity,
        ordinal: u32,
        time: DateTime<Utc>,
    ) -> Result<()> {
        let n = Node::parse(bytes, 32 * 1024)?;
        n.keys(&["format", "identity", "sequence", "occurred_at", "payload"])?;
        if n.field("format")?.text()? != "ortak-confidential-event/1"
            || n.field("sequence")?.integer()? != ordinal as u64
            || ordinal != self.count + 1
        {
            return Err(Error::Protocol);
        }
        let id = identity(&n, expected)?;
        let occurred = DateTime::parse_from_rfc3339(n.field("occurred_at")?.text()?)
            .map_err(|_| Error::Protocol)?
            .with_timezone(&Utc);
        if occurred != time || self.last.is_some_and(|old| occurred < old) {
            return Err(Error::Protocol);
        }
        let p = n.field("payload")?;
        match (self.phase, p.field("event_type")?.text()?) {
            (0, "run.started") => {
                p.keys(&["event_type", "runtime_run_ref"])?;
                let reference = format!(
                    "ortak:{}:{}",
                    id.field("company_id")?.text()?,
                    id.field("run_id")?.text()?
                );
                if ordinal != 1 || p.field("runtime_run_ref")?.text()? != reference {
                    return Err(Error::Protocol);
                }
                self.phase = 1;
            }
            (1, "assistant.delta") => {
                p.keys(&["delta", "event_type", "turn"])?;
                p.field("delta")?.keys(&["text"])?;
                let text = p.field("delta")?.field("text")?.text()?;
                if p.field("turn")?.integer()? != 0
                    || text.is_empty()
                    || text.len() > 8192
                    || text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                {
                    return Err(Error::Protocol);
                }
                self.text.push_str(text);
                self.phase = 2;
            }
            (1 | 2, "delivery.intent") => {
                p.keys(&["event_type", "intent"])?;
                let reply = match p.field("intent")?.text()? {
                    "reply" => true,
                    "silent" => false,
                    _ => return Err(Error::Protocol),
                };
                if reply == self.text.is_empty() {
                    return Err(Error::Protocol);
                }
                self.intent = Some(reply);
                self.phase = 3;
            }
            (3, "run.completed") => {
                p.keys(&["delivery_intent", "event_type"])?;
                if Some(p.field("delivery_intent")?.text()? == "reply") != self.intent
                    || !matches!(p.field("delivery_intent")?.text()?, "reply" | "silent")
                {
                    return Err(Error::Protocol);
                }
                self.phase = 4;
            }
            _ => return Err(Error::Protocol),
        }
        self.last = Some(time);
        self.count += 1;
        Ok(())
    }
    pub(super) fn finish(self) -> Result<Option<Zeroizing<String>>> {
        if self.phase != 4 {
            return Err(Error::Protocol);
        }
        Ok(if self.intent == Some(true) {
            Some(self.text)
        } else {
            None
        })
    }
}

pub(super) fn reply_bytes(expected: &ValidatedIdentity, text: &str) -> Result<Zeroizing<Vec<u8>>> {
    // Text is serialized directly into the final zeroizing buffer. The only
    // intermediate Value is the public canonical identity.
    #[derive(serde::Serialize)]
    struct Reply<'a> {
        format: &'static str,
        identity: serde_json::Value,
        text: &'a str,
    }
    let identity =
        serde_json::from_slice(expected.canonical_bytes()).map_err(|_| Error::Protocol)?;
    Ok(Zeroizing::new(
        serde_json::to_vec(&Reply {
            format: "ortak-confidential-reply/1",
            identity,
            text,
        })
        .map_err(|_| Error::Protocol)?,
    ))
}
pub(super) fn open_reply(bytes: &[u8], expected: &ValidatedIdentity) -> Result<Zeroizing<String>> {
    let n = Node::parse(bytes, 16 * 1024)?;
    n.keys(&["format", "identity", "text"])?;
    identity(&n, expected)?;
    let text = n.field("text")?.text()?;
    if n.field("format")?.text()? != "ortak-confidential-reply/1"
        || text.is_empty()
        || text.len() > 8192
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(Error::Protocol);
    }
    Ok(Zeroizing::new(text.to_owned()))
}
