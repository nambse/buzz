use super::*;

pub(super) struct ExportFixture {
    pub f: Fixture,
    pub app: Router,
    pub scope: CompanyScope,
    pub project: Uuid,
    pub employee: Employee,
    pub target: ReviewedMemoryTarget,
    pub fact: Uuid,
    pub source: String,
}
impl ExportFixture {
    pub async fn new(expiry: Duration, advertise: bool) -> Self {
        Self::configured(expiry, advertise, None, false).await
    }
    pub async fn with_signer(expiry: Duration, public_key: &str, signer_ref: &str) -> Self {
        Self::configured(expiry, false, Some((public_key, signer_ref)), false).await
    }
    pub async fn with_owned_signer(expiry: Duration, public_key: &str, signer_ref: &str) -> Self {
        Self::configured(expiry, false, Some((public_key, signer_ref)), true).await
    }
    async fn configured(
        expiry: Duration,
        advertise: bool,
        signer: Option<(&str, &str)>,
        owned: bool,
    ) -> Self {
        let f = Fixture::new().await;
        let employee = if let Some((public_key, signer_ref)) = signer {
            if owned {
                super::super::execution::fixture::employee_with_owned_memory_and_signer(
                    &f, public_key, signer_ref,
                )
                .await
            } else {
                super::super::execution::fixture::employee_with_memory_and_signer(
                    &f, "honcho", public_key, signer_ref,
                )
                .await
            }
        } else {
            super::super::execution::fixture::employee_with_memory_adapter(&f, "honcho").await
        };
        let app = work_app(&f, true, Role::Operator, vec![f.channel]);
        let project = project(&f, &app, f.channel).await;
        let source = boundaries::source_message(&f, f.channel).await;
        let body = json!({"operation_id":Uuid::new_v4(),"fact":{"employee_id":"cem","source":{"kind":"conversation","message_id":source},
            "content":"Reviewed deployment fact","expires_at":Utc::now()+chrono::Duration::from_std(expiry).unwrap(),"reviewed":true}});
        let saved = post(
            &app,
            &f.operator,
            &format!("/api/v1/projects/{project}/reviewed-memory"),
            &body,
        )
        .await;
        assert_eq!(saved.0, StatusCode::OK, "{saved:?}");
        let fact = id(&saved.1["fact"]);
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        let binding = employee.memory.clone().unwrap();
        let deployment = Uuid::new_v4();
        let receipt = json!({"company_id":f.company,"deployment_id":deployment,"employee_id":employee.id,"binding":binding,
            "creation_key":"synthetic-owned-create","request_hash":"ab".repeat(32),"native_ids":{"workspace":"synthetic-native","peers":{
                binding.user_peer.clone():"synthetic-human",binding.employee_peer.clone():"synthetic-employee"}},
            "resources":{"workspace":{"resource_ref":format!("workspace:{}",binding.workspace),"ownership":"created"},
                "user_peer":{"resource_ref":format!("peer:{}/{}",binding.workspace,binding.user_peer),"ownership":"created"},
                "employee_peer":{"resource_ref":format!("peer:{}/{}",binding.workspace,binding.employee_peer),"ownership":"created"}}});
        let target = ReviewedMemoryTarget {
            runtime_consumption_enabled: false,
            project_id: project,
            employee_id: employee.id.clone(),
            deployment_id: deployment,
            binding,
            creation_receipt: receipt,
            valid_for: Duration::from_secs(55),
        };
        let result = Self {
            f,
            app,
            scope,
            project,
            employee,
            target,
            fact,
            source,
        };
        if advertise {
            result.advertise().await;
        }
        result
    }
    pub async fn advertise(&self) {
        assert_eq!(
            exports::advertise_targets(
                &self.f.control,
                &self.scope,
                std::slice::from_ref(&self.target)
            )
            .await
            .unwrap(),
            1
        );
    }
    pub fn path(&self) -> String {
        format!(
            "/api/v1/projects/{}/reviewed-memory/{}",
            self.project, self.fact
        )
    }
    pub fn publish_path(&self) -> String {
        format!("{}/publish", self.path())
    }
    pub fn command(&self) -> Value {
        json!({"operation_id":Uuid::new_v4(),"expected_version":1,"confirmed":true})
    }
    pub async fn publish(&self) {
        let result = post(
            &self.app,
            &self.f.operator,
            &self.publish_path(),
            &self.command(),
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
    }
    pub async fn stop(&self) {
        let result=post(&self.app,&self.f.operator,&format!("{}/stop",self.path()),&json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Human selected Stop using"})).await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
    }
    pub async fn counts(&self) -> (i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM reviewed_memory_exports WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_export_jobs WHERE company_id=$1),(SELECT count(*) FROM reviewed_memory_export_commands WHERE company_id=$1)")
        .bind(self.f.company).fetch_one(&self.f.pool).await.unwrap()
    }
    pub async fn page(&self) -> Value {
        let result = get(
            &self.app,
            &self.f.operator,
            &format!(
                "/api/v1/projects/{}/reviewed-memory?employee_id=cem",
                self.project
            ),
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
        result.1
    }
}

/// Controlled boundary after production PG prepare; D2a's real HTTP/PG suites
/// separately prove the remote operation/tombstone semantics.
#[derive(Default)]
pub(super) struct ObservedAdapter {
    pub calls: Mutex<Vec<(ReviewedExportAction, String, Vec<u8>)>>,
    pub unavailable: bool,
    pub published: Mutex<std::collections::HashMap<Uuid, Vec<u8>>>,
}
pub(super) fn acknowledgement(request: &PreparedReviewedExport) -> ReviewedExportAcknowledgement {
    let removal = request.lease.action == ReviewedExportAction::Withdraw;
    ReviewedExportAcknowledgement{request_hash:request.request_hash.clone(),
        binding_hash:Sha256::digest(serde_json::to_vec(&json!({"request_hash":request.creation_receipt["request_hash"],"native_ids":request.creation_receipt["native_ids"]})).unwrap()).to_vec(),
        content_hash:request.content.as_ref().map(|v|Sha256::digest(v.as_bytes()).to_vec()),remote_status:if removal{"withdrawn"}else{"active"}.into(),
        erased_from_reviewed_store:removal,tombstone_at:removal.then(Utc::now)}
}
impl ReviewedExportAdapter for ObservedAdapter {
    async fn write(
        &self,
        request: &PreparedReviewedExport,
    ) -> Result<ReviewedExportAcknowledgement, MemoryError> {
        self.calls.lock().unwrap().push((
            request.lease.action,
            request.idempotency_key.clone(),
            request.request_hash.clone(),
        ));
        if self.unavailable {
            Err(MemoryError::Unavailable {
                detail: Detail::new("synthetic unavailable"),
            })
        } else {
            let mut receipt = acknowledgement(request);
            let mut published = self.published.lock().unwrap();
            if let Some(content_hash) = &receipt.content_hash {
                published.insert(request.lease.fact_id, content_hash.clone());
            } else {
                receipt.content_hash = published.get(&request.lease.fact_id).cloned();
            }
            Ok(receipt)
        }
    }
}
