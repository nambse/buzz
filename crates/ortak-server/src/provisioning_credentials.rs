//! Explicit credential ownership; OAuth material stays inside the bridge.

use ortak_control::{
    adapter::Detail,
    credentials::{
        CredentialError, CredentialReferenceStatus, CredentialResolver, EnvCredentialBinding,
        EnvCredentialResolver,
    },
};
use ortak_domain::{CredentialRef, RuntimeBinding};
use ortak_runtime::hermes::HermesAdapter;
use serde::Deserialize;

/// Exactly one manager owns the runtime credential references in this operation.
#[derive(Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeCredentialSelection {
    /// Explicit finite local environment mappings, for supported key-based profiles.
    Environment {
        /// Authorized reference-to-environment mappings; never credential values.
        bindings: Vec<EnvCredentialBinding>,
    },
    /// Current read-only enrollment status from the selected Hermes profile.
    /// OAuth access/refresh tokens never enter the provisioning process.
    HermesProfile {},
}

impl RuntimeCredentialSelection {
    pub(super) fn environment_bindings(&self) -> &[EnvCredentialBinding] {
        match self {
            Self::Environment { bindings } => bindings,
            Self::HermesProfile {} => &[],
        }
    }
}

pub(super) struct PreparedCredentialResolver<'a> {
    pub environment: EnvCredentialResolver,
    pub runtime: &'a HermesAdapter,
    pub bridge_binding: Option<&'a RuntimeBinding>,
}

impl CredentialResolver for PreparedCredentialResolver<'_> {
    async fn verify_reference(
        &self,
        reference: &CredentialRef,
    ) -> Result<CredentialReferenceStatus, CredentialError> {
        if let Some(binding) = self
            .bridge_binding
            .filter(|b| b.credential_refs.contains(reference))
        {
            let resolved = self
                .runtime
                .resolvable_credential_references(binding)
                .await
                .map_err(|_| CredentialError::Unavailable {
                    detail: Detail::new("selected Hermes credential manager is unavailable"),
                })?;
            return Ok(if resolved.contains(reference) {
                CredentialReferenceStatus::Resolvable
            } else {
                CredentialReferenceStatus::Missing
            });
        }
        self.environment.verify_reference(reference).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn oauth_existence_uses_only_exact_authenticated_profile_and_never_returns_tokens() {
        let company = Uuid::new_v4();
        let binding: RuntimeBinding = serde_json::from_value(json!({
            "adapter":"hermes", "profile_ref":"fresh-profile", "model":"selected-model", "workspace_ref":"none",
            "credential_refs":["secret://fresh/oauth"], "options":{"reasoning_effort":"max"}
        }))
        .unwrap();
        let reply = Arc::new(Mutex::new(
            json!({"profile_ref":"fresh-profile", "healthy":false,
            "credential_references":["secret://fresh/oauth"]}),
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let expected = json!({"company_id":company,"binding":binding});
        let app = Router::new()
            .route(
                "/v1/profiles/inspect",
                post({
                    let calls = Arc::clone(&calls);
                    move |State(reply): State<Arc<Mutex<Value>>>,
                          headers: HeaderMap,
                          Json(body): Json<Value>| {
                        let expected = expected.clone();
                        let calls = Arc::clone(&calls);
                        async move {
                            assert_eq!(headers["authorization"], "Bearer selected-bridge-token");
                            assert_eq!(body, expected);
                            calls.fetch_add(1, Ordering::SeqCst);
                            Json(reply.lock().unwrap().clone())
                        }
                    }
                }),
            )
            .with_state(Arc::clone(&reply));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let runtime = HermesAdapter::new(company, &origin, "selected-bridge-token").unwrap();
        let resolver = PreparedCredentialResolver {
            environment: EnvCredentialResolver::new(vec![]).unwrap(),
            runtime: &runtime,
            bridge_binding: Some(&binding),
        };
        let reference = &binding.credential_refs[0];
        // Enrollment can exist while expired or lacking a recent execution witness.
        assert_eq!(
            resolver.verify_reference(reference).await.unwrap(),
            CredentialReferenceStatus::Resolvable
        );
        reply.lock().unwrap()["credential_references"] = json!([]);
        assert_eq!(
            resolver.verify_reference(reference).await.unwrap(),
            CredentialReferenceStatus::Missing
        );
        reply.lock().unwrap()["credential_references"] = json!(["secret://unselected/oauth"]);
        assert!(resolver.verify_reference(reference).await.is_err());
        reply.lock().unwrap()["credential_references"] = json!([reference, reference]);
        assert!(resolver.verify_reference(reference).await.is_err());
        reply.lock().unwrap()["credential_references"] = json!([reference]);
        reply.lock().unwrap()["profile_ref"] = json!("different-profile");
        assert!(resolver.verify_reference(reference).await.is_err());
        let unselected = CredentialRef::parse("secret://unselected/oauth").unwrap();
        assert!(matches!(
            resolver.verify_reference(&unselected).await,
            Err(CredentialError::Unauthorized { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        server.abort();
        let _ = server.await;
    }
}
