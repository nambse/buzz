use super::*;
use sqlx::{PgPool, Postgres, Transaction};

pub(super) struct Fixture {
    pub tx: Transaction<'static, Postgres>,
    pub scope: CompanyScope,
    pub community: Uuid,
    pub channel: Uuid,
    pub project: Uuid,
    pub employee: EmployeeId,
    pub human: [u8; 32],
    pub channels: Vec<Uuid>,
    pub employees: Vec<EmployeeId>,
    base: DateTime<Utc>,
}

impl Fixture {
    pub async fn new() -> Self {
        let url = std::env::var("ORTAK_TEST_DATABASE_URL")
            .expect("explicit disposable database URL required");
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert!(
            matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432
        );
        let pool = PgPool::connect_with(options).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let company = Uuid::new_v4();
        let community = Uuid::new_v4();
        let channel = Uuid::new_v4();
        let project = Uuid::new_v4();
        let revision = Uuid::new_v4();
        let human = [3; 32];
        let employee = EmployeeId::parse("conversation-fixture").unwrap();
        sqlx::query("INSERT INTO companies(id,slug,display_name) VALUES($1,$2,'Conversation resolver fixture')")
            .bind(company).bind(format!("conversation-{}",company.simple())).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO communities(id,host) VALUES($1,$2)")
            .bind(community)
            .bind(format!("{community}.example"))
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES($1,$2)")
            .bind(company)
            .bind(community)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO channels(community_id,id,name,created_by,ttl_deadline) VALUES($1,$2,'conversation-source',$3,clock_timestamp()+interval '1 day')")
            .bind(community).bind(channel).bind(human.as_slice()).execute(&mut *tx).await.unwrap();
        for key in [human, [4; 32]] {
            sqlx::query("INSERT INTO users(community_id,pubkey) VALUES($1,$2)")
                .bind(community)
                .bind(key.as_slice())
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES($1,$2,$3)",
            )
            .bind(community)
            .bind(channel)
            .bind(key.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,$2)")
            .bind(company)
            .bind(employee.as_str())
            .execute(&mut *tx)
            .await
            .unwrap();
        let manifest = json!({"office":{"public_key":hex::encode([4u8;32]),"signer_ref":"secret://synthetic/conversation"}});
        sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES($1,$2,$3,1,$4,$5,'adopt')")
            .bind(company).bind(revision).bind(employee.as_str()).bind(manifest).bind([5u8;32].as_slice()).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE employees SET active_revision_id=$3,status='active' WHERE company_id=$1 AND id=$2")
            .bind(company).bind(employee.as_str()).bind(revision).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO employee_office_bindings(company_id,revision_id,employee_id,provisioning_mode,public_key,signer_ref,verified_at) VALUES($1,$2,$3,'adopt',$4,'secret://synthetic/conversation',clock_timestamp())")
            .bind(company).bind(revision).bind(employee.as_str()).bind([4u8;32].as_slice()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO projects(company_id,id,slug,name,created_by_type) VALUES($1,$2,'conversation-fixture','Conversation fixture','system')")
            .bind(company).bind(project).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO project_api_bindings(company_id,project_id,community_id,channel_id,created_by) VALUES($1,$2,$3,$4,$5)")
            .bind(company).bind(project).bind(community).bind(channel).bind(hex::encode(human)).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by) VALUES($1,$2,$3,'viewer',$3)")
            .bind(company).bind(project).bind(hex::encode(human)).execute(&mut *tx).await.unwrap();
        Self {
            tx,
            scope: CompanyScope::new(company, Some(community)),
            community,
            channel,
            project,
            channels: vec![channel],
            employees: vec![employee.clone()],
            employee,
            human,
            base: DateTime::parse_from_rfc3339("2026-09-06T12:00:00.123456Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }
    pub fn time(&self, index: usize) -> DateTime<Utc> {
        self.base - Duration::seconds(index as i64)
    }
    pub async fn event(
        &mut self,
        index: usize,
        parent: Option<(MessageId, usize, MessageId, usize, i32)>,
    ) -> MessageId {
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        let id = MessageId::from_bytes(bytes);
        let mut tags = vec![vec!["h".to_owned(), self.channel.to_string()]];
        if let Some((parent, _, root, _, _)) = parent {
            tags.push(vec!["e".into(), root.to_hex(), "".into(), "root".into()]);
            tags.push(vec!["e".into(), parent.to_hex(), "".into(), "reply".into()]);
        }
        sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,$6,$7,$8)")
            .bind(self.community).bind(id.as_bytes().as_slice()).bind(self.human.as_slice()).bind(self.time(index)).bind(json!(tags))
            .bind(if index==0 {"canonical evidence\nÖ\n".to_owned()}else{format!("reply {index}")}).bind([9u8;64].as_slice()).bind(self.channel).execute(&mut *self.tx).await.unwrap();
        sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at) VALUES($1,$2,$3,9,$4,$5,'decided',clock_timestamp())")
            .bind(self.scope.company_id()).bind(id.as_bytes().as_slice()).bind(self.time(index)).bind(self.human.as_slice()).bind(self.channel).execute(&mut *self.tx).await.unwrap();
        if let Some((parent, parent_index, root, root_index, depth)) = parent {
            sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(self.community).bind(id.as_bytes().as_slice()).bind(self.time(index)).bind(self.channel).bind(parent.as_bytes().as_slice()).bind(self.time(parent_index))
                .bind(root.as_bytes().as_slice()).bind(self.time(root_index)).bind(depth).execute(&mut *self.tx).await.unwrap();
        }
        id
    }
    pub async fn resolve(
        &mut self,
        source: MessageId,
        kind: ConversationAudienceKind,
    ) -> Option<ConversationObservation> {
        let scope = self.scope.clone();
        let employee = self.employee.clone();
        let human = self.human;
        let request = ConversationReadRequest {
            scope: &scope,
            project_id: self.project,
            employee_id: &employee,
            human_public_key: &human,
            channel_grants: &self.channels,
            employee_grants: &self.employees,
            source_message_id: source,
            audience_kind: kind,
        };
        let observation = resolve_conversation_on(&mut self.tx, &request)
            .await
            .unwrap();
        super::parity::compare(&mut self.tx, &request, observation.as_ref()).await;
        observation
    }
    pub async fn resolve_with(
        &mut self,
        source: MessageId,
        channels: &[Uuid],
        employees: &[EmployeeId],
    ) -> Option<ConversationObservation> {
        // SQL intentionally has no API ceiling. Only the regular resolve path
        // compares SQL; these empty/narrow ceiling tests remain facade-specific.
        let request = ConversationReadRequest {
            scope: &self.scope,
            project_id: self.project,
            employee_id: &self.employee,
            human_public_key: &self.human,
            channel_grants: channels,
            employee_grants: employees,
            source_message_id: source,
            audience_kind: ConversationAudienceKind::Thread,
        };
        resolve_conversation_on(&mut self.tx, &request)
            .await
            .unwrap()
    }
    pub async fn fault(&mut self, sql: &'static str) {
        sqlx::query("SAVEPOINT conversation_fault")
            .execute(&mut *self.tx)
            .await
            .unwrap();
        // Bind both slots without relying on an untyped unused PostgreSQL parameter.
        let mut query = sqlx::QueryBuilder::<Postgres>::new(sql);
        query
            .push(" AND ")
            .push_bind(self.scope.company_id())
            .push("::uuid IS NOT NULL AND ")
            .push_bind(self.community)
            .push("::uuid IS NOT NULL");
        query.build().execute(&mut *self.tx).await.unwrap();
    }
    pub async fn restore(&mut self) {
        sqlx::query("ROLLBACK TO SAVEPOINT conversation_fault")
            .execute(&mut *self.tx)
            .await
            .unwrap();
        sqlx::query("RELEASE SAVEPOINT conversation_fault")
            .execute(&mut *self.tx)
            .await
            .unwrap();
    }
}
