use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{DomainError, EmployeeId};

/// Actor category that authored an Office message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum MessageOrigin {
    /// Human company member.
    Human(String),
    /// Managed company employee.
    Employee(EmployeeId),
    /// Authenticated external integration.
    Integration(String),
    /// Ortak-generated system event.
    System,
}

impl MessageOrigin {
    /// Returns the employee id when the origin is an employee.
    pub fn employee_id(&self) -> Option<&EmployeeId> {
        match self {
            Self::Employee(employee_id) => Some(employee_id),
            Self::Human(_) | Self::Integration(_) | Self::System => None,
        }
    }

    /// Returns whether semantic fan-out is allowed for this origin.
    pub fn allows_semantic_routing(&self) -> bool {
        matches!(self, Self::Human(_))
    }
}

/// Message class used by deterministic non-routable-event guards.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Human-readable Office content.
    Text,
    /// Reaction-only event.
    Reaction,
    /// Delivery acknowledgement from an adapter.
    DeliveryAcknowledgement,
    /// Internal system event.
    System,
}

impl MessageKind {
    /// Returns whether this event is allowed to create employee work.
    pub fn is_routable(self) -> bool {
        matches!(self, Self::Text)
    }
}

/// Office conversation placement used by deterministic routing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationContext {
    /// Shared Office channel.
    Channel {
        /// Stable Office channel identifier.
        channel_id: String,
    },
    /// Direct conversation with zero or more employee participants.
    Direct {
        /// Stable Office conversation identifier.
        conversation_id: String,
        /// Employee participants other than the message author.
        #[serde(default)]
        employee_participants: Vec<EmployeeId>,
    },
}

impl ConversationContext {
    /// Returns direct-message employee recipients, if applicable.
    pub fn direct_employee_participants(&self) -> &[EmployeeId] {
        match self {
            Self::Direct {
                employee_participants,
                ..
            } => employee_participants,
            Self::Channel { .. } => &[],
        }
    }
}

/// Author information for the message being replied to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplyContext {
    /// Stable parent message identifier.
    pub message_id: String,
    /// Origin of the parent message.
    pub origin: MessageOrigin,
}

/// Bounded delivery-chain metadata used to prevent employee loops.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeliveryChain {
    root_message_id: String,
    hop_count: u8,
    wake_count: usize,
    visited_employee_ids: BTreeSet<EmployeeId>,
}

impl DeliveryChain {
    /// Starts a new chain rooted at the supplied message.
    pub fn root(message_id: impl Into<String>) -> Self {
        Self {
            root_message_id: message_id.into(),
            hop_count: 0,
            wake_count: 0,
            visited_employee_ids: BTreeSet::new(),
        }
    }

    /// Advances server-owned chain state for one successful dispatch batch.
    pub fn advance_for_dispatch<'a>(
        &self,
        employee_ids: impl IntoIterator<Item = &'a EmployeeId>,
    ) -> Result<Self, DomainError> {
        let mut visited_employee_ids = self.visited_employee_ids.clone();
        let newly_visited = employee_ids
            .into_iter()
            .filter(|employee_id| visited_employee_ids.insert((*employee_id).clone()))
            .count();
        if newly_visited == 0 {
            return Ok(self.clone());
        }
        let hop_count = self
            .hop_count
            .checked_add(1)
            .ok_or(DomainError::DeliveryChainOverflow)?;
        let wake_count = self
            .wake_count
            .checked_add(newly_visited)
            .ok_or(DomainError::DeliveryChainOverflow)?;

        Ok(Self {
            root_message_id: self.root_message_id.clone(),
            hop_count,
            wake_count,
            visited_employee_ids,
        })
    }

    /// Returns the message that started this dispatch chain.
    pub fn root_message_id(&self) -> &str {
        &self.root_message_id
    }

    /// Returns completed dispatch batches, including the initial root batch.
    pub fn hop_count(&self) -> u8 {
        self.hop_count
    }

    /// Returns the number of unique employee wakes consumed by the chain.
    pub fn wake_count(&self) -> usize {
        self.wake_count
    }

    /// Returns whether an employee has already been woken in this chain.
    pub fn has_visited(&self, employee_id: &EmployeeId) -> bool {
        self.visited_employee_ids.contains(employee_id)
    }

    /// Iterates through employees already visited by this chain.
    pub fn visited_employee_ids(&self) -> impl Iterator<Item = &EmployeeId> {
        self.visited_employee_ids.iter()
    }
}

/// Transport-independent input consumed by the Ortak conversation router.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MessageEnvelope {
    /// Stable Office message identifier.
    pub id: String,
    /// Message classification.
    pub kind: MessageKind,
    /// Actor that authored the message.
    pub origin: MessageOrigin,
    /// Conversation placement.
    pub conversation: ConversationContext,
    /// Human-readable message content.
    pub body: String,
    /// Explicit targets emitted by an authorized structured dispatch action.
    pub dispatch_targets: Vec<EmployeeId>,
    /// Employee mentions already resolved by the Office editor/event adapter.
    pub structured_mentions: Vec<EmployeeId>,
    /// Optional replied-to message.
    pub reply_to: Option<ReplyContext>,
    /// Employee assignments resolved from a structured Work command.
    pub assigned_employee_ids: Vec<EmployeeId>,
    /// Loop-prevention and fan-out budget state.
    chain: DeliveryChain,
}

impl MessageEnvelope {
    /// Builds a new root message from already authenticated Office input.
    pub fn root(
        id: impl Into<String>,
        kind: MessageKind,
        origin: MessageOrigin,
        conversation: ConversationContext,
        body: impl Into<String>,
    ) -> Self {
        let id = id.into();
        Self {
            chain: DeliveryChain::root(id.clone()),
            id,
            kind,
            origin,
            conversation,
            body: body.into(),
            dispatch_targets: Vec::new(),
            structured_mentions: Vec::new(),
            reply_to: None,
            assigned_employee_ids: Vec::new(),
        }
    }

    /// Builds a minimal human channel message for adapters and tests.
    pub fn human_channel(
        id: impl Into<String>,
        actor_id: impl Into<String>,
        channel_id: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self::root(
            id,
            MessageKind::Text,
            MessageOrigin::Human(actor_id.into()),
            ConversationContext::Channel {
                channel_id: channel_id.into(),
            },
            body,
        )
    }

    /// Carries server-derived chain state into an employee-origin delivery.
    #[must_use]
    pub fn with_delivery_chain(mut self, chain: DeliveryChain) -> Self {
        self.chain = chain;
        self
    }

    /// Returns server-owned loop-prevention metadata.
    pub fn chain(&self) -> &DeliveryChain {
        &self.chain
    }

    /// Validates transport-independent identity and content invariants.
    pub fn validate_for_routing(&self) -> Result<(), DomainError> {
        validate_identifier("message.id", &self.id, 256)?;
        validate_identifier(
            "message.chain.root_message_id",
            self.chain.root_message_id(),
            256,
        )?;

        match &self.origin {
            MessageOrigin::Human(actor_id) => {
                validate_identifier("message.origin.human_id", actor_id, 256)?;
            }
            MessageOrigin::Integration(integration_id) => {
                validate_identifier("message.origin.integration_id", integration_id, 256)?;
            }
            MessageOrigin::Employee(_) | MessageOrigin::System => {}
        }

        match &self.conversation {
            ConversationContext::Channel { channel_id } => {
                validate_identifier("message.conversation.channel_id", channel_id, 512)?;
            }
            ConversationContext::Direct {
                conversation_id, ..
            } => {
                validate_identifier("message.conversation.conversation_id", conversation_id, 512)?;
            }
        }

        if self.kind == MessageKind::Text && self.body.trim().is_empty() {
            return Err(DomainError::InvalidMessage {
                field: "message.body",
            });
        }
        if self
            .body
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(DomainError::InvalidMessage {
                field: "message.body",
            });
        }
        if let Some(reply) = &self.reply_to {
            validate_identifier("message.reply_to.message_id", &reply.message_id, 256)?;
        }
        if self.chain.wake_count() != self.chain.visited_employee_ids().count()
            || (self.chain.hop_count() == 0 && self.chain.wake_count() != 0)
        {
            return Err(DomainError::InvalidMessage {
                field: "message.chain",
            });
        }

        Ok(())
    }
}

fn validate_identifier(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        Err(DomainError::InvalidMessage { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversationContext, DeliveryChain, MessageEnvelope, MessageKind, MessageOrigin};
    use crate::EmployeeId;

    #[test]
    fn empty_ids_and_blank_text_fail_routing_validation() {
        let blank_id = MessageEnvelope::human_channel("", "sefa", "office", "hello");
        assert!(blank_id.validate_for_routing().is_err());

        let blank_body = MessageEnvelope::human_channel("message", "sefa", "office", "  \n ");
        assert!(blank_body.validate_for_routing().is_err());

        let blank_actor = MessageEnvelope::human_channel("message", "", "office", "hello");
        assert!(blank_actor.validate_for_routing().is_err());
    }

    #[test]
    fn advancing_without_a_new_recipient_is_a_noop() {
        let cem = EmployeeId::parse("cem").expect("test employee id must be valid");
        let chain = DeliveryChain::root("root")
            .advance_for_dispatch([&cem])
            .expect("first dispatch must advance");
        let repeated = chain
            .advance_for_dispatch([&cem])
            .expect("repeated recipient must remain a safe no-op");
        let empty: [&EmployeeId; 0] = [];
        let empty_batch = repeated
            .advance_for_dispatch(empty)
            .expect("empty dispatch batch must remain a safe no-op");

        assert_eq!(chain, repeated);
        assert_eq!(repeated, empty_batch);
    }

    #[test]
    fn valid_employee_delivery_chain_passes_validation() {
        let cem = EmployeeId::parse("cem").expect("test employee id must be valid");
        let chain = DeliveryChain::root("human-root")
            .advance_for_dispatch([&cem])
            .expect("initial dispatch must advance");
        let message = MessageEnvelope::root(
            "employee-reply",
            MessageKind::Text,
            MessageOrigin::Employee(cem),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "typed reply",
        )
        .with_delivery_chain(chain);

        message
            .validate_for_routing()
            .expect("valid server-derived delivery must pass");
    }
}
