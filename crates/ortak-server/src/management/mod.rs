//! Audited prepared-resource commands. Product requests never run adapters.
mod catalog;
mod executor;
mod http;
mod policy;

pub use catalog::import_prepared_catalog;
pub use executor::{execute_next, ExecutionOutcome};
pub use policy::synchronize_authorizations;

use crate::routes::ApiState;
use axum::{
    routing::{get, post},
    Router,
};

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/employee-preparations", get(http::catalog))
        .route(
            "/api/v1/employees/{employee_id}/configuration-drafts",
            post(http::draft),
        )
        .route(
            "/api/v1/employees/{employee_id}/management-commands",
            get(http::commands).post(http::admit),
        )
}

fn fingerprint(value: &impl serde::Serialize) -> Result<Vec<u8>, &'static str> {
    use sha2::{Digest, Sha256};
    Ok(
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "invalid management selection")?)
            .to_vec(),
    )
}
