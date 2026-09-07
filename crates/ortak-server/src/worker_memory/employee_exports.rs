//! One finite ordinary employee export attempt against configured original ownership.
use super::*;
use ortak_control::PgControlPlane;
use ortak_server::employee_memory_exports::{
    EmployeeExportAdapter, HonchoEmployeeExportAdapter, PreparedEmployeeExport,
};

impl EmployeeExportAdapter for WorkerMemory {
    async fn write(
        &self,
        request: &PreparedEmployeeExport,
    ) -> Result<ortak_memory::ReviewedEmployeeAcknowledgement, MemoryError> {
        let configured = {
            let values = self
                .validations
                .lock()
                .map_err(|_| MemoryError::Unsupported {
                    capability: MemoryCapability::Remember,
                })?;
            values.iter().any(|v| {
                v.creation_receipt.as_ref() == Some(&request.original)
                    && v.resource.employee_id == request.employee_id
                    && v.resource.binding == request.original.binding
                    && request.original.company_id == request.company_id
            })
        };
        if !configured {
            return Err(MemoryError::Unsupported {
                capability: MemoryCapability::Remember,
            });
        }
        let adapter = self.adapter.as_ref().ok_or(MemoryError::Unsupported {
            capability: MemoryCapability::Remember,
        })?;
        HonchoEmployeeExportAdapter::new(adapter)
            .write(request)
            .await
    }
}

impl WorkerMemory {
    /// Existing signed publication/withdrawal commands supply the durable intent.
    /// Only explicitly configured original owned bindings can execute it; no
    /// runtime-consumption opt-in or new validation probe is implied.
    pub(crate) async fn schedule_employee_export(
        &self,
        control: &PgControlPlane,
        scope: &CompanyScope,
    ) -> ortak_work::Result<bool> {
        if self.adapter.is_none()
            || !self
                .validations
                .lock()
                .map_err(|_| ortak_work::WorkError::OperationTimedOut)?
                .iter()
                .any(|v| v.creation_receipt.is_some())
        {
            return Ok(false);
        }
        let installed = tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query_scalar::<_, bool>(
                "SELECT to_regclass('public.employee_reviewed_memory_export_jobs') IS NOT NULL",
            )
            .fetch_one(control.pool()),
        )
        .await
        .map_err(|_| ortak_work::WorkError::OperationTimedOut)??;
        if !installed {
            return Ok(false);
        }
        ortak_server::employee_memory_exports::schedule_one(control, scope, self).await
    }
}
