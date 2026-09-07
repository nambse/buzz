//! Snapshot witness for the coordinated Office mutation fence.
//!
//! The witness is read before any routing input. Every authorization-relevant
//! Office mutation advances its company generation under the exclusive fence.
//! Routing commit and runtime admission acquire the shared fence and compare
//! the witness before writing. Thus absent-row insertions are fenced too.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Server-read authority generation and the next binding or selected-DM expiry.
///
/// Fields are private: callers carry a repository witness instead of inventing
/// an authorization generation from a message or runtime response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficeAuthority {
    company_id: Uuid,
    generation: i64,
    valid_before: Option<DateTime<Utc>>,
}

impl OfficeAuthority {
    pub(crate) fn new(
        company_id: Uuid,
        generation: i64,
        valid_before: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            company_id,
            generation,
            valid_before,
        }
    }

    /// Company whose Office rows the witness covers.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Monotonic generation of the coordinated Office mutation protocol.
    pub fn generation(&self) -> i64 {
        self.generation
    }

    /// First future binding/selected-DM transition; equality is already expired.
    pub fn valid_before(&self) -> Option<DateTime<Utc>> {
        self.valid_before
    }
}
