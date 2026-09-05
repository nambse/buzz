use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use ortak_observability::ActivityError;

pub(crate) struct ApiError(pub StatusCode, pub &'static str);

impl ApiError {
    pub(crate) fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
    }
    pub(crate) fn invalid() -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request")
    }
    pub(crate) fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response =
            (self.0, Json(serde_json::json!({"error": {"code": self.1}}))).into_response();
        if self.0 == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                "www-authenticate",
                axum::http::HeaderValue::from_static("Nostr"),
            );
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(_: sqlx::Error) -> Self {
        Self::unavailable()
    }
}

impl From<ActivityError> for ApiError {
    fn from(error: ActivityError) -> Self {
        match error {
            ActivityError::RunNotFound { .. } => Self::not_found(),
            ActivityError::InvalidQuery(_) => Self::invalid(),
            _ => Self::unavailable(),
        }
    }
}

impl From<ortak_work::WorkError> for ApiError {
    fn from(error: ortak_work::WorkError) -> Self {
        use ortak_domain::DomainError;
        use ortak_work::WorkError;
        let conflict = || Self(StatusCode::CONFLICT, "work_conflict");
        match error {
            WorkError::AccessDenied => Self(StatusCode::FORBIDDEN, "forbidden"),
            WorkError::EmployeeNotFound { .. }
            | WorkError::ProjectNotFound { .. }
            | WorkError::WorkItemNotFound { .. }
            | WorkError::SourceMessageNotDecided { .. } => Self::not_found(),
            WorkError::InvalidQuery(_) => Self::invalid(),
            WorkError::Domain(error) => match error {
                DomainError::InvalidWorkTransition { .. }
                | DomainError::WorkItemTerminal { .. }
                | DomainError::CompletionBlocked { .. }
                | DomainError::DependenciesUnresolved { .. }
                | DomainError::CriterionAlreadySatisfied
                | DomainError::ApprovalAlreadyResolved
                | DomainError::DuplicateAssignment
                | DomainError::ProjectArchived => conflict(),
                _ => Self::invalid(),
            },
            WorkError::ProjectConflict { .. }
            | WorkError::ProjectArchived { .. }
            | WorkError::VersionConflict { .. }
            | WorkError::PromotionConflict { .. }
            | WorkError::OperationConflict
            | WorkError::EmployeeNotAssignable { .. } => conflict(),
            _ => Self::unavailable(),
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, ApiError>;
