//! Read seam for Activity.
//!
//! Every method takes the server-resolved
//! [`CompanyScope`](ortak_control::CompanyScope) beside caller-supplied
//! filters, so no query can be widened past the company boundary. All
//! methods are reads; nothing here mutates runs or events.

use ortak_control::CompanyScope;
use uuid::Uuid;

use crate::error::Result;
use crate::model::RunDetail;
use crate::query::{RunEventPage, RunEventsQuery, RunListPage, RunListQuery};

/// Company-scoped Activity reads.
#[allow(async_fn_in_trait)]
pub trait ActivityQueries {
    /// Lists runs newest first with deterministic keyset paging.
    async fn list_runs(&self, scope: &CompanyScope, query: &RunListQuery) -> Result<RunListPage>;

    /// One run with its bounded terminal text and aggregate summary.
    /// Unknown runs and runs of other companies are both
    /// [`ActivityError::RunNotFound`](crate::ActivityError::RunNotFound).
    async fn run_detail(&self, scope: &CompanyScope, run_id: Uuid) -> Result<RunDetail>;

    /// Ordered, dense, bounded page of a run's typed events after a
    /// sequence cursor. Fails closed exactly like [`Self::run_detail`] when
    /// the run is not visible in the scope, even when it has no events.
    async fn run_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        query: &RunEventsQuery,
    ) -> Result<RunEventPage>;
}
