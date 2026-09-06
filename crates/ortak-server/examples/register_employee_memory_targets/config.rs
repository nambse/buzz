use chrono::{DateTime, Utc};
use ortak_domain::CredentialRef;
use ortak_memory::{EmployeeNamespaceDiagnostic, HonchoCreatedResourcesReceipt};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, os::unix::fs::MetadataExt, path::Path};
use uuid::Uuid;

use super::Result;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format: String,
    pub company_id: Uuid,
    pub community_id: Uuid,
    pub database_env: String,
    pub database_port: u16,
    pub deployment: Deployment,
    pub targets: Vec<Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub deployment_id: Uuid,
    pub endpoint_ref: String,
    pub origin: String,
    pub token_ref: CredentialRef,
    pub token_env: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub original: HonchoCreatedResourcesReceipt,
    pub destination_channel_id: Uuid,
    pub diagnostic: EmployeeNamespaceDiagnostic,
    pub valid_until: DateTime<Utc>,
}

fn env_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
        && value.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

pub fn hash(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn stamp(value: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32, u32, u64) {
    (value.dev(), value.ino(), value.len(), value.mtime(), value.mtime_nsec(),
        value.ctime(), value.ctime_nsec(), value.mode(), value.uid(), value.nlink())
}

pub fn read(path: &Path) -> Result<(Config, String)> {
    if !path.is_absolute() || path.canonicalize().map_err(|_| "config_path")? != path {
        return Err("config_path");
    }
    let fd = rustix::fs::open(path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty()).map_err(|_| "config_open")?;
    let before = rustix::fs::fstat(&fd).map_err(|_| "config_metadata")?;
    if rustix::fs::FileType::from_raw_mode(before.st_mode) != rustix::fs::FileType::RegularFile
        || before.st_mode & 0o777 != 0o600 || before.st_nlink != 1
        || before.st_uid != rustix::process::getuid().as_raw()
        || !(1..=65_536).contains(&before.st_size)
    {
        return Err("config_metadata");
    }
    let mut file = std::fs::File::from(fd);
    let initial = file.metadata().map_err(|_| "config_metadata")?;
    let mut raw = Vec::new();
    (&mut file).take(65_537).read_to_end(&mut raw).map_err(|_| "config_read")?;
    let after = file.metadata().map_err(|_| "config_metadata")?;
    let linked = std::fs::symlink_metadata(path).map_err(|_| "config_metadata")?;
    if raw.len() as u64 != before.st_size as u64 || stamp(&initial) != stamp(&after)
        || stamp(&initial) != stamp(&linked) || !linked.is_file()
        || after.mode() & 0o777 != 0o600 || after.uid() != rustix::process::getuid().as_raw()
        || after.nlink() != 1
        || path.canonicalize().map_err(|_| "config_path")? != path
        || raw.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err("config_changed");
    }
    let config: Config = serde_json::from_slice(&raw).map_err(|_| "config_encoding")?;
    config.validate()?;
    Ok((config, hash(&raw)))
}

impl Config {
    fn validate(&self) -> Result<()> {
        let d = &self.deployment;
        let origin = url::Url::parse(&d.origin).map_err(|_| "deployment_origin")?;
        let loopback = matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if self.format != "ortak-employee-target-operator/1" || self.company_id.is_nil()
            || self.community_id.is_nil() || d.deployment_id.is_nil()
            || !matches!(self.database_port, 55432 | 55433)
            || !env_name(&self.database_env) || !env_name(&d.token_env)
            || self.database_env == d.token_env
            || !(origin.scheme() == "https" || origin.scheme() == "http" && loopback)
            || origin.origin().ascii_serialization() != d.origin
            || !origin.username().is_empty() || origin.password().is_some()
            || self.targets.is_empty() || self.targets.len() > 3
            || d.endpoint_ref.is_empty() || d.endpoint_ref.len() > 256
            || d.endpoint_ref.chars().any(char::is_control)
        {
            return Err("config_selection");
        }
        let mut employees = BTreeSet::new();
        let mut operations = BTreeSet::new();
        let mut namespaces = BTreeSet::new();
        for item in &self.targets {
            let r = &item.original;
            let diagnostic = &item.diagnostic;
            if r.company_id != self.company_id || r.deployment_id != d.deployment_id
                || r.binding.endpoint_ref != d.endpoint_ref || r.binding.adapter != "honcho"
                || item.destination_channel_id.is_nil() || diagnostic.operation_id.is_nil()
                || diagnostic.employee_revision_id.is_nil() || diagnostic.employee_lifecycle_epoch < 0
                || diagnostic.challenge.len() != 64
                || !diagnostic.challenge.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || item.valid_until.timestamp_subsec_nanos() % 1000 != 0
                || !employees.insert(r.employee_id.as_str())
                || !operations.insert(diagnostic.operation_id)
                || !namespaces.insert(r.binding.workspace.as_str())
            {
                return Err("target_selection");
            }
        }
        Ok(())
    }

    pub fn database(&self) -> Result<String> {
        let value = std::env::var(&self.database_env).map_err(|_| "database_credential")?;
        if value.len() > 4096 { return Err("database_selection"); }
        let parsed = url::Url::parse(&value).map_err(|_| "database_selection")?;
        if !matches!(parsed.scheme(), "postgres" | "postgresql")
            || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            || parsed.port() != Some(self.database_port)
            || parsed.query().is_some() || parsed.fragment().is_some()
            || parsed.path().len() < 2 || parsed.path()[1..].contains('/')
        {
            return Err("database_selection");
        }
        Ok(value)
    }
}
