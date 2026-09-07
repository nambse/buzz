//! Explicit reviewed-project operations, never native resource or legacy-memory deletion.
use super::*;
use serde_json::Value;

mod selected;
mod types;
mod validation;
pub use types::*;
const FAMILY: &str = "reviewed-project/1";

impl HonchoMemoryAdapter {
    async fn reviewed_ready(
        &self,
        scope: &ReviewedProjectScope,
        capability: MemoryCapability,
    ) -> Result<(HonchoEmployeeBinding, IoGate, String), MemoryError> {
        let allowed = self
            .allowed(Some(&scope.employee_id), &scope.binding)?
            .clone();
        if scope.project_id.is_nil() || !allowed.allowed_projects.contains(&scope.project_id) {
            return Err(invalid("reviewed project is outside the selected binding"));
        }
        let gate = self.require_witness(&allowed, capability)?;
        self.protocol().await?;
        self.require_owned(&allowed).await?;
        let identity = self
            .creation_receipts
            .lock()
            .map_err(|_| unavailable("memory identity unavailable"))?
            .get(&scope.employee_id)
            .cloned()
            .ok_or_else(|| rejected("reviewed binding identity missing"))?;
        let hash = wire::fingerprint(
            &json!({"request_hash":identity.request_hash,"native_ids":identity.native_ids}),
        )?;
        self.check_gate(&allowed, gate)?;
        Ok((allowed, gate, hash))
    }

    /// Publish only explicitly approved text into the separate reviewed-project store.
    /// The caller must authorize current human/project rights and retain the operation key.
    pub async fn publish_reviewed_project(
        &self,
        scope: &ReviewedProjectScope,
        publication: &ReviewedProjectPublication,
    ) -> Result<ReviewedProjectReceipt, MemoryError> {
        self.bounded(async {
            let body = validation::publication(self.company_id, scope, publication)?;
            let (allowed, gate, binding_hash) = self
                .reviewed_ready(scope, MemoryCapability::Remember)
                .await?;
            self.check_gate(&allowed, gate)?;
            let (status, response) = self
                .http
                .request(
                    Method::POST,
                    &format!("{}/records/{}/publish", path(scope), publication.record_id),
                    Some(body.clone()),
                )
                .await?;
            validation::receipt(
                response,
                &body,
                "publish",
                self.company_id,
                scope,
                publication.record_id,
                &binding_hash,
                status == reqwest::StatusCode::CREATED,
            )
        })
        .await
    }

    /// Irreversibly withdraw or expire one reviewed record, including before publication.
    /// The proof covers only this extension's text store, not source evidence or backups.
    pub async fn remove_reviewed_project(
        &self,
        scope: &ReviewedProjectScope,
        record_id: Uuid,
        idempotency_key: &str,
        removal: ReviewedProjectRemoval,
    ) -> Result<ReviewedProjectReceipt, MemoryError> {
        self.bounded(async {
            if record_id.is_nil() || !config::key(idempotency_key) { return Err(invalid("invalid reviewed removal identity")); }
            let action = match removal { ReviewedProjectRemoval::Withdraw => "withdraw", ReviewedProjectRemoval::Expire => "expire" };
            let body = json!({"company_id":self.company_id,"employee_id":scope.employee_id,"idempotency_key":idempotency_key});
            let (allowed, gate, binding_hash) = self.reviewed_ready(scope, MemoryCapability::Remember).await?;
            self.check_gate(&allowed, gate)?;
            let (status, response) = self.http.request(Method::POST, &format!("{}/records/{record_id}/{action}",path(scope)), Some(body.clone())).await?;
            if status != reqwest::StatusCode::OK { return Err(rejected("unexpected reviewed erasure status")); }
            validation::receipt(response, &body, action, self.company_id, scope, record_id, &binding_hash, false)
        }).await
    }

    /// Inspect at most 25 records with current expiry/withdrawal and exact provenance.
    /// Inspection performs no writes, cleanup, remote derivation or witness refresh.
    pub async fn inspect_reviewed_project(
        &self,
        scope: &ReviewedProjectScope,
        after: Option<Uuid>,
    ) -> Result<ReviewedProjectPage, MemoryError> {
        self.bounded(async {
            if after.is_some_and(|id| id.is_nil()) {
                return Err(invalid("invalid reviewed cursor"));
            }
            let mut body =
                json!({"company_id":self.company_id,"employee_id":scope.employee_id,"limit":25});
            if let Some(after) = after {
                body["after"] = json!(after);
            }
            let (allowed, gate, binding_hash) =
                self.reviewed_ready(scope, MemoryCapability::Recall).await?;
            self.check_gate(&allowed, gate)?;
            let (status, response) = self
                .http
                .request(
                    Method::POST,
                    &format!("{}/inspect", path(scope)),
                    Some(body),
                )
                .await?;
            if status != reqwest::StatusCode::OK {
                return Err(rejected("unexpected reviewed inspection status"));
            }
            let page: ReviewedProjectPage = serde_json::from_value(response)
                .map_err(|_| rejected("invalid reviewed inspection page"))?;
            if page.records.len() > 25 {
                return Err(rejected("reviewed inspection exceeded page bound"));
            }
            let mut previous = after;
            for record in &page.records {
                validation::record(record, self.company_id, scope, &binding_hash, true)?;
                if previous.is_some_and(|id| record.record_id <= id) {
                    return Err(rejected("reviewed inspection cursor did not advance"));
                }
                previous = Some(record.record_id);
            }
            if page.next_after.is_some()
                && (page.records.len() != 25 || page.next_after != previous)
            {
                return Err(rejected("reviewed inspection cursor is inconsistent"));
            }
            Ok(page)
        })
        .await
    }

    /// Recall at most eight active approved records and 8 KiB, with no provider call.
    /// The caller must revalidate withdrawal/expiry before admitting records to a run.
    pub async fn recall_reviewed_project(
        &self,
        scope: &ReviewedProjectScope,
        query: &str,
    ) -> Result<ReviewedProjectRecall, MemoryError> {
        self.bounded(async {
            if query.trim().is_empty() || query.len() > 1024 || query.contains('\0')
                || ortak_control::run_event::RedactionPolicy::new().redact(query) != query {
                return Err(invalid("invalid reviewed recall query"));
            }
            let (allowed, gate, binding_hash) = self.reviewed_ready(scope, MemoryCapability::Recall).await?;
            self.check_gate(&allowed, gate)?;
            let (status, response) = self.http.request(Method::POST, &format!("{}/recall",path(scope)),
                Some(json!({"company_id":self.company_id,"employee_id":scope.employee_id,"query":query}))).await?;
            if status != reqwest::StatusCode::OK { return Err(rejected("unexpected reviewed recall status")); }
            let result: ReviewedProjectRecall = serde_json::from_value(response).map_err(|_| rejected("invalid reviewed recall"))?;
            if result.records.len() > 8 { return Err(rejected("reviewed recall exceeded record bound")); }
            let (mut ids, mut total) = (BTreeSet::new(), 0);
            for record in &result.records {
                validation::record(record, self.company_id, scope, &binding_hash, true)?;
                total += record.content.as_ref().map_or(0, String::len);
                if record.status != ReviewedProjectStatus::Active || !ids.insert(record.record_id) || total > 8192 {
                    return Err(rejected("reviewed recall exceeded scope or content budget"));
                }
            }
            Ok(result)
        }).await
    }
}

fn path(scope: &ReviewedProjectScope) -> String {
    format!(
        "/v3/ortak/workspaces/{}/reviewed-projects/{}",
        scope.binding.workspace, scope.project_id
    )
}
