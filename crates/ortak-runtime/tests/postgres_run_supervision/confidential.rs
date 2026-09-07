//! Requires a disposable migrated database with BOTH unnumbered encrypted SQL
//! fragments installed. The explicit synthetic key environment below must be
//! exactly 32 bytes of 0x31 in hex; never point it at real credential material.
use super::*;
use chrono::TimeZone;
use nostr::{Event, EventBuilder, JsonUtil, Keys, SecretKey, UnsignedEvent};
use ortak_control::office_identity::OfficePublicKey;
use ortak_office::{
    encrypted::{
        decode,
        jobs::{ConfiguredDmPair, DmDecryptClaim, DmOuterSource, PgDecryptJobs},
        key_provider::{DmOfficeKeyBinding, EnvDmKeyProvider, OfficeKeyPurpose},
        DmDecryptKey,
    },
    transport::OfficeSignerBinding,
};
use ortak_runtime::postgres::confidential::{PgConfidentialRuns, ProtectedConfidentialRun};

#[path = "../../../ortak-control/tests/direct_channel_support.rs"]
mod direct_support;
mod execution;

struct EncryptedFixture {
    f: Fixture,
    human: Keys,
    employee: Keys,
    channel: Uuid,
    pair: ConfiguredDmPair,
    jobs: PgDecryptJobs,
    protected: PgConfidentialRuns,
}
impl EncryptedFixture {
    async fn new() -> Self {
        // This is a public synthetic test vector, supplied explicitly to the
        // actual production environment-key provider; no unsafe env mutation.
        assert_eq!(
            std::env::var("ORTAK_TEST_CONFIDENTIAL_SYNTHETIC_KEY").unwrap(),
            "31".repeat(32)
        );
        let employee = Keys::new(SecretKey::from_slice(&[0x31; 32]).unwrap());
        let human = Keys::new(SecretKey::from_slice(&[0x32; 32]).unwrap());
        let mut definition = fixture_employee();
        definition.office.public_key = employee.public_key().to_hex();
        definition.permissions = PermissionPolicy::default();
        let f = Fixture::new_for_employee(definition).await;
        let channel = direct_support::create(
            &f.pool,
            f.community_id,
            &[
                human.public_key().to_bytes(),
                employee.public_key().to_bytes(),
            ],
        )
        .await;
        direct_support::select(&f.control, &f.scope, channel, &f.employee.id).await;
        let binding:Uuid=sqlx::query_scalar("SELECT id FROM employee_office_bindings WHERE company_id=$1 AND employee_id=$2 AND public_key=$3")
            .bind(f.scope.company_id()).bind(f.employee.id.as_str()).bind(employee.public_key().to_bytes().as_slice()).fetch_one(&f.pool).await.unwrap();
        let pair = ConfiguredDmPair {
            selection_id: Uuid::new_v4(),
            channel_id: channel,
            employee_id: f.employee.id.clone(),
            human_public_key: OfficePublicKey::parse_hex(&human.public_key().to_hex()).unwrap(),
            employee_public_key: OfficePublicKey::parse_hex(&employee.public_key().to_hex())
                .unwrap(),
            office_binding_id: binding,
            key_version: 0,
            decrypt_ref: f.employee.office.signer_ref.clone(),
        };
        let jobs = PgDecryptJobs::new(f.pool.clone());
        assert_eq!(jobs.register_pair(&f.scope, &pair).await.unwrap(), 1);
        let protected = PgConfidentialRuns::new(f.pool.clone());
        Self {
            f,
            human,
            employee,
            channel,
            pair,
            jobs,
            protected,
        }
    }
    fn rumor(&self) -> UnsignedEvent {
        let mut rumor = EventBuilder::private_msg_rumor(
            self.employee.public_key(),
            "Protected admission canary\nİ 🧭",
        )
        .build(self.human.public_key());
        rumor.ensure_id();
        rumor
    }
    async fn outer(&self, rumor: UnsignedEvent) -> Event {
        EventBuilder::gift_wrap(&self.human, &self.employee.public_key(), rumor, [])
            .await
            .unwrap()
    }
    async fn accept(&self, outer: &Event) -> DmDecryptClaim {
        let at = Utc
            .timestamp_opt(outer.created_at.as_secs() as i64, 0)
            .single()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&outer.as_json()).unwrap();
        sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,1059,$5,$6,$7,NULL)")
            .bind(self.f.community_id).bind(outer.id.to_bytes().as_slice()).bind(outer.pubkey.to_bytes().as_slice()).bind(at)
            .bind(&value["tags"]).bind(&outer.content).bind(hex::decode(value["sig"].as_str().unwrap()).unwrap()).execute(&self.f.pool).await.unwrap();
        self.f
            .control
            .insert_accepted_event(
                self.f.community_id,
                &InboxEvent {
                    event_id: MessageId::from_bytes(outer.id.to_bytes()),
                    event_created_at: at,
                    event_kind: 1059,
                    author_pubkey: outer.pubkey.to_bytes(),
                    channel_id: None,
                },
            )
            .await
            .unwrap();
        assert!(self
            .jobs
            .enqueue(
                &self.f.scope,
                self.pair.selection_id,
                &DmOuterSource::new(outer.id, at).unwrap()
            )
            .await
            .unwrap());
        let claim = self
            .jobs
            .claim_next(&self.f.scope, Uuid::new_v4())
            .await
            .unwrap()
            .unwrap();
        assert!(self
            .jobs
            .claim_is_current(&self.f.scope, &claim)
            .await
            .unwrap());
        let verified = decode(
            &DmDecryptKey::for_recipient(&self.employee, self.employee.public_key()).unwrap(),
            claim.expected(),
            claim.outer_bytes(),
        )
        .unwrap();
        self.jobs
            .record_verified(&self.f.scope, &claim, &verified)
            .await
            .unwrap();
        claim
    }
    async fn prepare(&self, claim: &DmDecryptClaim) -> ProtectedConfidentialRun {
        let verified = decode(
            &DmDecryptKey::for_recipient(&self.employee, self.employee.public_key()).unwrap(),
            claim.expected(),
            claim.outer_bytes(),
        )
        .unwrap();
        let prepared = self
            .protected
            .prepare(&self.f.scope, claim, &verified)
            .await
            .unwrap();
        let provider = EnvDmKeyProvider::new(vec![DmOfficeKeyBinding {
            signer: OfficeSignerBinding {
                company_id: self.f.scope.company_id(),
                employee_id: self.f.employee.id.clone(),
                signer_ref: self.pair.decrypt_ref.clone(),
                public_key: self.pair.employee_public_key,
                secret_env: "ORTAK_TEST_CONFIDENTIAL_SYNTHETIC_KEY".into(),
            },
            office_binding_id: self.pair.office_binding_id,
            key_version: 0,
            purposes: vec![OfficeKeyPurpose::WrapMaster, OfficeKeyPurpose::UnwrapMaster],
        }])
        .unwrap();
        prepared.protect(&provider).unwrap()
    }
    async fn remove_human(&self, removed: bool) {
        sqlx::query("UPDATE channel_members SET removed_at=CASE WHEN $4 THEN clock_timestamp() ELSE NULL END WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
            .bind(self.f.community_id).bind(self.channel).bind(self.human.public_key().to_bytes().as_slice()).bind(removed).execute(&self.f.pool).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires disposable55432 + encrypted jobs/admission SQL and explicit synthetic key env"]
async fn confidential_atomic_ciphertext_admission_dedupes_wrappers_and_quarantines_ordinary_paths()
{
    let x = EncryptedFixture::new().await;
    let rumor = x.rumor();
    let first = x.outer(rumor.clone()).await;
    let claim = x.accept(&first).await;
    let protected = x.prepare(&claim).await;
    let receipt = x
        .protected
        .commit(&x.f.scope, &claim, &protected)
        .await
        .unwrap();
    assert!(!receipt.duplicate_rumor);
    assert_eq!(
        x.protected
            .commit(&x.f.scope, &claim, &protected)
            .await
            .unwrap(),
        receipt
    );
    let second = x.outer(rumor).await;
    let claim2 = x.accept(&second).await;
    let protected2 = x.prepare(&claim2).await;
    let duplicate = x
        .protected
        .commit(&x.f.scope, &claim2, &protected2)
        .await
        .unwrap();
    assert!(duplicate.duplicate_rumor);
    assert_eq!(duplicate.run_id, receipt.run_id);
    let counts:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM runs WHERE company_id=$1),(SELECT count(*) FROM confidential_dm_receipts WHERE company_id=$1),(SELECT count(*) FROM confidential_run_dispatches WHERE company_id=$1),(SELECT count(*) FROM run_events WHERE company_id=$1),(SELECT count(*) FROM run_context_snapshots WHERE company_id=$1)")
        .bind(x.f.scope.company_id()).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(counts, (1, 2, 1, 0, 0));
    assert!(x
        .jobs
        .claim_next(&x.f.scope, Uuid::new_v4())
        .await
        .unwrap()
        .is_none());
    let original:(Vec<u8>,Option<Uuid>,i32)=sqlx::query_as("SELECT author_pubkey,channel_id,event_kind FROM office_inbox WHERE company_id=$1 AND event_id=$2")
        .bind(x.f.scope.company_id()).bind(first.id.to_bytes().as_slice()).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(original, (first.pubkey.to_bytes().to_vec(), None, 1059));
    // Valid ordinary append except for its confidential run. Guard removal makes
    // this INSERT succeed: no legacy snapshot validator masks the quarantine.
    let error=sqlx::query("INSERT INTO run_events(company_id,run_id,sequence,event_type,occurred_at,payload) VALUES($1,$2,0,'run.queued',clock_timestamp(),'{\"type\":\"run.queued\"}'::jsonb)")
        .bind(x.f.scope.company_id()).bind(receipt.run_id).execute(&x.f.pool).await.unwrap_err();
    assert!(error.to_string().contains("ordinary content path"));
    let mut tx = x.f.pool.begin().await.unwrap();
    assert!(
        PgConfidentialRuns::load_current_on(&mut tx, &x.f.scope, receipt.run_id)
            .await
            .unwrap()
            .is_some()
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable55432 + encrypted jobs/admission SQL and explicit synthetic key env"]
async fn confidential_revoke_restore_never_revives_content_but_commit_receipt_and_cancel_survive() {
    let x = EncryptedFixture::new().await;
    let outer = x.outer(x.rumor()).await;
    let claim = x.accept(&outer).await;
    let protected = x.prepare(&claim).await;
    let receipt = x
        .protected
        .commit(&x.f.scope, &claim, &protected)
        .await
        .unwrap();
    x.remove_human(true).await;
    x.remove_human(false).await;
    let mut tx = x.f.pool.begin().await.unwrap();
    assert!(
        PgConfidentialRuns::load_current_on(&mut tx, &x.f.scope, receipt.run_id)
            .await
            .unwrap()
            .is_none()
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        x.protected
            .commit(&x.f.scope, &claim, &protected)
            .await
            .unwrap(),
        receipt
    );
    assert!(x
        .protected
        .cancel(&x.f.scope, receipt.run_id)
        .await
        .unwrap());
    let pending:i64=sqlx::query_scalar("SELECT count(*) FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='pending'")
        .bind(x.f.scope.company_id()).bind(receipt.run_id).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(pending, 1);
}

#[tokio::test]
#[ignore = "requires disposable55432 + encrypted jobs/admission SQL and explicit synthetic key env"]
async fn confidential_revoke_after_protection_rolls_back_every_admission_effect() {
    let x = EncryptedFixture::new().await;
    let outer = x.outer(x.rumor()).await;
    let claim = x.accept(&outer).await;
    let protected = x.prepare(&claim).await;
    x.remove_human(true).await;
    assert!(x
        .protected
        .commit(&x.f.scope, &claim, &protected)
        .await
        .is_err());
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM runs WHERE company_id=$1),(SELECT count(*) FROM routing_decisions WHERE company_id=$1),(SELECT count(*) FROM confidential_dm_receipts WHERE company_id=$1)")
        .bind(x.f.scope.company_id()).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
}
