//! Signed production routes against immutable 1..76 plus the two reviewed
//! employee-memory candidate fragments, applied by the explicit root PG gate.
//! This fixture never inserts facts/receipts, manufactures approval or opens a
//! remote target. Its canonical Office events are signed with the fixture actor.
use super::*;
use chrono::{DateTime, SecondsFormat};
use ortak_control::memory::employee::EmployeeMemoryProvenanceV1;

#[path = "employee_memory/atomic.rs"]
mod atomic;
#[path = "employee_memory/authority.rs"]
mod authority;
#[path = "employee_memory/exports.rs"]
pub(crate) mod exports;
#[path = "employee_memory/fixture.rs"]
mod fixture;
#[path = "employee_memory/recovery.rs"]
mod recovery;
use fixture::*;
