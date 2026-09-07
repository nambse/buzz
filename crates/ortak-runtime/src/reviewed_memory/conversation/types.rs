use super::*;

/// Current selection metadata with no approved text or runtime authority.
#[derive(Clone)]
pub struct ReviewedConversationSelection {
    /// Server-resolved company, also checked against the owned remote receipt.
    pub company_id: Uuid,
    /// Explicit configured project, never inferred from model input.
    pub project_id: Uuid,
    /// Sole selected employee.
    pub employee_id: ortak_domain::EmployeeId,
    /// Exact current memory resource identity.
    pub binding: ortak_domain::MemoryBinding,
    /// Canonical database-observed requester and source.
    pub origin: ConversationMemoryOrigin,
    /// Final at most eight pins in thread/channel/project then UUID order.
    pub records: Vec<ReviewedSelectionPin>,
    /// The bounded candidate, count, encoded-size or content budget omitted matches.
    pub truncated: bool,
}

/// Exact request metadata for one selected record; never includes local fact text.
#[derive(Clone, Eq, PartialEq)]
pub enum ReviewedSelectionPin {
    /// Unchanged project approval metadata.
    Project {
        /// Legacy project use pins.
        pin: ReviewedMemoryPin,
    },
    /// Conversation approval and canonical fact provenance.
    Conversation {
        /// Explicit thirteen-field conversation use pins.
        pin: ReviewedConversationPin,
        /// Exact canonical v1 provenance JSON string, not source text.
        provenance: String,
    },
}

impl ReviewedSelectionPin {
    /// Stable remote record identity.
    pub fn fact_id(&self) -> Uuid {
        match self {
            Self::Project { pin } => pin.fact_id,
            Self::Conversation { pin, .. } => pin.fact_id,
        }
    }

    /// Common remote metadata only; conversation remains explicitly tagged.
    pub fn common_pin(&self) -> ReviewedMemoryPin {
        match self {
            Self::Project { pin } => pin.clone(),
            Self::Conversation { pin, .. } => ReviewedMemoryPin {
                fact_id: pin.fact_id,
                target_id: pin.target_id,
                fact_version: pin.fact_version,
                consumption_epoch: pin.consumption_epoch,
                content_hash: pin.content_hash.clone(),
                source_hash: pin.source_hash.clone(),
                binding_hash: pin.binding_hash.clone(),
                approval_id: pin.approval_id,
                approved_by: pin.approved_by.clone(),
                expires_at: pin.expires_at,
            },
        }
    }

    pub(super) fn record(&self, content: String) -> ReviewedContextRecord {
        match self {
            Self::Project { pin } => ReviewedContextRecord::Project {
                record: ReviewedMemoryRecord {
                    pin: pin.clone(),
                    content,
                },
            },
            Self::Conversation { pin, provenance } => ReviewedContextRecord::Conversation {
                record: ReviewedConversationRecord {
                    pin: pin.clone(),
                    content,
                    provenance: provenance.clone(),
                },
            },
        }
    }

    pub(super) fn matches(&self, record: &ReviewedContextRecord) -> bool {
        match (self, record) {
            (Self::Project { pin }, ReviewedContextRecord::Project { record }) => {
                pin == &record.pin
            }
            (
                Self::Conversation { pin, provenance },
                ReviewedContextRecord::Conversation { record },
            ) => pin == &record.pin && provenance == &record.provenance,
            _ => false,
        }
    }
}

/// Verified remote records; an absent selected record stays absent.
#[derive(Clone, Default)]
pub struct ReviewedSelectedRecall {
    /// Returned records can be empty or project-only; composition chooses v1–4.
    pub records: Vec<ReviewedContextRecord>,
    /// The remote read stopped at its finite result/content budget.
    pub truncated: bool,
}
