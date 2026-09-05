//! Production semantic scorer stand-in that is explicitly disabled.
//!
//! Until `ortak-routing-semantic` exists (Remaining Work v1, slice D), the
//! composition root must not run a fake scorer in production. This adapter
//! makes the absence explicit: every request returns
//! [`SemanticScoringFailure::Disabled`] with honest pinned metadata, so the
//! router records one silent decision with
//! [`RoutingReason::SemanticScorerDisabled`](ortak_domain::RoutingReason::SemanticScorerDisabled)
//! and no placeholder score ever reaches persistence. It performs no I/O.

use ortak_router::SemanticScoringFailure;

use crate::ports::{ScoringOutcome, SemanticScorer};
use crate::routing::ScorerMetadata;
use crate::semantic::SemanticScoringInput;

/// Adapter name pinned on every decision this scorer touches.
pub const DISABLED_SCORER_ADAPTER: &str = "disabled";
/// Scorer version pinned on every decision this scorer touches.
pub const DISABLED_SCORER_VERSION: &str = "disabled-v0";

/// Semantic scorer that refuses every request without any remote call.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisabledSemanticScorer;

impl DisabledSemanticScorer {
    /// Builds the disabled scorer.
    pub fn new() -> Self {
        Self
    }

    /// Metadata recorded for a disabled scoring attempt.
    pub fn metadata() -> ScorerMetadata {
        ScorerMetadata {
            adapter: DISABLED_SCORER_ADAPTER.to_owned(),
            model: None,
            prompt_version: None,
            version: DISABLED_SCORER_VERSION.to_owned(),
            latency_ms: Some(0),
            usage: None,
        }
    }
}

impl SemanticScorer for DisabledSemanticScorer {
    fn metadata(&self) -> ScorerMetadata {
        Self::metadata()
    }

    async fn score(&self, _input: &SemanticScoringInput) -> ScoringOutcome {
        ScoringOutcome {
            result: Err(SemanticScoringFailure::Disabled),
            metadata: Self::metadata(),
        }
    }
}

#[cfg(test)]
mod tests {
    use ortak_domain::{
        Employee, EmployeeCatalog, EmployeeManifest, MessageEnvelope, RoutingMode, RoutingPolicy,
        RoutingReason,
    };
    use ortak_router::{Router, RoutingPreparation};

    use super::*;

    fn fixture(name: &str) -> Employee {
        let yaml = std::fs::read_to_string(format!(
            "{}/../../config/employees/{name}.yaml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("read fixture");
        let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
        let mut employee = manifest.employee;
        employee.status = ortak_domain::EmployeeStatus::Active;
        employee
    }

    #[tokio::test]
    async fn disabled_scorer_yields_a_silent_disabled_decision_with_pinned_metadata() {
        let policy = RoutingPolicy::default();
        let router = Router::new(policy.clone()).expect("router");
        let catalog = EmployeeCatalog::new([fixture("cem"), fixture("zeynep")]).expect("catalog");
        let message =
            MessageEnvelope::human_channel("ab".repeat(32), "sefa", "office", "Herkese merhaba");
        let RoutingPreparation::Semantic(request) = router.prepare(&message, &catalog) else {
            panic!("an untargeted human message must reach semantic preparation");
        };

        let input = crate::semantic::tests::fixture_input(request.clone());
        let outcome = DisabledSemanticScorer::new().score(&input).await;
        assert_eq!(outcome.result, Err(SemanticScoringFailure::Disabled));
        assert_eq!(outcome.metadata.adapter, DISABLED_SCORER_ADAPTER);
        assert_eq!(outcome.metadata.version, DISABLED_SCORER_VERSION);
        assert_eq!(outcome.metadata.model, None);

        let decision = router.complete_semantic(request, outcome.result);
        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(
            decision.summary_reason,
            RoutingReason::SemanticScorerDisabled
        );
        assert_eq!(decision.wake_count(), 0);
        assert!(decision
            .recipients
            .iter()
            .all(|recipient| recipient.score.is_none()));
    }
}
