use super::*;

pub(super) const EMPLOYEE_FACT: &str =
    "Deployment lesson explicitly shared by the human with this employee.";
pub(super) const PATH: &str = "/api/v1/employees/cem/reviewed-memory";

pub(super) struct EmployeeFixture {
    pub c: ConversationFixture,
    pub remote: Remote,
    pub app: Router,
    pub target: Uuid,
    pub fact: Uuid,
    pub until: DateTime<Utc>,
}
impl EmployeeFixture {
    pub async fn new(kind: &str, include_project: bool) -> Self {
        let mut c = ConversationFixture::new_owned().await;
        if include_project {
            c.memory.project_enabled = true;
            c.x.target.runtime_consumption_enabled = true;
            super::super::super::conversation_publication::advertise(&c.x).await;
            c.x.publish().await;
            assert!(
                schedule_one(&c.x.f.control, &c.x.scope, &ObservedAdapter::default())
                    .await
                    .unwrap()
            );
            c.memory
                .contents
                .lock()
                .unwrap()
                .insert(c.x.fact, PROJECT_FACT.into());
        }
        let mut cfg = config(c.x.f.community, &c.x.f.operator, c.x.f.channel);
        cfg.humans[0].can_review_employee_memory = true;
        let app = product_router(c.x.f.control.clone(), cfg, Arc::new(Replay::default())).unwrap();
        let remote = Remote::new_on(&c.x.f).await;
        let (target, until) = remote.register_on(&c.x.f, c.x.f.channel).await;
        // Explicit operator advertisement fixture; this is the exact guarded
        // target switch, never a synthetic use/snapshot/publication receipt.
        sqlx::query("UPDATE employee_reviewed_memory_targets SET runtime_consumption_enabled=true WHERE company_id=$1 AND id=$2")
            .bind(c.x.f.company).bind(target).execute(&c.x.f.pool).await.unwrap();
        let human = (kind == "relationship").then(|| c.x.f.operator.public_key().to_hex());
        let (status, preview) = post(&app, &c.x.f.operator, &format!("{PATH}/preview"),
            &json!({"source_event_id":c.x.source,"destination_channel_id":c.x.f.channel,"kind":kind,"human_public_key":human})).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let preview = &preview["preview"];
        let command = json!({"operation_id":Uuid::new_v4(),"fact":{
            "source_event_id":c.x.source,"source_event_created_at":preview["source"]["event_created_at"],
            "destination_channel_id":c.x.f.channel,"kind":kind,"human_public_key":human,
            "expected_audience_hash":preview["audience_hash"],"content":EMPLOYEE_FACT,
            "expires_at":(Utc::now()+chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Micros,true),"reviewed":true}});
        let (status, saved) = post(&app, &c.x.f.operator, PATH, &command).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        let fact = id(&saved["fact"]);
        let (status, exported) = post(
            &app,
            &c.x.f.operator,
            &format!("{PATH}/{fact}/export"),
            &json!({"operation_id":Uuid::new_v4(),"expected_version":1}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{exported}");
        assert!(
            employee_exports::schedule_one(
                &c.x.f.control,
                &c.x.scope,
                &employee_exports::HonchoEmployeeExportAdapter::new(&remote.service)
            )
            .await
            .unwrap()
        );
        assert_eq!(remote.diagnostic_count(), 3);
        Self {
            c,
            remote,
            app,
            target,
            fact,
            until,
        }
    }
    pub fn memory(&self) -> MixedMemory<'_> {
        MixedMemory {
            c: &self.c,
            remote: &self.remote,
            app: &self.app,
            fact: self.fact,
            destination: Some(EmployeeReviewedDestination {
                target_id: self.target,
                destination_channel_id: self.c.x.f.channel,
            }),
            stop_after_read: std::sync::atomic::AtomicBool::new(false),
        }
    }
    pub fn selected(&self) -> Vec<Value> {
        self.remote
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(p, _)| p.ends_with("/recall-selected"))
            .map(|(_, body)| body.clone())
            .collect()
    }
    pub async fn uses(&self, run: Uuid) -> Vec<(i32, Uuid)> {
        sqlx::query_as("SELECT ordinal,fact_id FROM run_employee_reviewed_memory_uses WHERE company_id=$1 AND run_id=$2 ORDER BY ordinal")
            .bind(self.c.x.f.company).bind(run).fetch_all(&self.c.x.f.pool).await.unwrap()
    }
    pub async fn prove_target_unchanged(&self) {
        let actual: DateTime<Utc> = sqlx::query_scalar("SELECT valid_until FROM employee_reviewed_memory_targets WHERE company_id=$1 AND id=$2")
            .bind(self.c.x.f.company).bind(self.target).fetch_one(&self.c.x.f.pool).await.unwrap();
        assert_eq!(actual, self.until);
        assert_eq!(self.remote.diagnostic_count(), 3);
    }
}

pub(super) async fn stop(c: &ConversationFixture, app: &Router, fact: Uuid) {
    let (status, result) = post(
        app,
        &c.x.f.operator,
        &format!("{PATH}/{fact}/stop"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
}
