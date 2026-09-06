//! Explicit employee-owned reviewed storage, separate from project and generic
//! peer memory. None of these operations grants runtime/Office authority.
use crate::*;
mod namespace;
mod records;
mod types;
mod wire;
pub use types::*;
pub use wire::employee_reviewed_request_hash;
