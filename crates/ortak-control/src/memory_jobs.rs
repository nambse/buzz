//! Durable writes of acknowledged Office output into the original run scope.
//! A lease is work ownership, never authorization to call a memory adapter.

use std::time::Duration;

use uuid::Uuid;

use crate::memory::MemoryWriteReceipt;
use crate::{CompanyScope, Result};

/// One claimed job. Claim immediately before use; do not hoard batches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryWriteJobLease {
    /// Original completed run.
    pub run_id: Uuid,
    /// Fencing token for this attempt.
    pub lease_token: Uuid,
    /// Number of claims already consumed, at most twenty.
    pub attempt_count: i32,
}

/// Result of recording an unsuccessful attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryWriteJobOutcome {
    /// The lease no longer owns this pending job.
    Stale,
    /// A bounded retry is durably scheduled.
    Retrying,
    /// No further automatic attempt is allowed.
    Failed,
}

/// Persistence boundary for post-publication memory. Requests must first pass
/// canonical Office revalidation and `postgres::prepare_memory_write_on` in
/// one bounded shared-fence transaction, committed before adapter I/O.
#[allow(async_fn_in_trait)]
pub trait MemoryWriteJobRepository {
    /// Claims at most one due job for the configured adapter. Also terminalizes
    /// at most 64 expired final attempts. Accepts leases from 1 to 300 seconds.
    async fn claim_memory_write(
        &self,
        scope: &CompanyScope,
        adapter: &str,
        lease: Duration,
    ) -> Result<Option<MemoryWriteJobLease>>;

    /// Saves the adapter's verified receipt with a live admitted lease. The
    /// adapter must validate its remote receipt against the exact request;
    /// this repository checks the bounded reference and expected fact count.
    async fn acknowledge_memory_write(
        &self,
        scope: &CompanyScope,
        lease: &MemoryWriteJobLease,
        receipt: &MemoryWriteReceipt,
    ) -> Result<bool>;

    /// Persists a bounded error code and exponential backoff capped at 300s.
    /// Permanent failures and the twentieth attempt remain visibly terminal.
    async fn fail_memory_write(
        &self,
        scope: &CompanyScope,
        lease: &MemoryWriteJobLease,
        code: &str,
        permanent: bool,
    ) -> Result<MemoryWriteJobOutcome>;
}
