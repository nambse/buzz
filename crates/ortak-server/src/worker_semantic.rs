//! Optional semantic evidence; absence performs no credential lookup or HTTP.

use ortak_control::{
    ports::{ScoringOutcome, SemanticScorer},
    routing::ScorerMetadata,
    scorer::DisabledSemanticScorer,
    CompanyScope, SemanticScoringInput,
};
use ortak_router::SemanticScoringFailure;
use ortak_routing_semantic::{
    ChatCompletionsScorer, HermesCodexConfig, HermesCodexScorer, SemanticConfig, SemanticToken,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySelection {
    deployment: SemanticConfig,
    token_env: String,
}

#[derive(Deserialize)]
enum HermesAdapter {
    #[serde(rename = "hermes-codex")]
    Codex,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HermesSelection {
    adapter: HermesAdapter,
    deployment: HermesCodexConfig,
    bridge_token_env: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SemanticSelection {
    Hermes(HermesSelection),
    Legacy(LegacySelection),
}

pub(crate) enum WorkerSemantic {
    Disabled,
    Unavailable,
    Configured(Box<ChatCompletionsScorer>),
    Hermes(Box<HermesCodexScorer>),
}

impl WorkerSemantic {
    pub fn new(scope: &CompanyScope, selection: Option<serde_json::Value>) -> Self {
        let Some(selection) = selection else {
            return Self::Disabled;
        };
        let configured = serde_json::from_value::<SemanticSelection>(selection)
            .map_err(|_| "invalid semantic selection")
            .and_then(|selection| match selection {
                SemanticSelection::Legacy(selection) => {
                    selection.deployment.validate()?;
                    let token = SemanticToken::from_env(
                        selection.deployment.token_ref.clone(),
                        &selection.token_env,
                    )?;
                    ChatCompletionsScorer::new(scope, selection.deployment, token)
                        .map(|scorer| Self::Configured(Box::new(scorer)))
                }
                SemanticSelection::Hermes(selection) => {
                    let HermesAdapter::Codex = selection.adapter;
                    selection.deployment.validate()?;
                    let token = SemanticToken::from_env(
                        selection.deployment.bridge_token_ref.clone(),
                        &selection.bridge_token_env,
                    )?;
                    HermesCodexScorer::new(scope, selection.deployment, token)
                        .map(|scorer| Self::Hermes(Box::new(scorer)))
                }
            });
        match configured {
            Ok(scorer) => scorer,
            Err(_) => {
                eprintln!(
                    "ortak-worker: semantic configuration unavailable; untargeted messages remain silent"
                );
                Self::Unavailable
            }
        }
    }
}

impl SemanticScorer for WorkerSemantic {
    fn metadata(&self) -> ScorerMetadata {
        match self {
            Self::Disabled => DisabledSemanticScorer::metadata(),
            Self::Configured(scorer) => scorer.metadata(),
            Self::Hermes(scorer) => scorer.metadata(),
            Self::Unavailable => ScorerMetadata {
                adapter: "unavailable".to_owned(),
                model: None,
                prompt_version: None,
                version: "semantic-configuration-unavailable-v1".to_owned(),
                latency_ms: Some(0),
                usage: None,
            },
        }
    }

    async fn score(
        &self,
        input: &SemanticScoringInput,
        budget: ortak_control::ScoringBudget,
    ) -> ScoringOutcome {
        match self {
            Self::Configured(scorer) => scorer.score(input, budget).await,
            Self::Hermes(scorer) => scorer.score(input, budget).await,
            Self::Disabled => ScoringOutcome {
                result: Err(SemanticScoringFailure::Disabled),
                metadata: self.metadata(),
            },
            Self::Unavailable => ScoringOutcome {
                result: Err(SemanticScoringFailure::Unavailable),
                metadata: self.metadata(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absence_is_disabled_and_bad_selection_does_not_prevent_recovery() {
        let company = ortak_control::fakes::InMemoryProvisioningRepository::new();
        assert!(matches!(
            WorkerSemantic::new(&company.scope(), None),
            WorkerSemantic::Disabled
        ));
        let selection = serde_json::json!({
            "deployment":{"deployment_id":uuid::Uuid::new_v4(),"origin":"http://127.0.0.1:1",
                "model":"fixture-model","response_model":"fixture-model","token_ref":"credential://test/semantic"},
            "token_env":"invalid environment name"
        });
        assert!(matches!(
            WorkerSemantic::new(&company.scope(), Some(selection)),
            WorkerSemantic::Unavailable
        ));
        assert!(matches!(
            WorkerSemantic::new(
                &company.scope(),
                Some(serde_json::json!({"deployment":123}))
            ),
            WorkerSemantic::Unavailable
        ));
    }
}
