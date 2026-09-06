use super::PgControlPlane;
use crate::{
    error::{ControlError, Result},
    CompanyScope,
};
use uuid::Uuid;

impl PgControlPlane {
    /// Refreshes only the existing Office binding of an already resolved company.
    /// Presence is routing metadata, not a current pair/content authorization.
    pub async fn resolve_current_encrypted_scope(
        &self,
        company: &CompanyScope,
    ) -> Result<Option<CompanyScope>> {
        let community: Option<Uuid> = sqlx::query_scalar(
            "SELECT community_id FROM office_company_bindings WHERE company_id=$1",
        )
        .bind(company.company_id())
        .fetch_optional(&self.pool)
        .await?;
        Ok(community.map(|id| CompanyScope::new(company.company_id(), Some(id))))
    }

    /// Resolves bounded retained encrypted provenance for receipt-only recovery,
    /// including after Office unbinding. This is NOT current admission authority:
    /// content/effect repositories must still check their original frozen tuple.
    /// The caller must first resolve the company through the ordinary directory.
    pub async fn confidential_recovery_scopes(
        &self,
        company: &CompanyScope,
    ) -> Result<Vec<CompanyScope>> {
        // The selection registry is capped at 128 retained rows/company. Anchor
        // discovery there instead of materializing an unbounded run history.
        let communities: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT s.community_id FROM encrypted_dm_selections s
             WHERE s.company_id=$1 AND (
               EXISTS(SELECT 1 FROM encrypted_dm_decrypt_jobs j
                 WHERE j.company_id=s.company_id AND j.selection_id=s.selection_id
                   AND j.community_id=s.community_id
                   AND NOT ortak_encrypted_dm_job_consumed(j.company_id,j.source_id)
                   AND (j.state IN('pending','claimed','verified') OR EXISTS(
                     SELECT 1 FROM office_inbox i WHERE i.company_id=j.company_id
                       AND i.event_id=j.source_id AND i.event_created_at=j.source_created_at AND i.state='pending')))
               OR EXISTS(SELECT 1 FROM confidential_runs c
                 WHERE c.company_id=s.company_id AND c.selection_id=s.selection_id
                   AND c.community_id=s.community_id AND (
                     EXISTS(SELECT 1 FROM confidential_run_dispatches d WHERE d.company_id=c.company_id AND d.run_id=c.run_id AND d.state='pending')
                     OR EXISTS(SELECT 1 FROM confidential_execution_leases x WHERE x.company_id=c.company_id AND x.run_id=c.run_id AND x.state IN('observing','sealing','cancelling'))
                     OR EXISTS(SELECT 1 FROM runtime_cancellations stop WHERE stop.company_id=c.company_id AND stop.run_id=c.run_id AND stop.state='pending')
                     OR EXISTS(SELECT 1 FROM confidential_reply_outbox o WHERE o.company_id=c.company_id AND o.run_id=c.run_id AND o.state='pending'))))
             ORDER BY s.community_id LIMIT 129",
        ).bind(company.company_id()).fetch_all(&self.pool).await?;
        if communities.len() > 128 {
            return Err(ControlError::InvalidData(
                "encrypted recovery scope bound exceeded".into(),
            ));
        }
        Ok(communities
            .into_iter()
            .map(|community| CompanyScope::new(company.company_id(), Some(community)))
            .collect())
    }
}
