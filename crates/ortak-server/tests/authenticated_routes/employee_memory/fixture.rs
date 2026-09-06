use super::*;

pub(super) const PATH: &str = "/api/v1/employees/cem/reviewed-memory";
pub(super) struct MemoryFixture {
    pub f: Fixture,
    pub app: Router,
    pub source: String,
}
impl MemoryFixture {
    pub async fn new(actor_is_reader: bool) -> Self {
        Self::configured(actor_is_reader, false).await
    }
    pub async fn new_owned() -> Self {
        Self::configured(false, true).await
    }
    async fn configured(actor_is_reader: bool, owned: bool) -> Self {
        let f = Fixture::new().await;
        employee(&f, owned).await;
        let actor = if actor_is_reader {
            &f.reader
        } else {
            &f.operator
        };
        let app = app(
            &f,
            actor,
            true,
            Role::Reader,
            vec![f.channel, f.hidden],
            vec!["cem", "other"],
        );
        let source = source(&f, actor, f.channel).await;
        Self { f, app, source }
    }
    pub async fn preview(&self, actor: &Keys, kind: &str) -> Value {
        let body = json!({"source_event_id":self.source,"destination_channel_id":self.f.hidden,
            "kind":kind,"human_public_key":if kind=="relationship" {Some(actor.public_key().to_hex())} else {None}});
        let (status, value) = post(&self.app, actor, &format!("{PATH}/preview"), &body).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert!(
            !value
                .to_string()
                .contains("Original source private fixture")
        );
        value["preview"].clone()
    }
    pub fn command(&self, preview: &Value) -> Value {
        json!({"operation_id":Uuid::new_v4(),"fact":{
            "source_event_id":self.source,"source_event_created_at":preview["source"]["event_created_at"],
            "destination_channel_id":self.f.hidden,"kind":preview["audience"]["kind"],
            "human_public_key":preview["audience"]["human_public_key"],
            "expected_audience_hash":preview["audience_hash"],"content":"The human edited this shared lesson.",
            "expires_at":(Utc::now()+chrono::Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Micros,true),"reviewed":true}})
    }
    pub async fn counts(&self) -> (i64, i64, i64) {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM employee_reviewed_memory_facts WHERE company_id=$1),
            (SELECT count(*) FROM employee_reviewed_memory_operations WHERE company_id=$1),
            (SELECT count(*) FROM employee_reviewed_memory_exports WHERE company_id=$1)",
        )
        .bind(self.f.company)
        .fetch_one(&self.f.pool)
        .await
        .unwrap()
    }
}
pub(super) fn app(
    f: &Fixture,
    actor: &Keys,
    capable: bool,
    role: Role,
    channels: Vec<Uuid>,
    employees: Vec<&str>,
) -> Router {
    let mut conf = config(f.community, actor, f.channel);
    conf.humans[0].role = role;
    conf.humans[0].can_review_employee_memory = capable;
    conf.humans[0].channel_ids = channels;
    conf.humans[0].employee_ids = employees
        .into_iter()
        .map(|e| EmployeeId::parse(e).unwrap())
        .collect();
    product_router(f.control.clone(), conf, Arc::new(Replay::default())).unwrap()
}
pub(super) async fn post(
    app: &Router,
    actor: &Keys,
    path: &str,
    body: &Value,
) -> (StatusCode, Value) {
    response(app, signed(actor, "POST", path, &body.to_string(), true)).await
}
pub(super) async fn get(app: &Router, actor: &Keys, path: &str) -> (StatusCode, Value) {
    response(app, signed(actor, "GET", path, "", false)).await
}
pub(super) async fn source(f: &Fixture, actor: &Keys, channel: Uuid) -> String {
    let event = EventBuilder::new(Kind::Custom(9), "Original source private fixture")
        .tags([
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(actor)
        .unwrap();
    let at = DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
    sqlx::query(
        "INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
        VALUES($1,$2,$3,$4,9,$5,$6,$7,$8)",
    )
    .bind(f.community)
    .bind(event.id.to_bytes().as_slice())
    .bind(actor.public_key().to_bytes().as_slice())
    .bind(at)
    .bind(serde_json::to_value(&event.tags).unwrap())
    .bind(&event.content)
    .bind(event.sig.serialize().as_slice())
    .bind(channel)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at)
        VALUES($1,$2,$3,9,$4,$5,'decided',now())")
        .bind(f.company).bind(event.id.to_bytes().as_slice()).bind(at).bind(actor.public_key().to_bytes().as_slice()).bind(channel)
        .execute(&f.pool).await.unwrap();
    event.id.to_hex()
}
async fn employee(f: &Fixture, owned: bool) {
    // Real canonical identity binding, with no runtime/fact/use/ACK fabrication.
    let mut employee = serde_yaml::from_str::<ortak_domain::EmployeeManifest>(include_str!(
        "../../../../../config/employees/cem.yaml"
    ))
    .unwrap()
    .employee;
    employee.status = ortak_domain::EmployeeStatus::Active;
    employee.office.public_key = Keys::generate().public_key().to_hex();
    employee.office.signer_ref =
        ortak_domain::CredentialRef::parse("credential://fixture/employee-memory").unwrap();
    if owned {
        let memory = employee.memory.as_mut().unwrap();
        memory.options.clear();
        memory.workspace = format!("employee_reviewed_{}", Uuid::new_v4().simple());
    }
    let revision = Uuid::new_v4();
    let manifest = serde_json::to_value(&employee).unwrap();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
        VALUES($1,$2,'cem',2,$3,$4,'adopt')")
        .bind(f.company).bind(revision).bind(&manifest).bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES($1,'cem',$2,'adopt',$3,$4,now())")
        .bind(f.company).bind(revision).bind(hex::decode(&employee.office.public_key).unwrap()).bind(employee.office.signer_ref.as_str())
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE employees SET active_revision_id=$2 WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .bind(revision)
        .execute(&f.pool)
        .await
        .unwrap();
    if owned {
        let m = employee.memory.as_ref().unwrap();
        sqlx::query("INSERT INTO employee_memory_bindings(company_id,employee_id,revision_id,provisioning_mode,adapter,endpoint_ref,workspace,user_peer,employee_peer,options,validated_at)
            VALUES($1,'cem',$2,'adopt',$3,$4,$5,$6,$7,$8,clock_timestamp())")
            .bind(f.company).bind(revision).bind(&m.adapter).bind(&m.endpoint_ref).bind(&m.workspace).bind(&m.user_peer).bind(&m.employee_peer)
            .bind(serde_json::to_value(&m.options).unwrap()).execute(&f.pool).await.unwrap();
    }
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,'other')")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    for channel in [f.channel, f.hidden] {
        sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')")
            .bind(f.community).bind(channel).bind(hex::decode(&employee.office.public_key).unwrap()).execute(&f.pool).await.unwrap();
    }
}
