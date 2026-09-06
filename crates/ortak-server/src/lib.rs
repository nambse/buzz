#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Authenticated, private-MVP Employees and Activity API over durable Ortak data.
//!
//! This router starts no runtime workers. Cancellation returns a durable pending
//! request; only the supervisor can report that execution actually stopped.

mod auth;
mod cancel;
mod config;
mod employees;
mod employee_memory;
pub mod employee_memory_exports;
mod error;
pub mod management;
mod memory;
pub mod prepared_memory;
mod prepared_runtime;
pub mod provisioning;
#[path = "worker_reviewed_exports.rs"]
pub mod reviewed_export_worker;
mod routes;
pub mod shutdown;
mod store;
mod work;
mod worker_database;

pub use config::{ApiConfig, HumanGrant, Role};
pub use routes::product_router;
pub use worker_database::{connect_private_database, connect_worker_database};

pub mod workspace_reader;
pub mod worker_workspace_tools;
