use super::{invalid, unavailable, HermesAdapter};
use ortak_control::runtime::{RunSpec, RunStartReceipt, RuntimeError};
use ortak_control::workspace::{
    empty_policy, workspace_read_policy, WorkspaceGrant, WorkspaceResult, WorkspaceToolAck,
    WorkspaceToolPort, WorkspaceToolRequest,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    request: Option<WorkspaceToolRequest>,
}

impl HermesAdapter {
    pub(super) async fn start_selected(
        &self,
        spec: &RunSpec,
        workspace: Option<&WorkspaceGrant>,
    ) -> Result<RunStartReceipt, RuntimeError> {
        spec.validate()?;
        let run_id = self.run_id(&spec.idempotency_key)?;
        if run_id != spec.run_id || spec.binding.adapter != "hermes" {
            return Err(invalid());
        }
        let mut body = json!({"company_id": self.company_id, "spec": spec});
        match workspace {
            None if empty_policy(&spec.permissions) => (),
            Some(grant) => {
                grant.validate()?;
                if grant.company_id != self.company_id
                    || grant.employee_id != spec.employee_id
                    || spec.context.work_item_id.is_none()
                    || spec.context.conversation_ref.is_some()
                    || spec.context.reply_to_message_id.is_some()
                    || spec.binding.workspace_ref != grant.workspace_ref
                    || !workspace_read_policy(&spec.permissions, &grant.workspace_ref)
                {
                    return Err(invalid());
                }
                body["workspace"] = serde_json::to_value(grant).map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
        let receipt = self
            .request(Method::POST, "/v1/runs", Some(body))
            .await?
            .ok_or_else(unavailable)?;
        self.receipt(run_id, receipt)
    }
}

impl WorkspaceToolPort for HermesAdapter {
    async fn pending_workspace_tool(
        &self,
        key: &str,
        grant: &WorkspaceGrant,
    ) -> Result<Option<WorkspaceToolRequest>, RuntimeError> {
        grant.validate()?;
        if grant.company_id != self.company_id {
            return Err(invalid());
        }
        let run_id = self.run_id(key)?;
        let pending: Pending = self
            .request(
                Method::POST,
                "/v1/runs/tools/pending",
                Some(json!({"company_id":self.company_id,"run_id":run_id,"idempotency_key":key})),
            )
            .await?
            .ok_or_else(unavailable)?;
        if let Some(request) = &pending.request {
            request.validate(grant)?;
        }
        Ok(pending.request)
    }

    async fn resolve_workspace_tool(
        &self,
        key: &str,
        grant: &WorkspaceGrant,
        request: &WorkspaceToolRequest,
        result: &WorkspaceResult,
    ) -> Result<WorkspaceToolAck, RuntimeError> {
        if grant.company_id != self.company_id {
            return Err(invalid());
        }
        result.validate(grant, request)?;
        let run_id = self.run_id(key)?;
        let ack: WorkspaceToolAck = self
            .request(
                Method::POST,
                "/v1/runs/tools/resolve",
                Some(
                    json!({"company_id":self.company_id,"run_id":run_id,"idempotency_key":key,
                "request":request,"result":result}),
                ),
            )
            .await?
            .ok_or_else(unavailable)?;
        if !ack.acknowledged
            || ack.call_id != request.call_id
            || ack.arguments_hash != request.arguments_hash
        {
            return Err(invalid());
        }
        Ok(ack)
    }
}
