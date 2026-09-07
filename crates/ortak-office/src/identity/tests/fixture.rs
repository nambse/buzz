use super::*;
use base64::Engine;
use sqlx::{PgPool, Row};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

pub(super) struct Fixture {
    pub(super) pool: PgPool,
    pub(super) config: OfficeIdentityConfig,
    pub(super) signer: EnvOfficeSigner,
    pub(super) key: String,
    listener: Option<TcpListener>,
}

#[derive(Clone, Copy)]
pub(super) enum Reply {
    Accepted,
    LostAck,
    AckOnly,
    Oversized,
    Rejected,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let url = std::env::var("ORTAK_TEST_DATABASE_URL")
            .expect("explicit disposable database required");
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert!(
            matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432,
            "identity tests require disposable localhost:55432"
        );
        let pool = PgPool::connect(&url).await.unwrap();
        MIGRATED
            .get_or_init(|| async {
                buzz_db::migration::run_migrations(&pool).await.unwrap();
            })
            .await;
        let (mut config, signer) = configuration();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
        config.origin = format!("http://{host}");
        let entry = &config.employees[0];
        let public_key = OfficePublicKey::parse_hex(&entry.office.public_key).unwrap();
        sqlx::query("INSERT INTO communities(id,host) VALUES ($1,$2)")
            .bind(config.community_id)
            .bind(host)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO companies(id,slug,display_name) VALUES ($1,$2,'Office identity fixture')",
        )
        .bind(config.company_id)
        .bind(format!("identity-{}", config.company_id.simple()))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES ($1,$2)")
            .bind(config.company_id)
            .bind(config.community_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO employees(company_id,id) VALUES ($1,$2)")
            .bind(config.company_id)
            .bind(entry.employee_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO relay_members(community_id,pubkey,role) VALUES ($1,$2,'member')")
            .bind(config.community_id)
            .bind(&entry.office.public_key)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO users(community_id,pubkey) VALUES ($1,$2)")
            .bind(config.community_id)
            .bind(public_key.as_bytes().as_slice())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO channels(community_id,id,name,created_by) VALUES ($1,$2,'identity-office',$3)")
            .bind(config.community_id).bind(entry.channels[0]).bind(public_key.as_bytes().as_slice())
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES ($1,$2,$3,'bot')")
            .bind(config.community_id).bind(entry.channels[0]).bind(public_key.as_bytes().as_slice())
            .execute(&pool).await.unwrap();
        let operation = Uuid::new_v4();
        let key = format!("provisioning:{operation}:publish_office_profile");
        let manifest = serde_json::json!({"schema_version": "ortak.employee/v0", "provisioning": "adopt",
            "employee": {"id": entry.employee_id, "name": "Ada", "office": entry.office}});
        sqlx::query("INSERT INTO provisioning_operations(company_id,id,employee_id,mode,dry_run,idempotency_key,manifest,manifest_fingerprint,status) VALUES ($1,$2,$3,'adopt',false,$4,$5,$6,'running')")
            .bind(config.company_id).bind(operation).bind(entry.employee_id.as_str()).bind(format!("test:{operation}"))
            .bind(manifest).bind(vec![0_u8;32]).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provisioning_operation_steps(company_id,operation_id,step_index,step_name,state,idempotency_key) VALUES ($1,$2,7,'publish_office_profile','running',$3)")
            .bind(config.company_id).bind(operation).bind(&key).execute(&pool).await.unwrap();
        Self {
            pool,
            config,
            signer,
            key,
            listener: Some(listener),
        }
    }

    pub(super) fn adapter(&self) -> PgOfficeIdentityAdapter {
        PgOfficeIdentityAdapter::new(
            PgControlPlane::new(self.pool.clone()),
            self.signer.clone(),
            self.config.clone(),
            Duration::from_secs(3),
        )
        .unwrap()
    }

    pub(super) fn entry(&self) -> &OfficeIdentityEmployee {
        &self.config.employees[0]
    }

    pub(super) async fn publish(
        &self,
        adapter: &PgOfficeIdentityAdapter,
    ) -> Result<ProfilePublication, OfficeIdentityError> {
        adapter
            .publish_profile(
                &self.entry().employee_id,
                &self.entry().office,
                "Ada",
                &self.key,
            )
            .await
    }

    pub(super) async fn journal(&self) -> (Vec<u8>, bool) {
        sqlx::query_as("SELECT signed_event_bytes,acknowledged_at IS NOT NULL FROM office_identity_profiles WHERE company_id=$1 AND idempotency_key=$2")
            .bind(self.config.company_id).bind(&self.key).fetch_one(&self.pool).await.unwrap()
    }

    pub(super) async fn journal_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM office_identity_profiles WHERE company_id=$1")
            .bind(self.config.company_id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// This is a fixture HTTP relay, not a real relay acceptance claim. It
    /// verifies NIP-98 and pre-network durable bytes, then mirrors only the
    /// canonical event/profile rows the production adapter must observe.
    pub(super) fn serve(&mut self, replies: Vec<Reply>) -> tokio::task::JoinHandle<Vec<Vec<u8>>> {
        let listener = self.listener.take().unwrap();
        let pool = self.pool.clone();
        let config = self.config.clone();
        tokio::spawn(async move {
            let mut bodies = Vec::new();
            for reply in replies {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(8), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut bytes = Vec::new();
                let (header_end, content_length) = loop {
                    assert!(bytes.len() <= 32768, "bounded fixture request");
                    let mut chunk = [0_u8; 2048];
                    let count =
                        tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
                            .await
                            .unwrap()
                            .unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8(bytes[..index].to_vec()).unwrap();
                        let length: usize = header
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        assert!(length <= 16384);
                        if bytes.len() >= index + 4 + length {
                            break (index + 4, length);
                        }
                    }
                };
                let body = bytes[header_end..header_end + content_length].to_vec();
                let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                assert!(header.starts_with("POST /events HTTP/1.1\r\n"));
                let auth = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("authorization")
                            .then(|| value.trim())
                    })
                    .unwrap()
                    .strip_prefix("Nostr ")
                    .unwrap();
                let auth = String::from_utf8(
                    base64::engine::general_purpose::STANDARD
                        .decode(auth)
                        .unwrap(),
                )
                .unwrap();
                let actual = buzz_auth::verify_nip98_event(
                    &auth,
                    &format!("{}/events", config.origin),
                    "POST",
                    Some(&body),
                )
                .unwrap();
                assert_eq!(actual.to_hex(), config.employees[0].office.public_key);
                let row = sqlx::query(
                    "SELECT signed_event_bytes FROM office_identity_profiles WHERE company_id=$1",
                )
                .bind(config.company_id)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(
                    row.get::<Vec<u8>, _>("signed_event_bytes"),
                    body,
                    "persist before HTTP"
                );
                let event = nostr::Event::from_json(&body).unwrap();
                event.verify().unwrap();
                if matches!(reply, Reply::Accepted | Reply::LostAck) {
                    mirror_profile(&pool, &config, &event).await;
                }
                bodies.push(body);
                if matches!(reply, Reply::LostAck) {
                    continue;
                }
                let response = if matches!(reply, Reply::Oversized) {
                    "x".repeat(8193)
                } else {
                    serde_json::json!({"event_id":event.id.to_hex(),"accepted":!matches!(reply,Reply::Rejected),"message":""}).to_string()
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
            }
            bodies
        })
    }
}

async fn mirror_profile(pool: &PgPool, config: &OfficeIdentityConfig, event: &nostr::Event) {
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig) VALUES ($1,$2,$3,$4,0,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(config.community_id).bind(event.id.as_bytes().as_slice()).bind(event.pubkey.to_bytes().to_vec())
        .bind(chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64,0).unwrap())
        .bind(serde_json::json!([])).bind(&event.content).bind(event.sig.serialize().to_vec())
        .execute(pool).await.unwrap();
    sqlx::query("UPDATE users SET display_name='Ada' WHERE community_id=$1 AND pubkey=$2")
        .bind(config.community_id)
        .bind(event.pubkey.to_bytes().to_vec())
        .execute(pool)
        .await
        .unwrap();
}
