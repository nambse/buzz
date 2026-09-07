use super::super::authority::NamedMemory;
use super::adapter::SelectedMemory;
use super::*;
use ortak_control::ports::{InboxRepository, SemanticScorer};
use ortak_control::{InboxRoutingService, RoutingWorkerConfig};

pub(super) const FACT: &str = "Deployment conversation fact uniquely approved for this thread.";
pub(super) const SCRATCH: &str = "Private scratch before conversation selection.";
pub(super) const ANSWER: &str = "Deployment answer incorporating the selected conversation fact.";
const SIGNER: &str = "credential://fixture/conversation-office-signer";

pub(super) struct ConversationFixture {
    pub x: ExportFixture,
    pub fact: Uuid,
    pub signer: FakeOfficeSigner,
    pub runtime: FakeRuntimeAdapter,
    pub memory: SelectedMemory,
    remote: ObservedAdapter,
}
impl ConversationFixture {
    pub async fn new() -> Self {
        Self::configured(false).await
    }
    pub async fn new_owned() -> Self {
        Self::configured(true).await
    }
    async fn configured(owned: bool) -> Self {
        let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER);
        let public = signer.public_key(SIGNER).unwrap().to_hex();
        let x = if owned {
            ExportFixture::with_owned_signer(Duration::from_secs(86400), &public, SIGNER).await
        } else {
            ExportFixture::with_signer(Duration::from_secs(86400), &public, SIGNER).await
        };
        super::super::conversation_publication::advertise(&x).await;
        let memory = SelectedMemory {
            inner: NamedMemory(
                FakeMemoryAdapter::new().with_existing_binding(x.employee.memory.as_ref().unwrap()),
            ),
            project: x.project,
            project_enabled: false,
            contents: Mutex::new(Default::default()),
            selected: Mutex::new(vec![]),
        };
        let mut c = Self {
            x,
            fact: Uuid::nil(),
            signer,
            memory,
            runtime: FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true),
            remote: ObservedAdapter::default(),
        };
        c.fact = c.approve_publish("thread", &c.x.source, FACT).await;
        c
    }
    pub async fn approve_publish(&self, kind: &str, source: &str, content: &str) -> Uuid {
        let fact =
            super::super::conversation_publication::approve(&self.x, kind, source, content).await;
        let result = post(
            &self.x.app,
            &self.x.f.operator,
            &format!(
                "/api/v1/projects/{}/conversation-memory/{fact}/publish",
                self.x.project
            ),
            &self.x.command(),
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
        assert!(schedule_one(&self.x.f.control, &self.x.scope, &self.remote)
            .await
            .unwrap());
        self.memory
            .contents
            .lock()
            .unwrap()
            .insert(fact, content.into());
        fact
    }
    pub async fn opt_out(&self) {
        exports::advertise_targets_with_conversations(
            &self.x.f.control,
            &self.x.scope,
            std::slice::from_ref(&self.x.target),
            &[],
        )
        .await
        .unwrap();
    }
    pub async fn start_office(&self) -> (Uuid, RuntimeRunRef) {
        self.start_office_with(&self.memory).await
    }
    pub async fn start_office_with(
        &self,
        memory: &(impl MemoryAdapter + ReviewedRunAdapter),
    ) -> (Uuid, RuntimeRunRef) {
        let event = EventBuilder::new(Kind::Custom(9), "Deployment conversation reply requested")
            .tags([
                Tag::parse(["h", &self.x.f.channel.to_string()]).unwrap(),
                Tag::parse(["p", &self.x.employee.office.public_key]).unwrap(),
                Tag::parse(["e", &self.x.source, "", "reply"]).unwrap(),
                Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
            ])
            .sign_with_keys(&self.x.f.operator)
            .unwrap();
        let at = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
        let source_at: chrono::DateTime<Utc> = sqlx::query_scalar(
            "SELECT event_created_at FROM office_inbox WHERE company_id=$1 AND event_id=$2",
        )
        .bind(self.x.f.company)
        .bind(hex::decode(&self.x.source).unwrap())
        .fetch_one(&self.x.f.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,$6,$7,$8)")
            .bind(self.x.f.community).bind(event.id.to_bytes().as_slice()).bind(event.pubkey.to_bytes().as_slice()).bind(at)
            .bind(serde_json::to_value(&event.tags).unwrap()).bind(&event.content).bind(event.sig.serialize().as_slice())
            .bind(self.x.f.channel).execute(&self.x.f.pool).await.unwrap();
        sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,$5,$6,$5,$6,1)")
            .bind(self.x.f.community).bind(event.id.to_bytes().as_slice()).bind(at).bind(self.x.f.channel)
            .bind(hex::decode(&self.x.source).unwrap()).bind(source_at).execute(&self.x.f.pool).await.unwrap();
        self.x
            .f
            .control
            .insert_accepted_event(
                self.x.f.community,
                &ortak_control::inbox::InboxEvent {
                    event_id: ortak_control::MessageId::parse_hex(&event.id.to_hex()).unwrap(),
                    event_kind: 9,
                    event_created_at: at,
                    author_pubkey: event.pubkey.to_bytes(),
                    channel_id: Some(self.x.f.channel),
                },
            )
            .await
            .unwrap();
        let service = InboxRoutingService::new(
            self.x.f.control.clone(),
            ortak_office::PgChannelNormalizer::new(self.x.f.pool.clone()),
            NeverScore,
            RoutingWorkerConfig::default(),
        );
        let outcome = service.claim_and_route(&self.x.scope).await.unwrap();
        assert!(
            matches!(outcome, Some(ortak_control::ServiceOutcome::Committed(_))),
            "{outcome:?}"
        );
        self.dispatch_next_with(memory).await
    }
    pub async fn dispatch(&self, run: Uuid) -> RuntimeRunRef {
        self.dispatch_with(run, &self.memory).await
    }
    pub async fn dispatch_with(
        &self,
        run: Uuid,
        memory: &(impl MemoryAdapter + ReviewedRunAdapter),
    ) -> RuntimeRunRef {
        let (run_id, reference) = self.dispatch_next_with(memory).await;
        assert_eq!(run_id, run);
        reference
    }
    async fn dispatch_next_with(
        &self,
        memory: &(impl MemoryAdapter + ReviewedRunAdapter),
    ) -> (Uuid, RuntimeRunRef) {
        let leases = self
            .x
            .f
            .control
            .claim_runtime_dispatches(
                &self.x.scope,
                "fake-runtime",
                "conversation-v4",
                Duration::from_secs(60),
                8,
            )
            .await
            .unwrap();
        assert_eq!(leases.len(), 1);
        let supervisor = RunSupervisor::new(
            self.x.f.control.clone(),
            &self.runtime,
            SupervisorConfig::default(),
        )
        .with_run_memory(ReviewedRunMemory::new(
            memory,
            self.x.f.control.clone(),
            self.x.scope.clone(),
        ));
        let outcome = supervisor
            .dispatch(&self.x.scope, &leases[0])
            .await
            .unwrap();
        let DispatchOutcome::Started {
            run_id,
            runtime_run_ref,
            ..
        } = outcome
        else {
            panic!("{outcome:?}")
        };
        (run_id, runtime_run_ref)
    }
    pub async fn complete(&self, run: Uuid, reference: &RuntimeRunRef) {
        let is_work: bool = sqlx::query_scalar(
            "SELECT work_item_id IS NOT NULL FROM runs WHERE company_id=$1 AND id=$2",
        )
        .bind(self.x.f.company)
        .bind(run)
        .fetch_one(&self.x.f.pool)
        .await
        .unwrap();
        let intent = if is_work {
            DeliveryIntentKind::Silent
        } else {
            DeliveryIntentKind::Reply
        };
        self.runtime.push_event(
            reference,
            RunEventPayload::AssistantDelta {
                turn: 0,
                delta: BoundedText::raw(ANSWER),
            },
        );
        self.runtime.push_event(
            reference,
            RunEventPayload::DeliveryIntent {
                intent,
                target_ref: None,
            },
        );
        self.runtime.push_event(
            reference,
            RunEventPayload::RunCompleted {
                delivery_intent: intent,
            },
        );
        RunSupervisor::new(
            self.x.f.control.clone(),
            &self.runtime,
            SupervisorConfig::default(),
        )
        .drain(&self.x.scope, run)
        .await
        .unwrap();
    }
    pub async fn ready_work(&self) -> Value {
        let mut body = item_body("Deployment conversation result");
        body["source_message_id"] = json!(self.x.source);
        let created = post(
            &self.x.app,
            &self.x.f.operator,
            &format!("/api/v1/projects/{}/promotions", self.x.project),
            &body,
        )
        .await;
        assert_eq!(created.0, StatusCode::CREATED, "{created:?}");
        let item = &created.1["work_item"];
        let assigned = post(&self.x.app, &self.x.f.operator, &format!("/api/v1/work-items/{}/assignments", id(item)),
            &json!({"operation_id":Uuid::new_v4(),"expected_version":version(item),"employee_id":"cem","role":"owner"})).await;
        assert_eq!(assigned.0, StatusCode::OK, "{assigned:?}");
        transition(
            &self.x.f,
            &self.x.app,
            assigned.1["work_item"].clone(),
            "ready",
        )
        .await
    }
    pub async fn edit_fact_source(&self, edited: bool) {
        sqlx::query("UPDATE events SET content=$3 WHERE community_id=$1 AND id=$2")
            .bind(self.x.f.community)
            .bind(hex::decode(&self.x.source).unwrap())
            .bind(if edited {
                "Temporarily changed source"
            } else {
                "Canonical source fixture"
            })
            .execute(&self.x.f.pool)
            .await
            .unwrap();
    }
    pub async fn bytes(&self, run: Uuid) -> Vec<u8> {
        sqlx::query_scalar(
            "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
        )
        .bind(self.x.f.company)
        .bind(run)
        .fetch_one(&self.x.f.pool)
        .await
        .unwrap()
    }
    pub async fn wire(&self, run: Uuid) -> Value {
        serde_json::from_slice(&self.bytes(run).await).unwrap()
    }
    pub async fn current(&self, run: Uuid) -> bool {
        sqlx::query_scalar("SELECT ortak_run_reviewed_memory_current($1,$2)")
            .bind(self.x.f.company)
            .bind(run)
            .fetch_one(&self.x.f.pool)
            .await
            .unwrap()
    }
    pub async fn read(&self, run: Uuid) -> (StatusCode, Value) {
        get(
            &self.x.app,
            &self.x.f.operator,
            &format!("/api/v1/runs/{run}"),
        )
        .await
    }
}

struct NeverScore;
impl SemanticScorer for NeverScore {
    fn metadata(&self) -> ortak_control::routing::ScorerMetadata {
        ortak_control::routing::ScorerMetadata {
            adapter: "unused".into(),
            model: None,
            prompt_version: None,
            version: "fixture".into(),
            latency_ms: None,
            usage: None,
        }
    }
    async fn score(
        &self,
        _: &ortak_control::SemanticScoringInput,
        _: ortak_control::semantic::ScoringBudget,
    ) -> ortak_control::ports::ScoringOutcome {
        panic!("explicit human mention must use central deterministic routing")
    }
}
