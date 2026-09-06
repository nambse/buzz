//! Default-off central encrypted-DM composition. No independent subscription,
//! pair registration, permission downgrade, memory/Work or ordinary payload I/O.

#[cfg(feature = "encrypted-dm")]
#[path = "worker_encrypted/admission.rs"]
mod admission;
#[cfg(feature = "encrypted-dm")]
#[path = "worker_encrypted/config.rs"]
mod config;
#[cfg(feature = "encrypted-dm")]
#[path = "worker_encrypted/metadata.rs"]
mod metadata;
#[cfg(all(test, feature = "encrypted-dm"))]
#[path = "worker_encrypted/tests.rs"]
mod tests;

use ortak_control::{CompanyScope, PgControlPlane};
use ortak_runtime::hermes::HermesAdapter;
use serde_json::Value;

#[cfg(feature = "encrypted-dm")]
use {
    admission::{PendingAdmission, SourceCursor},
    config::Selection,
    ortak_office::encrypted::{jobs::PgDecryptJobs, key_provider::EnvDmKeyProvider},
    ortak_runtime::{
        encrypted::EncryptedExecution,
        postgres::confidential::{PgConfidentialExecution, PgConfidentialRuns},
    },
    uuid::Uuid,
};

pub struct WorkerEncrypted {
    #[cfg(feature = "encrypted-dm")]
    control: PgControlPlane,
    #[cfg(feature = "encrypted-dm")]
    company: CompanyScope,
    #[cfg(feature = "encrypted-dm")]
    adapter: HermesAdapter,
    #[cfg(feature = "encrypted-dm")]
    selected: Option<Selection>,
    #[cfg(feature = "encrypted-dm")]
    denied: EnvDmKeyProvider,
    #[cfg(feature = "encrypted-dm")]
    capable: bool,
    #[cfg(feature = "encrypted-dm")]
    jobs: PgDecryptJobs,
    #[cfg(feature = "encrypted-dm")]
    runs: PgConfidentialRuns,
    #[cfg(feature = "encrypted-dm")]
    execution: PgConfidentialExecution,
    #[cfg(feature = "encrypted-dm")]
    worker: Uuid,
    #[cfg(feature = "encrypted-dm")]
    after_community: Option<Uuid>,
    #[cfg(feature = "encrypted-dm")]
    pending: Option<PendingAdmission>,
    #[cfg(feature = "encrypted-dm")]
    source_cursor: Option<SourceCursor>,
}

impl WorkerEncrypted {
    /// Public metadata validation only; keys remain purpose-lazy. Invalid or
    /// removed selection pauses new work while the enabled binary still drains
    /// retained keyless obligations. A binary without the feature refuses opt-in.
    pub fn new(
        control: PgControlPlane,
        company: &CompanyScope,
        adapter: HermesAdapter,
        value: Option<Value>,
        capable: bool,
    ) -> Result<Self, &'static str> {
        #[cfg(not(feature = "encrypted-dm"))]
        {
            let _ = (control, company, adapter, capable);
            if value.is_some() {
                return Err("encrypted DM requires the selected binary feature");
            }
            Ok(Self {})
        }
        #[cfg(feature = "encrypted-dm")]
        {
            let selected = match value {
                Some(value) => match Selection::parse(company.company_id(), value) {
                    Ok(selected) => Some(selected),
                    Err(_) => {
                        eprintln!(
                            "ortak-worker: encrypted selection invalid; keyless recovery only"
                        );
                        None
                    }
                },
                None => None,
            };
            Ok(Self {
                jobs: PgDecryptJobs::new(control.pool().clone()),
                runs: PgConfidentialRuns::new(control.pool().clone()),
                execution: PgConfidentialExecution::new(control.pool().clone()),
                control,
                company: company.clone(),
                adapter,
                selected,
                denied: EnvDmKeyProvider::denied(),
                capable,
                worker: Uuid::new_v4(),
                after_community: None,
                pending: None,
                source_cursor: None,
            })
        }
    }

    /// At most one admission/dispatch/observation/seal/publication per tick.
    /// Current pair checks precede each fresh admission and the execution ports
    /// repeat authority around content/effects. Errors propagate; no lost failure
    /// becomes success, and every claimed operation retains its finite lease.
    pub async fn step(&mut self) -> Result<(), &'static str> {
        #[cfg(not(feature = "encrypted-dm"))]
        {
            Ok(())
        }
        #[cfg(feature = "encrypted-dm")]
        {
            // An uncertain admission owns its exact protected object until a
            // bounded receipt replay. Do not let a fresh claim replace it.
            let replaying = self.pending.is_some();
            self.replay_admission().await?;
            let mut scopes = self
                .control
                .confidential_recovery_scopes(&self.company)
                .await
                .map_err(|_| "encrypted recovery scope read failed")?;
            let current = self
                .control
                .resolve_current_encrypted_scope(&self.company)
                .await
                .map_err(|_| "encrypted current scope read failed")?;
            if let Some(scope) = current {
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
            scopes.sort_by_key(CompanyScope::community_id);
            let scope = scopes
                .iter()
                .find(|s| s.community_id() > self.after_community)
                .or_else(|| scopes.first())
                .cloned();
            let Some(scope) = scope else {
                return Ok(());
            };
            self.after_community = scope.community_id();
            let allowed = self.allowed_pairs(&scope).await?;
            let stopping_unselected = self.stop_unselected(&scope, &allowed).await?;
            self.retire_cancelled_copy(&scope).await?;
            self.finalize_failed_job(&scope).await?;
            if !replaying && self.pending.is_none() {
                self.admit_one(&scope, &allowed).await?;
            }
            let keys = if self.capable {
                self.selected
                    .as_ref()
                    .map(|s| &s.keys)
                    .unwrap_or(&self.denied)
            } else {
                &self.denied
            };
            let execute = EncryptedExecution::new(&scope, &self.execution, &self.adapter, keys);
            if stopping_unselected || allowed.is_empty() {
                execute
                    .recover_stop_once()
                    .await
                    .map_err(|_| "encrypted keyless stop unresolved")?;
                return Ok(());
            }
            // A stopped selection was cancelled above before these repository
            // claims. They recheck the exact run; config is never authority.
            execute
                .dispatch_once()
                .await
                .map_err(|_| "encrypted dispatch unresolved")?;
            execute
                .observe_once()
                .await
                .map_err(|_| "encrypted observation unresolved")?;
            execute
                .seal_reply_once()
                .await
                .map_err(|_| "encrypted reply seal unresolved")?;
            if self.capable {
                if let Some(selected) = &self.selected {
                    execute
                        .publish_once(&selected.publisher)
                        .await
                        .map_err(|_| "encrypted publication unresolved")?;
                }
            }
            Ok(())
        }
    }
}
