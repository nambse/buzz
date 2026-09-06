//! One bounded reviewed publication/cleanup attempt; no remote I/O inside PG transactions.
use ortak_control::{memory::MemoryError, CompanyScope, PgControlPlane};
use ortak_work::{
    reviewed_exports::{self, PreparedReviewedExport, ReviewedExportAcknowledgement},
    WorkError,
};
use std::time::Duration;

/// The production worker seam, allowing a controlled transport failure in PG tests.
#[allow(async_fn_in_trait)]
pub trait ReviewedExportAdapter {
    /// Publishes or removes the exact retained target represented by the request.
    async fn write(
        &self,
        request: &PreparedReviewedExport,
    ) -> Result<ReviewedExportAcknowledgement, MemoryError>;
}
/// Claims one due durable operation. Cancellation stays ahead of this finite pass.
pub async fn schedule_one<A: ReviewedExportAdapter>(
    control: &PgControlPlane,
    scope: &CompanyScope,
    adapter: &A,
) -> ortak_work::Result<bool> {
    tokio::time::timeout(Duration::from_secs(12), async {
        let Some(lease) = reviewed_exports::claim(control, scope).await? else {
            return Ok(false);
        };
        let request = match reviewed_exports::prepare(control, scope, &lease).await {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(false),
            Err(error) => {
                reviewed_exports::fail(
                    control,
                    scope,
                    &lease,
                    if matches!(error, WorkError::AccessDenied) {
                        "authority_refused"
                    } else {
                        "database_retry"
                    },
                    matches!(error, WorkError::AccessDenied),
                )
                .await?;
                return Ok(false);
            }
        };
        match tokio::time::timeout(Duration::from_secs(4), adapter.write(&request)).await {
            Ok(Ok(receipt)) => {
                reviewed_exports::acknowledge(control, scope, &lease, &receipt).await
            }
            failure => {
                let (code, permanent) = match failure {
                    Err(_) => ("deadline", false),
                    Ok(Err(MemoryError::Unsupported { .. })) => ("target_unavailable", false),
                    Ok(Err(error)) if error.is_retryable() => ("service_retry", false),
                    _ => ("service_refused", true),
                };
                reviewed_exports::fail(control, scope, &lease, code, permanent).await?;
                Ok(false)
            }
        }
    })
    .await
    .map_err(|_| WorkError::OperationTimedOut)?
}
