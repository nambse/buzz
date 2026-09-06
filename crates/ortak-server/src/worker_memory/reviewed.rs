use super::*;
use ortak_control::PgControlPlane;
use ortak_memory::{ReviewedProjectPublication, ReviewedProjectRemoval, ReviewedProjectScope};
use ortak_work::reviewed_exports::{
    PreparedReviewedExport, ReviewedConversationTarget, ReviewedExportAcknowledgement,
    ReviewedExportAction, ReviewedMemoryTarget,
};

impl WorkerMemory {
    /// Advertises only an explicit finite project list after the existing actual
    /// diagnostic succeeded. A health read never makes a target eligible.
    pub(crate) async fn advertise_reviewed(
        &self,
        control: &PgControlPlane,
        scope: &CompanyScope,
    ) -> ortak_work::Result<()> {
        let waiting = {
            let due = self
                .reviewed_advertisement_due
                .lock()
                .map_err(|_| ortak_work::WorkError::OperationTimedOut)?;
            *due > Instant::now()
        };
        if waiting {
            return Ok(());
        }
        // Retained employee namespace registration has its own fixed expiry.
        // This only converges explicit selection flags; it never probes or renews.
        employee_advertisement::apply(self, control, scope).await?;
        let (targets, conversations) = {
            let values = self
                .validations
                .lock()
                .map_err(|_| ortak_work::WorkError::OperationTimedOut)?;
            let mut targets = Vec::new();
            let mut conversations = Vec::new();
            for value in values.iter() {
                let Some(deadline) = value.ready_until.filter(|d| *d > Instant::now()) else {
                    continue;
                };
                let Some(receipt) = &value.creation_receipt else {
                    continue;
                };
                for project in &value.reviewed_projects {
                    targets.push(ReviewedMemoryTarget {
                        project_id: *project,
                        runtime_consumption_enabled: value
                            .reviewed_runtime_projects
                            .contains(project),
                        employee_id: receipt.employee_id.clone(),
                        deployment_id: receipt.deployment_id,
                        binding: receipt.binding.clone(),
                        creation_receipt: serde_json::to_value(receipt)
                            .map_err(|_| ortak_work::WorkError::OperationTimedOut)?,
                        valid_for: deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_secs(55)),
                    });
                }
                conversations.extend(value.reviewed_conversations.iter().map(|selection| {
                    ReviewedConversationTarget {
                        project_id: selection.project_id,
                        employee_id: receipt.employee_id.clone(),
                        channel_id: selection.channel_id,
                    }
                }));
            }
            (targets, conversations)
        };
        ortak_work::reviewed_exports::advertise_targets_with_conversations(
            control,
            scope,
            &targets,
            &conversations,
        )
        .await?;
        *self
            .reviewed_advertisement_due
            .lock()
            .map_err(|_| ortak_work::WorkError::OperationTimedOut)? =
            Instant::now() + Duration::from_secs(25);
        Ok(())
    }

    /// Executes only the original configured owned binding. Cleanup ignores the
    /// employee's current status but never switches to replacement resources.
    pub(crate) async fn write_reviewed(
        &self,
        request: &PreparedReviewedExport,
    ) -> Result<ReviewedExportAcknowledgement, MemoryError> {
        let capability = MemoryCapability::Remember;
        let receipt: HonchoCreatedResourcesReceipt =
            serde_json::from_value(request.creation_receipt.clone())
                .map_err(|_| MemoryError::Unsupported { capability })?;
        let configured = {
            let values = self
                .validations
                .lock()
                .map_err(|_| MemoryError::Unsupported { capability })?;
            values.iter().any(|value| {
                value.creation_receipt.as_ref() == Some(&receipt)
                    && value.reviewed_projects.contains(&request.project_id)
                    && value.resource.binding == request.binding
                    && value.resource.employee_id == request.employee_id
            })
        };
        if !configured
            || receipt.company_id != request.company_id
            || receipt.deployment_id != request.deployment_id
            || receipt.employee_id != request.employee_id
            || receipt.binding != request.binding
        {
            return Err(MemoryError::Unsupported { capability });
        }
        let adapter = self.selected(capability)?;
        let scope = ReviewedProjectScope {
            employee_id: request.employee_id.clone(),
            binding: request.binding.clone(),
            project_id: request.project_id,
        };
        let response = match request.lease.action {
            ReviewedExportAction::Publish => {
                adapter
                    .publish_reviewed_project(
                        &scope,
                        &ReviewedProjectPublication {
                            record_id: request.lease.fact_id,
                            idempotency_key: request.idempotency_key.clone(),
                            content: request
                                .content
                                .clone()
                                .ok_or(MemoryError::Unsupported { capability })?,
                            source_hash: request.source_hash.clone(),
                            approval_id: request.approval_id,
                            approved_by: request.approved_by.clone(),
                            expires_at: request.expires_at,
                        },
                    )
                    .await?
            }
            ReviewedExportAction::Withdraw => {
                adapter
                    .remove_reviewed_project(
                        &scope,
                        request.lease.fact_id,
                        &request.idempotency_key,
                        ReviewedProjectRemoval::Withdraw,
                    )
                    .await?
            }
        };
        let decode =
            |value: &str| hex::decode(value).map_err(|_| MemoryError::Unsupported { capability });
        let request_hash = decode(&response.request_hash)?;
        if request_hash != request.request_hash {
            return Err(MemoryError::Unsupported { capability });
        }
        Ok(ReviewedExportAcknowledgement {
            request_hash,
            binding_hash: decode(&response.record.binding_hash)?,
            content_hash: response
                .record
                .content_hash
                .as_deref()
                .map(decode)
                .transpose()?,
            remote_status: match response.record.status {
                ortak_memory::ReviewedProjectStatus::Active => "active",
                ortak_memory::ReviewedProjectStatus::Expired => "expired",
                ortak_memory::ReviewedProjectStatus::Withdrawn => "withdrawn",
            }
            .into(),
            erased_from_reviewed_store: response.record.erased_from_reviewed_store,
            tombstone_at: response.record.tombstone_at,
        })
    }
}

impl ortak_server::reviewed_export_worker::ReviewedExportAdapter for WorkerMemory {
    async fn write(
        &self,
        request: &PreparedReviewedExport,
    ) -> Result<ReviewedExportAcknowledgement, MemoryError> {
        self.write_reviewed(request).await
    }
}
