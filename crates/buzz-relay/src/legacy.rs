//! Compile-selected legacy write families. Historical reads remain unchanged.

use buzz_core::kind::*;

pub(crate) fn unavailable_event(kind: u32) -> Option<&'static str> {
    if !cfg!(feature = "legacy-workflow")
        && (matches!(
            kind,
            KIND_WORKFLOW_DEF | KIND_WORKFLOW_TRIGGER | KIND_APPROVAL_GRANT | KIND_APPROVAL_DENY
        ) || is_workflow_execution_kind(kind))
    {
        return Some("unsupported: legacy workflows are not available in this build");
    }
    if !cfg!(feature = "legacy-git")
        && matches!(
            kind,
            KIND_GIT_REPO_ANNOUNCEMENT
                | KIND_GIT_REPO_STATE
                | KIND_GIT_PATCH
                | KIND_GIT_PULL_REQUEST
                | KIND_GIT_PR_UPDATE
                | KIND_GIT_ISSUE
                | KIND_GIT_STATUS_OPEN
                | KIND_GIT_STATUS_MERGED
                | KIND_GIT_STATUS_CLOSED
                | KIND_GIT_STATUS_DRAFT
        )
    {
        return Some("unsupported: legacy git hosting is not available in this build");
    }
    None
}
