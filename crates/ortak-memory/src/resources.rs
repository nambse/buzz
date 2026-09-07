use ortak_control::memory::MemoryResourceRequest;
use ortak_control::{adapter::ResourceOutcome, memory::MemoryResourceOutcome};
use ortak_domain::{EmployeeId, ProvisioningMode};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    config, rejected, unavailable, HonchoEmployeeBinding, HonchoMemoryAdapter, MemoryError,
    PROTOCOL,
};

#[derive(Deserialize)]
struct Page {
    items: Vec<Value>,
    total: usize,
    page: usize,
    size: usize,
    pages: usize,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ResourceIdentity {
    pub request_hash: String,
    pub native_ids: crate::HonchoNativeResourceIds,
}

pub(crate) struct Resources {
    pub workspace: bool,
    pub user: bool,
    pub employee: bool,
    pub owned: bool,
}

impl HonchoMemoryAdapter {
    /// Restores only an existing frozen create receipt after an adapter restart.
    ///
    /// The caller supplies the durably retained original create key and binding.
    /// This method calls protocol/ownership inspection only: absent, replaced or
    /// mismatched resources fail without issuing a create request or memory I/O.
    pub async fn resume_created_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            if request.mode != ProvisioningMode::Create
                || allowed.mode != ProvisioningMode::Create
                || !config::key(&request.idempotency_key)
            {
                return Err(crate::invalid("invalid memory receipt resume request"));
            }
            self.protocol().await?;
            let body = self.creation_body(allowed, &request.idempotency_key);
            self.retain_creation_identity(allowed, &body).await?;
            Ok(outcome(allowed, true))
        })
        .await
    }

    pub(crate) fn creation_body(&self, allowed: &HonchoEmployeeBinding, key: &str) -> Value {
        let b = &allowed.binding;
        json!({"idempotency_key":key,"company_id":self.company_id,
            "employee_id":allowed.employee_id,"workspace_id":b.workspace,
            "user_peer":b.user_peer,"employee_peer":b.employee_peer})
    }

    pub(crate) async fn retain_creation_identity(
        &self,
        allowed: &HonchoEmployeeBinding,
        body: &Value,
    ) -> Result<(), MemoryError> {
        let identity = self.inspect_owned_identity(allowed).await?;
        if identity.request_hash != crate::wire::fingerprint(body)? {
            return Err(rejected(
                "memory ownership receipt differs from create request",
            ));
        }
        self.retain_verified_identity(allowed, identity)
    }

    pub(crate) fn retain_verified_identity(
        &self,
        allowed: &HonchoEmployeeBinding,
        identity: ResourceIdentity,
    ) -> Result<(), MemoryError> {
        let mut receipts = self
            .creation_receipts
            .lock()
            .map_err(|_| unavailable("memory creation receipt state unavailable"))?;
        if receipts
            .get(&allowed.employee_id)
            .is_some_and(|previous| previous != &identity)
        {
            return Err(rejected("memory native resource identity changed"));
        }
        receipts.insert(allowed.employee_id.clone(), identity);
        Ok(())
    }

    pub(crate) async fn inspect_owned_identity(
        &self,
        allowed: &HonchoEmployeeBinding,
    ) -> Result<ResourceIdentity, MemoryError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Inspection {
            protocol: String,
            company_id: Uuid,
            employee_id: EmployeeId,
            workspace_id: String,
            user_peer: String,
            employee_peer: String,
            ownership: String,
            request_hash: String,
            native_ids: crate::HonchoNativeResourceIds,
        }
        let b = &allowed.binding;
        let (_, value) = self
            .http
            .request(
                Method::POST,
                &format!("/v3/ortak/workspaces/{}/resources/inspect", b.workspace),
                Some(
                    json!({"company_id": self.company_id, "employee_id": allowed.employee_id,
                "user_peer": b.user_peer, "employee_peer": b.employee_peer}),
                ),
            )
            .await?;
        let result: Inspection = serde_json::from_value(value)
            .map_err(|_| rejected("invalid memory ownership inspection"))?;
        if result.protocol != PROTOCOL
            || result.company_id != self.company_id
            || result.employee_id != allowed.employee_id
            || result.workspace_id != b.workspace
            || result.user_peer != b.user_peer
            || result.employee_peer != b.employee_peer
            || result.ownership != "created"
            || result.request_hash.len() != 64
            || !result
                .request_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !result.native_ids.matches_binding(b)
        {
            return Err(rejected("memory ownership inspection differs from binding"));
        }
        Ok(ResourceIdentity {
            request_hash: result.request_hash,
            native_ids: result.native_ids,
        })
    }

    async fn listed(&self, path: &str, wanted: &[&str]) -> Result<Vec<Value>, MemoryError> {
        let mut found = Vec::new();
        let mut expected_total = None;
        for page in 1..=10 {
            let (_, value) = self
                .http
                .request(
                    Method::POST,
                    &format!("{path}?page={page}&size=100"),
                    Some(json!({})),
                )
                .await?;
            let result: Page = serde_json::from_value(value)
                .map_err(|_| rejected("invalid memory resource list"))?;
            if result.page != page
                || result.size != 100
                || result.items.len() > 100
                || result.pages != result.total.div_ceil(100)
                || expected_total.is_some_and(|n| n != result.total)
            {
                return Err(rejected("inconsistent memory resource pagination"));
            }
            expected_total = Some(result.total);
            for item in result.items {
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| rejected("invalid memory resource identity"))?;
                if wanted.contains(&id) {
                    if found.iter().any(|old: &Value| old["id"] == item["id"]) {
                        return Err(rejected("duplicate memory resource identity"));
                    }
                    found.push(item);
                }
            }
            if found.len() == wanted.len() || page >= result.pages {
                return Ok(found);
            }
        }
        Err(unavailable(
            "memory resource list exceeds bounded inspection window",
        ))
    }

    pub(crate) async fn inspect_resources(
        &self,
        allowed: &HonchoEmployeeBinding,
    ) -> Result<Resources, MemoryError> {
        let binding = &allowed.binding;
        let workspace = self
            .listed("/v3/workspaces/list", &[&binding.workspace])
            .await?;
        if workspace.is_empty() {
            return Ok(Resources {
                workspace: false,
                user: false,
                employee: false,
                owned: false,
            });
        }
        let peers = self
            .listed(
                &format!("/v3/workspaces/{}/peers/list", binding.workspace),
                &[&binding.user_peer, &binding.employee_peer],
            )
            .await?;
        if peers
            .iter()
            .any(|peer| peer["workspace_id"] != binding.workspace)
        {
            return Err(rejected("memory peer belongs to another workspace"));
        }
        let expected = self
            .creation_receipts
            .lock()
            .map_err(|_| unavailable("memory creation receipt state unavailable"))?
            .get(&allowed.employee_id)
            .cloned();
        let owned = if peers.len() == 2 {
            if let Some(expected) = expected {
                if self.inspect_owned_identity(allowed).await? != expected {
                    return Err(rejected("memory native resource identity changed"));
                }
                true
            } else {
                false
            }
        } else {
            false
        };
        Ok(Resources {
            workspace: true,
            user: peers.iter().any(|p| p["id"] == binding.user_peer),
            employee: peers.iter().any(|p| p["id"] == binding.employee_peer),
            owned,
        })
    }
}

pub(crate) fn outcome(allowed: &HonchoEmployeeBinding, created: bool) -> MemoryResourceOutcome {
    let binding = &allowed.binding;
    let resource = |value| {
        if created {
            ResourceOutcome::created(value)
        } else {
            ResourceOutcome::adopted(value)
        }
    };
    MemoryResourceOutcome {
        workspace: resource(format!("workspace:{}", binding.workspace)),
        user_peer: resource(format!("peer:{}/{}", binding.workspace, binding.user_peer)),
        employee_peer: resource(format!(
            "peer:{}/{}",
            binding.workspace, binding.employee_peer
        )),
    }
}

pub(crate) fn validate_create(
    allowed: &HonchoEmployeeBinding,
    value: Value,
) -> Result<MemoryResourceOutcome, MemoryError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Created {
        protocol: String,
        workspace_id: String,
        user_peer: String,
        employee_peer: String,
        ownership: String,
    }
    let result: Created =
        serde_json::from_value(value).map_err(|_| rejected("invalid memory resource receipt"))?;
    let b = &allowed.binding;
    if result.protocol != PROTOCOL
        || result.workspace_id != b.workspace
        || result.user_peer != b.user_peer
        || result.employee_peer != b.employee_peer
        || result.ownership != "created"
    {
        return Err(rejected(
            "memory resource receipt differs from authorized binding",
        ));
    }
    Ok(outcome(allowed, true))
}
