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
mod error;
mod memory;
mod routes;
pub mod shutdown;
mod store;
mod work;
mod worker_database;

pub use config::{ApiConfig, HumanGrant, Role};
pub use routes::product_router;
pub use worker_database::{connect_private_database, connect_worker_database};
