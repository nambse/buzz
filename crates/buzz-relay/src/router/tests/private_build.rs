//! Private build regression through the actual routers and shared write seam.

use super::*;
use buzz_auth::Scope;
use buzz_core::kind::*;
use buzz_core::{CommunityId, TenantContext};
use nostr::{EventBuilder, Keys, Kind};

use crate::handlers::ingest::{ingest_event, HttpAuthMethod, IngestAuth, IngestError};

#[tokio::test]
async fn private_build_refuses_legacy_workflow_and_git_routes_and_writes() {
    let mut state = readiness_state(Arc::new(ScriptedReadinessEvaluator::new([]))).await;
    let config = Arc::make_mut(
        &mut Arc::get_mut(&mut state)
            .expect("unshared fixture state")
            .config,
    );
    config.web_dir = None;
    config.admin = None;
    let router = build_router(state.clone());
    for (method, path, expected) in [
        ("GET", "/health", StatusCode::OK),
        ("POST", "/hooks/fixture", StatusCode::NOT_FOUND),
        ("GET", "/workflows/fixture/runs", StatusCode::NOT_FOUND),
        (
            "GET",
            "/workflows/fixture/runs/fixture/approvals",
            StatusCode::NOT_FOUND,
        ),
        ("GET", "/git/owner/repo/info/refs", StatusCode::NOT_FOUND),
        (
            "POST",
            "/git/owner/repo/git-upload-pack",
            StatusCode::NOT_FOUND,
        ),
        (
            "POST",
            "/git/owner/repo/git-receive-pack",
            StatusCode::NOT_FOUND,
        ),
        ("POST", "/internal/git/policy", StatusCode::NOT_FOUND),
        // The ordinary signed event endpoint remains mounted.
        ("GET", "/events", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), expected, "{method} {path}");
    }

    let tenant = TenantContext::resolved(
        CommunityId::from_uuid(uuid::Uuid::new_v4()),
        "fixture.invalid",
    );
    let keys = Keys::generate();
    let auths = [
        IngestAuth::Nip42 {
            pubkey: keys.public_key(),
            scopes: vec![Scope::MessagesWrite],
            channel_ids: None,
            conn_id: uuid::Uuid::new_v4(),
        },
        IngestAuth::Http {
            pubkey: keys.public_key(),
            scopes: vec![Scope::MessagesWrite],
            auth_method: HttpAuthMethod::Nip98,
        },
    ];
    for kind in [
        KIND_WORKFLOW_DEF,
        KIND_WORKFLOW_TRIGGER,
        KIND_APPROVAL_GRANT,
        KIND_APPROVAL_DENY,
        KIND_GIT_REPO_ANNOUNCEMENT,
        KIND_GIT_REPO_STATE,
        KIND_GIT_PATCH,
        KIND_GIT_PULL_REQUEST,
        KIND_GIT_PR_UPDATE,
        KIND_GIT_ISSUE,
        KIND_GIT_STATUS_OPEN,
        KIND_GIT_STATUS_MERGED,
        KIND_GIT_STATUS_CLOSED,
        KIND_GIT_STATUS_DRAFT,
    ]
    .into_iter()
    .chain(46001..=46012)
    {
        let event = EventBuilder::new(Kind::Custom(kind as u16), "synthetic legacy write")
            .sign_with_keys(&keys)
            .expect("signed event");
        for auth in &auths {
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                ingest_event(&state, &tenant, event.clone(), auth.clone()),
            )
            .await
            .expect("refusal must not reach the unavailable database");
            assert!(
                matches!(result, Err(IngestError::Rejected(reason)) if reason.starts_with("unsupported: legacy")),
                "kind {kind}"
            );
        }
        if matches!(
            kind,
            KIND_WORKFLOW_DEF | KIND_WORKFLOW_TRIGGER | KIND_APPROVAL_GRANT | KIND_APPROVAL_DENY
        ) {
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                crate::handlers::command_executor::handle_command(
                    &tenant,
                    &state,
                    event,
                    auths[0].clone(),
                ),
            )
            .await
            .expect("direct command refusal must precede database I/O");
            assert!(
                matches!(result, Err(IngestError::Rejected(reason)) if reason.starts_with("unsupported: legacy"))
            );
        }
    }
    for kind in [
        KIND_STREAM_MESSAGE,
        KIND_STREAM_MESSAGE_V2,
        KIND_GIFT_WRAP,
        KIND_READ_STATE,
    ] {
        assert_eq!(crate::legacy::unavailable_event(kind), None);
    }
}
