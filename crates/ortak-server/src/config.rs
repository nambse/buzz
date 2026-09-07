use std::collections::BTreeSet;

use nostr::PublicKey;
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// Server-owned access configuration. Contains public identifiers, never keys.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    /// Canonical origin used for Host and NIP-98 URL verification.
    pub origin: String,
    /// The Office community whose company binding is resolved on every request.
    pub community_id: Uuid,
    /// Explicit human audience/role grants for this isolated deployment.
    pub humans: Vec<HumanGrant>,
    /// Exact browser/Tauri origins allowed for signed cross-origin requests.
    /// Empty disables cross-origin browser access; cookies are never allowed.
    #[serde(default)]
    pub allowed_web_origins: Vec<String>,
}

/// A human principal authorized out of band by the deployment operator.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanGrant {
    /// Canonical lowercase hex public key. No private key is accepted here.
    pub public_key: String,
    /// Permission to read, or to read and request cancellation.
    pub role: Role,
    /// May create explicitly channel-bound projects; existing project roles
    /// remain mandatory for every read and mutation. Defaults to disabled.
    #[serde(default)]
    pub can_create_projects: bool,
    /// May inspect provisioning progress for granted employees. This does not
    /// permit executing an operation; defaults to disabled and requires operator.
    #[serde(default)]
    pub can_manage_employees: bool,
    /// May admit prepared-resource commands. Separate from the F1 read grant;
    /// defaults to disabled and requires both operator and management access.
    #[serde(default)]
    pub can_execute_provisioning: bool,
    /// May explicitly review employee-owned memory from their own Office source.
    /// Independent of Operator; no publication or runtime permission is implied.
    #[serde(default)]
    pub can_review_employee_memory: bool,
    /// Allowed Office channels; live private-channel membership is also required.
    pub channel_ids: Vec<Uuid>,
    /// Explicit employee directory and run audience for this private MVP.
    pub employee_ids: Vec<EmployeeId>,
}

/// Product API privileges, derived only from server configuration.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read the explicitly granted employee/channel audience.
    Reader,
    /// Also request cancellation of a visible run.
    Operator,
}

impl ApiConfig {
    /// Validates the bounded configuration and canonicalizes its origin.
    /// HTTP is accepted only for loopback origins; a proxy must preserve Host.
    pub fn validate(mut self) -> Result<Self, &'static str> {
        let url = Url::parse(&self.origin).map_err(|_| "invalid API origin")?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host == "[::1]"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || self.community_id.is_nil()
            || self.humans.is_empty()
            || self.humans.len() > 32
        {
            return Err("invalid API origin or audience configuration");
        }
        if self.allowed_web_origins.len() > 8 {
            return Err("too many web origins");
        }
        for origin in &self.allowed_web_origins {
            if matches!(
                origin.as_str(),
                "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
            ) {
                continue;
            }
            let parsed = Url::parse(origin).map_err(|_| "invalid web origin")?;
            let local = parsed.host_str().is_some_and(|host| {
                host == "localhost"
                    || host == "[::1]"
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            });
            if !(parsed.scheme() == "https" || parsed.scheme() == "http" && local)
                || parsed.origin().ascii_serialization() != *origin
            {
                return Err("invalid web origin");
            }
        }
        let mut principals = BTreeSet::new();
        for grant in &self.humans {
            let key =
                PublicKey::from_hex(&grant.public_key).map_err(|_| "invalid human public key")?;
            if key.to_hex() != grant.public_key
                || !principals.insert(grant.public_key.clone())
                || (grant.can_manage_employees && grant.role != Role::Operator)
                || (grant.can_execute_provisioning && !grant.can_manage_employees)
                || grant.channel_ids.is_empty()
                || grant.channel_ids.len() > 64
                || grant.channel_ids.iter().any(Uuid::is_nil)
                || grant.employee_ids.is_empty()
                || grant.employee_ids.len() > 64
            {
                return Err("invalid human audience configuration");
            }
        }
        self.origin = url.origin().ascii_serialization();
        Ok(self)
    }

    pub(crate) fn authority(&self) -> &str {
        self.origin.split_once("://").map_or("", |(_, host)| host)
    }
}
