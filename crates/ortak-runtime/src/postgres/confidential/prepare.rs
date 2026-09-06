use chrono::Utc;
use ortak_control::{CompanyScope, confidential::{ValidatedIdentity,PayloadPurpose}};
use ortak_domain::{Employee,PermissionPolicy};
use ortak_office::encrypted::{VerifiedDmRumor,jobs::{DmDecryptClaim,PgDecryptJobs},key_provider::{DmKeySelection,EnvDmKeyProvider}};
use sqlx::Row;
use uuid::Uuid;
use zeroize::{Zeroize,Zeroizing};

use super::{wire,ConfidentialAdmissionError as Error,PgConfidentialRuns,PreparedConfidentialRun,ProtectedConfidentialRun,Result};

impl PgConfidentialRuns {
    /// Derives protected identity and volatile input from the exact production
    /// crypto result and current verified job. No transaction spans protection.
    /// A fresh key ID is created only here; callers cannot substitute identity.
    pub async fn prepare(&self,scope:&CompanyScope,claim:&DmDecryptClaim,verified:&VerifiedDmRumor)->Result<PreparedConfidentialRun>{
        let mut tx=self.pool.begin().await?;
        PgDecryptJobs::lock_verified_on(&mut tx,scope,claim).await?;
        let source=claim.expected().outer_id().to_bytes().to_vec();
        let row=sqlx::query("SELECT j.seal_id,j.seal_created_at,j.rumor_id,j.rumor_created_at,j.rumor_hash,j.source_hash,j.reply_to,r.manifest,ortak_confidential_runtime_binding(j.company_id,j.employee_revision_id) AS binding FROM encrypted_dm_decrypt_jobs j JOIN employee_revisions r ON r.company_id=j.company_id AND r.employee_id=j.employee_id AND r.id=j.employee_revision_id WHERE j.company_id=$1 AND j.source_id=$2")
            .bind(scope.company_id()).bind(&source).fetch_one(&mut *tx).await?;
        if row.try_get::<Vec<u8>,_>("seal_id")?!=verified.seal_id().to_bytes()
            || row.try_get::<chrono::DateTime<Utc>,_>("seal_created_at")?!=verified.seal_created_at()
            || row.try_get::<Vec<u8>,_>("rumor_id")?!=verified.rumor_id().to_bytes()
            || row.try_get::<chrono::DateTime<Utc>,_>("rumor_created_at")?!=verified.rumor_created_at()
            || row.try_get::<Vec<u8>,_>("rumor_hash")?!=verified.rumor_hash().as_slice()
            || row.try_get::<Vec<u8>,_>("source_hash")?!=verified.outer_hash().as_slice()
            || row.try_get::<Option<Vec<u8>>,_>("reply_to")?!=verified.reply_to().map(|id|id.to_bytes().to_vec())
            || verified.source()!=claim.expected() { return Err(Error::Refused); }
        let employee:Employee=serde_json::from_value(row.try_get("manifest")?).map_err(|_|Error::Refused)?;
        employee.validate_definition().map_err(|_|Error::Refused)?;
        let binding=serde_json::from_value(row.try_get::<serde_json::Value,_>("binding")?).map_err(|_|Error::Refused)?;
        if employee.runtime!=binding || employee.permissions!=PermissionPolicy::default() { return Err(Error::Refused); }
        let run:Uuid=sqlx::query_scalar("SELECT ortak_confidential_dm_run_id($1,$2)")
            .bind(scope.company_id()).bind(&source).fetch_one(&mut *tx).await?;
        let key_id=Uuid::new_v4();
        let identity:Vec<u8>=sqlx::query_scalar("SELECT ortak_confidential_dm_identity($1,$2,$3,$4)")
            .bind(scope.company_id()).bind(&source).bind(run).bind(key_id).fetch_one(&mut *tx).await?;
        let identity=ValidatedIdentity::parse(&identity).map_err(|_|Error::Payload)?;
        let start_key=crate::run_idempotency_key(scope.company_id(),run);
        let reply=verified.reply_to().map(|id|id.to_hex());
        let plaintext=wire::snapshot(&identity,&binding,run,claim.identity().employee_revision_id,
            claim.identity().employee_id.as_str(),&start_key,&claim.identity().channel_id.to_string(),reply.as_deref(),verified.text())?;
        tx.commit().await?;
        Ok(PreparedConfidentialRun {identity,plaintext,source_id:source,run_id:run,key_id,
            signer_ref:claim.identity().decrypt_ref.clone(),deadline:claim.expires_at()})
    }
}

impl PreparedConfidentialRun {
    /// Generates one master, wraps through the explicit purpose provider and
    /// seals exactly the prepared bytes. This never downgrades a Files policy.
    /// Keep this result for an uncertain commit retry; do not generate new bytes.
    pub fn protect(mut self,provider:&EnvDmKeyProvider)->Result<ProtectedConfidentialRun>{
        if Utc::now()>=self.deadline { return Err(Error::Refused); }
        let mut master=Zeroizing::new([0u8;32]);
        getrandom::fill(master.as_mut()).map_err(|_|Error::Unavailable)?;
        let selected=DmKeySelection::from_expected_claims(&self.identity,self.signer_ref.clone());
        let wrapped=provider.wrap_master(&selected,&master).map_err(|error| match error {
            ortak_office::encrypted::key_provider::DmKeyError::Unavailable=>Error::Unavailable,
            _=>Error::Refused,
        })?;
        let master=crate::confidential::ConfidentialMasterKey::from_owned(master);
        let snapshot=crate::confidential::seal(&master,&self.identity,PayloadPurpose::Snapshot,0,&self.plaintext)
            .map_err(|_|Error::Payload)?;
        self.plaintext.zeroize();
        if Utc::now()>=self.deadline { return Err(Error::Refused); }
        Ok(ProtectedConfidentialRun { prepared:self,snapshot,wrapped })
    }
}
