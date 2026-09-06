use super::*;
use ortak_control::memory::MemoryError;

/// Controlled transport seam: the production scheduler invokes exactly the
/// immutable request prepared from its original retained target.
#[allow(async_fn_in_trait)]
pub trait EmployeeExportAdapter {
    /// Complete one bounded remote mutation, returning no edited text.
    async fn write(
        &self,
        request: &PreparedEmployeeExport,
    ) -> std::result::Result<ReviewedEmployeeAcknowledgement, MemoryError>;
}
/// One explicitly selected owned adapter. Retained old targets require their
/// original adapter/configuration; the worker never substitutes a new binding.
pub struct HonchoEmployeeExportAdapter<'a> {
    adapter: &'a HonchoMemoryAdapter,
}
impl<'a> HonchoEmployeeExportAdapter<'a> {
    /// Wrap the caller-selected configured adapter; this does not schedule work.
    pub fn new(adapter: &'a HonchoMemoryAdapter) -> Self {
        Self { adapter }
    }
}
impl EmployeeExportAdapter for HonchoEmployeeExportAdapter<'_> {
    async fn write(
        &self,
        r: &PreparedEmployeeExport,
    ) -> std::result::Result<ReviewedEmployeeAcknowledgement, MemoryError> {
        let rejected = || MemoryError::Rejected {
            detail: ortak_control::adapter::Detail::new("employee export retained target differs"),
        };
        let namespace = self
            .adapter
            .inspect_reviewed_employee_namespace(&r.original)
            .await?;
        if namespace.namespace_hash() != r.namespace_hash
            || namespace.binding_hash() != r.binding_hash
            || r.company_id != r.original.company_id
            || r.employee_id != r.original.employee_id
        {
            return Err(rejected());
        }
        let result = match r.lease.action {
            EmployeeExportAction::Publish => {
                self.adapter
                    .publish_reviewed_employee(
                        &namespace,
                        &ReviewedEmployeePublication {
                            commitment: r.commitment.clone(),
                            content: r.content.clone().ok_or_else(rejected)?,
                            provenance: r.provenance.clone().ok_or_else(rejected)?,
                        },
                    )
                    .await?
            }
            EmployeeExportAction::Withdraw => {
                self.adapter
                    .withdraw_reviewed_employee(&namespace, &r.commitment)
                    .await?
            }
        };
        if result.request_hash != r.request_hash {
            return Err(rejected());
        }
        Ok(result)
    }
}
/// One finite attempt; no loop, periodic diagnostic or runtime activation. The
/// caller runs this behind its explicit deployment selection and cancellation.
pub async fn schedule_one<A: EmployeeExportAdapter>(
    control: &PgControlPlane,
    scope: &CompanyScope,
    adapter: &A,
) -> Result<bool> {
    tokio::time::timeout(Duration::from_secs(24), async {
        let Some(lease) = claim(control, scope).await? else {
            return Ok(false);
        };
        let prepared = match prepare(control, scope, &lease).await {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(false),
            Err(error) => {
                let denied = matches!(error, WorkError::AccessDenied);
                fail(
                    control,
                    scope,
                    &lease,
                    if denied {
                        "authority_refused"
                    } else {
                        "database_retry"
                    },
                    denied,
                )
                .await?;
                return Err(error);
            }
        };
        match tokio::time::timeout(Duration::from_secs(12), adapter.write(&prepared)).await {
            Ok(Ok(receipt)) => acknowledge(control, scope, &lease, &receipt).await,
            failure => {
                let (code, permanent) = match failure {
                    Err(_) => ("deadline", false),
                    Ok(Err(MemoryError::Unsupported { .. })) => ("target_unavailable", false),
                    Ok(Err(e)) if e.is_retryable() => ("service_retry", false),
                    _ => ("service_refused", true),
                };
                fail(control, scope, &lease, code, permanent).await?;
                Ok(false)
            }
        }
    })
    .await
    .map_err(|_| WorkError::OperationTimedOut)?
}
