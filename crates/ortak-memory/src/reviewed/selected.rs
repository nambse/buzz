//! Current caller allowlist is applied by the owned store before its result limit.
use super::*;

impl HonchoMemoryAdapter {
    /// Recall only an explicit set of at most 32 current authorized fact IDs.
    /// Results remain bounded to eight records/8 KiB and are checked against the
    /// requested IDs. The caller must revalidate its authority before use.
    pub async fn recall_selected_reviewed_project(
        &self,
        scope: &ReviewedProjectScope,
        query: &str,
        record_ids: &BTreeSet<Uuid>,
    ) -> Result<ReviewedProjectRecall, MemoryError> {
        self.bounded(async {
            if record_ids.is_empty()
                || record_ids.len() > 32
                || record_ids.iter().any(Uuid::is_nil)
                || query.trim().is_empty()
                || query.len() > 1024
                || query.contains('\0')
                || ortak_control::run_event::RedactionPolicy::new().redact(query) != query
            {
                return Err(invalid("invalid reviewed recall selection"));
            }
            let (allowed, gate, binding_hash) =
                self.reviewed_ready(scope, MemoryCapability::Recall).await?;
            self.check_gate(&allowed, gate)?;
            let (status, response) = self
                .http
                .request(
                    Method::POST,
                    &format!("{}/recall-selected", path(scope)),
                    Some(
                        json!({"company_id":self.company_id,"employee_id":scope.employee_id,
                        "query":query,"record_ids":record_ids}),
                    ),
                )
                .await?;
            if status != reqwest::StatusCode::OK {
                return Err(rejected("unexpected selected reviewed recall status"));
            }
            let result: ReviewedProjectRecall = serde_json::from_value(response)
                .map_err(|_| rejected("invalid selected reviewed recall"))?;
            if result.records.len() > 8 {
                return Err(rejected("selected reviewed recall exceeded record bound"));
            }
            let (mut ids, mut total) = (BTreeSet::new(), 0);
            for record in &result.records {
                validation::record(record, self.company_id, scope, &binding_hash, true)?;
                total += record.content.as_ref().map_or(0, String::len);
                if !record_ids.contains(&record.record_id)
                    || record.status != ReviewedProjectStatus::Active
                    || !ids.insert(record.record_id)
                    || total > 8192
                {
                    return Err(rejected(
                        "selected reviewed recall exceeded authorized scope",
                    ));
                }
            }
            self.check_gate(&allowed, gate)?;
            Ok(result)
        })
        .await
    }
}
