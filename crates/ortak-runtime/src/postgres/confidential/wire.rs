use ortak_control::confidential::ValidatedIdentity;
use ortak_domain::RuntimeBinding;
use serde::Serialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{ConfidentialAdmissionError as Error, Result};

// Declaration order is lexical, including the reduced protected context. This
// is NOT RunContext serde: its absent Work/memory fields are reconstructed only
// in the volatile consumer. No serde_json::Value owns a copy of plaintext here.
#[derive(Serialize)]
struct Context<'a> { conversation_ref: &'a str, reply_to_message_id: Option<&'a str> }
#[derive(Serialize)]
struct Policy { allowed_networks: [();0], allowed_tools: [();0], allowed_workspaces: [();0], approval_required: [();0] }
#[derive(Serialize)]
struct Binding<'a> {
    adapter: &'a str, credential_refs: &'a [ortak_domain::CredentialRef],
    model: &'a str, options: &'a std::collections::BTreeMap<String,String>,
    profile_ref: &'a Option<String>, workspace_ref: &'a str,
}
#[derive(Serialize)]
struct Spec<'a> {
    binding: Binding<'a>, context: Context<'a>, employee_id: &'a str,
    idempotency_key: &'a str, input: &'a str, permissions: Policy,
    revision_id: Uuid, run_id: Uuid,
}
#[derive(Serialize)]
struct Inner<'a> { format: &'static str, identity: serde_json::Value, spec: Spec<'a> }

pub(super) fn snapshot(identity:&ValidatedIdentity,binding:&RuntimeBinding,run:Uuid,revision:Uuid,
    employee:&str,start_key:&str,channel:&str,reply:Option<&str>,input:&str)->Result<Zeroizing<Vec<u8>>>{
    if input.is_empty() || input.len()>8192 || input.contains('\0') { return Err(Error::Payload); }
    let identity=serde_json::from_slice(identity.canonical_bytes()).map_err(|_|Error::Payload)?;
    let inner=Inner {
        format:"ortak-confidential-run/1",identity,
        spec:Spec {
            binding:Binding {adapter:&binding.adapter,credential_refs:&binding.credential_refs,
                model:&binding.model,options:&binding.options,profile_ref:&binding.profile_ref,workspace_ref:&binding.workspace_ref},
            context:Context { conversation_ref:channel,reply_to_message_id:reply },employee_id:employee,
            idempotency_key:start_key,input,permissions:Policy {allowed_networks:[],allowed_tools:[],allowed_workspaces:[],approval_required:[]},
            revision_id:revision,run_id:run,
        }
    };
    let bytes=Zeroizing::new(serde_json::to_vec(&inner).map_err(|_|Error::Payload)?);
    if bytes.len()>48*1024 { return Err(Error::Payload); }
    Ok(bytes)
}
