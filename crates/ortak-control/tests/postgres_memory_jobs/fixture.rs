use super::*;
use ortak_control::memory::MemoryWriteRequest;
use ortak_control::memory_jobs::MemoryWriteJobLease;

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

pub(super) struct Fixture {
    pub pool: PgPool,
    pub control: PgControlPlane,
    pub scope: CompanyScope,
    pub run_id: Uuid,
    pub outbox: OutboxLease,
}

impl Fixture {
    pub async fn new(status: &str, intent: &str) -> Self {
        Self::with_content(status, intent, "published final reply").await
    }
    pub async fn with_content(status: &str, intent: &str, content: &str) -> Self {
        let url = std::env::var("ORTAK_TEST_DATABASE_URL")
            .expect("explicit disposable database URL required");
        let options: sqlx::postgres::PgConnectOptions = url.parse().expect("URL");
        assert!(
            matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432
        );
        let pool = PgPool::connect(&url)
            .await
            .expect("connect disposable database");
        MIGRATED
            .get_or_init(|| async {
                buzz_db::migration::run_migrations(&pool)
                    .await
                    .expect("migration52 once before parallel fixtures");
            })
            .await;
        let company = Uuid::new_v4();
        let community = Uuid::new_v4();
        let revision = Uuid::new_v4();
        let channel = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let outbox_id = Uuid::new_v4();
        sqlx::query("INSERT INTO companies(id,slug,display_name) VALUES ($1,$2,'Memory job test')")
            .bind(company)
            .bind(format!("memory-{}", company.simple()))
            .execute(&pool)
            .await
            .expect("company");
        sqlx::query("INSERT INTO communities(id,host) VALUES ($1,$2)")
            .bind(community)
            .bind(format!("{}.example", community))
            .execute(&pool)
            .await
            .expect("community");
        sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES ($1,$2)")
            .bind(company)
            .bind(community)
            .execute(&pool)
            .await
            .expect("scope binding");
        sqlx::query("INSERT INTO employees(company_id,id) VALUES ($1,'cem')")
            .bind(company)
            .execute(&pool)
            .await
            .expect("employee");
        let binding = serde_json::json!({"adapter":"honcho","endpoint_ref":"honcho:test","workspace":"owned","user_peer":"human","employee_peer":"cem","options":{}});
        let manifest = serde_json::json!({"memory":binding,"office":{"public_key":hex::encode([4u8; 32]),"signer_ref":"secret://test/signer"}});
        sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES ($1,$2,'cem',1,$3,$4,'create')")
            .bind(company).bind(revision).bind(manifest).bind([1u8; 32].as_slice()).execute(&pool).await.expect("pinned immutable revision");
        sqlx::query(
            "UPDATE employees SET status='active',active_revision_id=$2 WHERE company_id=$1",
        )
        .bind(company)
        .bind(revision)
        .execute(&pool)
        .await
        .expect("activate");
        sqlx::query("INSERT INTO employee_memory_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,endpoint_ref,workspace,user_peer,employee_peer,validated_at) VALUES ($1,$2,'cem','honcho','create','honcho:test','owned','human','cem',clock_timestamp())")
            .bind(company).bind(revision).execute(&pool).await.expect("validated memory");
        sqlx::query("INSERT INTO employee_office_bindings(company_id,revision_id,employee_id,provisioning_mode,public_key,signer_ref,verified_at) VALUES ($1,$2,'cem','create',$3,'secret://test/signer',clock_timestamp())")
            .bind(company).bind(revision).bind([4u8; 32].as_slice()).execute(&pool).await.expect("verified Office");
        sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at) VALUES ($1,$2,clock_timestamp(),9,$3,$4,'decided',clock_timestamp())")
            .bind(company).bind([2u8; 32].as_slice()).bind([3u8; 32].as_slice()).bind(channel).execute(&pool).await.expect("canonical source inbox");
        // This fixture exercises control persistence, not the Office normalizer:
        // the runtime's production-seam tests cover full canonical revalidation.
        sqlx::query("INSERT INTO runs(company_id,id,employee_id,employee_revision_id,message_id,runtime_adapter,status,delivery_intent,finished_at) VALUES ($1,$2,'cem',$3,$4,'test',$5,$6,clock_timestamp())")
            .bind(company).bind(run_id).bind(revision).bind([2u8; 32].as_slice()).bind(status).bind(intent).execute(&pool).await.expect("run");
        sqlx::query("INSERT INTO outbox(company_id,id,kind,dedup_key,run_id,signed_event_id,signed_event_bytes) VALUES ($1,$2,'office_publish',$3,$4,$5,$6)")
            .bind(company).bind(outbox_id).bind(format!("publish:{run_id}")).bind(run_id).bind([5u8; 32].as_slice()).bind(b"frozen-by-office".as_slice()).execute(&pool).await.expect("frozen outbox");
        let control = PgControlPlane::new(pool.clone());
        let scope = control
            .resolve_company_for_community(community)
            .await
            .expect("resolve scope");
        if status == "completed" && intent != "silent" {
            let mut tx = pool.begin().await.expect("freeze output");
            let witness = lock_office_authority_on(&mut tx, &scope)
                .await
                .expect("fenced snapshot");
            sqlx::query("UPDATE runtime_office_outputs SET state='enqueued',outbox_id=$3,enqueued_at=clock_timestamp(),draft_kind=9,draft_tags=$4,draft_content=$7,draft_created_at=clock_timestamp(),source_facts='{}',office_authority_generation=$5,office_authority_valid_before=$6,office_authority_token=gen_random_uuid() WHERE company_id=$1 AND run_id=$2")
                .bind(company).bind(run_id).bind(outbox_id).bind(serde_json::json!([["h",channel.to_string()]]))
                .bind(witness.generation()).bind(witness.valid_before()).bind(content).execute(&mut *tx).await.expect("freeze job");
            tx.commit().await.expect("output commit");
        }
        let outbox = control
            .claim_due(
                &scope,
                Some(OutboxKind::OfficePublish),
                "test",
                Duration::from_secs(60),
                1,
            )
            .await
            .expect("claim outbox")
            .remove(0);
        Self {
            pool,
            control,
            scope,
            run_id,
            outbox,
        }
    }

    pub async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM runtime_memory_writes WHERE company_id=$1")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }
    pub async fn claim(&self) -> MemoryWriteJobLease {
        self.control
            .claim_memory_write(&self.scope, "honcho", Duration::from_secs(60))
            .await
            .expect("claim job")
            .expect("due job")
    }
    pub async fn prepare(&self, lease: &MemoryWriteJobLease) -> MemoryWriteRequest {
        let mut tx = self.pool.begin().await.expect("prepare transaction");
        let witness = lock_office_authority_on(&mut tx, &self.scope)
            .await
            .expect("caller-held fence");
        let request = prepare_memory_write_on(&mut tx, &self.scope, lease, &witness)
            .await
            .expect("prepare")
            .expect("live lease");
        tx.commit().await.expect("deferred authority commit");
        request
    }
    pub async fn make_due(&self) {
        sqlx::query("UPDATE runtime_memory_writes SET next_attempt_at=clock_timestamp() WHERE company_id=$1")
            .bind(self.scope.company_id()).execute(&self.pool).await.expect("advance retry clock");
    }
}
