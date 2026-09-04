//! Deterministic-first conversation routing for Ortak employees.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ortak_domain::{
    normalize_alias, ConversationContext, DomainError, Employee, EmployeeCatalog, EmployeeId,
    EmployeeStatus, MessageEnvelope, MessageOrigin, RecipientAction, RecipientDecision,
    RoutingDecision, RoutingMode, RoutingPolicy, RoutingReason, SemanticScore,
};
use thiserror::Error;

/// Bounded failures produced by an out-of-process semantic scoring adapter.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SemanticScoringFailure {
    /// The configured scorer could not be reached or used.
    #[error("semantic scorer unavailable")]
    Unavailable,
    /// The control layer's scoring deadline elapsed.
    #[error("semantic scorer timed out")]
    TimedOut,
    /// The scoring operation was cancelled before it produced a result.
    #[error("semantic scoring cancelled")]
    Cancelled,
}

/// Least-privilege employee metadata sent to a semantic scorer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticCandidate {
    employee_id: EmployeeId,
    name: String,
    title: String,
    biography: String,
    responsibilities: Vec<String>,
    domains: Vec<String>,
}

impl SemanticCandidate {
    /// Returns the stable employee identifier.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.employee_id
    }

    /// Returns the employee's display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the employee's company title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the employee's routing biography.
    pub fn biography(&self) -> &str {
        &self.biography
    }

    /// Returns the employee's declared responsibilities.
    pub fn responsibilities(&self) -> &[String] {
        &self.responsibilities
    }

    /// Returns the employee's semantic expertise domains.
    pub fn domains(&self) -> &[String] {
        &self.domains
    }
}

impl From<&Employee> for SemanticCandidate {
    fn from(employee: &Employee) -> Self {
        Self {
            employee_id: employee.id.clone(),
            name: employee.name.clone(),
            title: employee.title.clone(),
            biography: employee.biography.clone(),
            responsibilities: employee.responsibilities.clone(),
            domains: employee.domains.clone(),
        }
    }
}

/// Opaque continuation plus the minimal payload needed for semantic scoring.
#[derive(Clone, PartialEq)]
pub struct SemanticRoutingRequest {
    message_id: String,
    body: String,
    candidates: Vec<SemanticCandidate>,
    policy_version: String,
    policy_fingerprint: String,
    wake_limit: usize,
    effective_thresholds: BTreeMap<EmployeeId, f32>,
    excluded_recipients: Vec<RecipientDecision>,
}

impl SemanticRoutingRequest {
    /// Returns the message identifier used to correlate the scoring request.
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// Returns the bounded human-authored text to score.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Returns the least-privilege candidate views to score.
    pub fn candidates(&self) -> &[SemanticCandidate] {
        &self.candidates
    }
}

impl fmt::Debug for SemanticRoutingRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let candidate_ids = self
            .candidates
            .iter()
            .map(SemanticCandidate::employee_id)
            .collect::<Vec<_>>();
        formatter
            .debug_struct("SemanticRoutingRequest")
            .field("message_id", &self.message_id)
            .field("body_bytes", &self.body.len())
            .field("candidate_count", &self.candidates.len())
            .field("candidate_ids", &candidate_ids)
            .finish_non_exhaustive()
    }
}

/// First-phase routing result: final policy output or a semantic scoring request.
#[derive(Clone, Debug, PartialEq)]
pub enum RoutingPreparation {
    /// Routing completed without an external scorer.
    Final(RoutingDecision),
    /// The control layer must score the supplied least-privilege request.
    Semantic(SemanticRoutingRequest),
}

/// Router construction failures.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RouterError {
    /// The supplied company policy is invalid.
    #[error(transparent)]
    InvalidPolicy(#[from] DomainError),
}

/// Deterministic-first pure routing engine.
#[derive(Clone, Debug)]
pub struct Router {
    policy: RoutingPolicy,
}

impl Router {
    /// Creates a router after validating company-wide limits.
    pub fn new(policy: RoutingPolicy) -> Result<Self, RouterError> {
        policy.validate()?;
        Ok(Self { policy })
    }

    /// Applies guards and deterministic rules, yielding a final decision or scoring request.
    pub fn prepare(
        &self,
        message: &MessageEnvelope,
        catalog: &EmployeeCatalog,
    ) -> RoutingPreparation {
        if message.validate_for_routing().is_err() {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::InvalidMessage,
                Vec::new(),
            ));
        }

        if !message.kind.is_routable() {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::NonRoutableMessage,
                Vec::new(),
            ));
        }

        if message.body.len() > self.policy.max_message_bytes {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::MessageTooLarge,
                Vec::new(),
            ));
        }

        if message.chain().hop_count() >= self.policy.max_hops {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::HopLimitReached,
                Vec::new(),
            ));
        }

        if message.chain().wake_count() >= self.policy.max_chain_wakes {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::WakeBudgetExhausted,
                Vec::new(),
            ));
        }

        if matches!(
            &message.origin,
            MessageOrigin::Integration(_) | MessageOrigin::System
        ) {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::OriginCannotFanOut,
                Vec::new(),
            ));
        }

        if let Some((reason, targets)) = deterministic_targets(message, catalog) {
            return RoutingPreparation::Final(
                self.route_deterministic(message, catalog, reason, targets),
            );
        }

        if !message.origin.allows_semantic_routing() {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::OriginCannotFanOut,
                Vec::new(),
            ));
        }

        self.prepare_semantic(message, catalog)
    }

    /// Applies authoritative policy to scores produced under a control-layer deadline.
    pub fn complete_semantic(
        &self,
        request: SemanticRoutingRequest,
        result: Result<Vec<SemanticScore>, SemanticScoringFailure>,
    ) -> RoutingDecision {
        if request.policy_version != self.policy.version
            || request.policy_fingerprint != self.policy.fingerprint()
        {
            return semantic_failure_decision(request, RoutingReason::SemanticScorerUnavailable);
        }

        match result {
            Ok(scores) if valid_score_set(&scores, &request.candidates) => {
                complete_semantic_decision(request, scores)
            }
            Ok(_) | Err(SemanticScoringFailure::Unavailable) => {
                semantic_failure_decision(request, RoutingReason::SemanticScorerUnavailable)
            }
            Err(SemanticScoringFailure::TimedOut) => {
                semantic_failure_decision(request, RoutingReason::SemanticScorerTimedOut)
            }
            Err(SemanticScoringFailure::Cancelled) => {
                semantic_failure_decision(request, RoutingReason::SemanticScoringCancelled)
            }
        }
    }

    fn route_deterministic(
        &self,
        message: &MessageEnvelope,
        catalog: &EmployeeCatalog,
        wake_reason: RoutingReason,
        targets: Vec<EmployeeId>,
    ) -> RoutingDecision {
        let remaining_chain_budget = self
            .policy
            .max_chain_wakes
            .saturating_sub(message.chain().wake_count());
        let wake_limit = self.policy.max_recipients.min(remaining_chain_budget);
        let mut wakes = 0usize;
        let mut recipients = Vec::with_capacity(targets.len());

        for employee_id in stable_deduplicate(targets) {
            let eligibility = eligibility_reason(message, catalog.get(&employee_id));
            let (action, reason) = match eligibility {
                Some(reason) => (RecipientAction::Drop, reason),
                None if wakes >= wake_limit => {
                    (RecipientAction::Drop, RoutingReason::RecipientLimitReached)
                }
                None => {
                    wakes += 1;
                    (RecipientAction::Wake, wake_reason)
                }
            };

            recipients.push(RecipientDecision {
                employee_id,
                action,
                reason,
                score: None,
                evidence: Vec::new(),
            });
        }

        let mode = if wakes > 0 {
            RoutingMode::Deterministic
        } else {
            RoutingMode::Silent
        };
        let summary_reason = if wakes > 0 {
            wake_reason
        } else {
            recipients
                .first()
                .map(|recipient| recipient.reason)
                .unwrap_or(RoutingReason::NoEligibleEmployee)
        };

        RoutingDecision {
            message_id: message.id.clone(),
            mode,
            summary_reason,
            policy_version: self.policy.version.clone(),
            policy_fingerprint: self.policy.fingerprint(),
            recipients,
        }
    }

    fn prepare_semantic(
        &self,
        message: &MessageEnvelope,
        catalog: &EmployeeCatalog,
    ) -> RoutingPreparation {
        let mut excluded_recipients = Vec::new();
        let mut candidates = Vec::new();
        let mut effective_thresholds = BTreeMap::new();

        for employee in catalog.employees() {
            if let Some(reason) = eligibility_reason(message, Some(employee)) {
                excluded_recipients.push(RecipientDecision {
                    employee_id: employee.id.clone(),
                    action: RecipientAction::Drop,
                    reason,
                    score: None,
                    evidence: Vec::new(),
                });
            } else {
                candidates.push(SemanticCandidate::from(employee));
                effective_thresholds.insert(
                    employee.id.clone(),
                    employee
                        .routing
                        .semantic_min_score
                        .unwrap_or(self.policy.semantic_threshold)
                        .max(self.policy.semantic_threshold),
                );
            }
        }

        if candidates.is_empty() {
            return RoutingPreparation::Final(self.silent(
                message,
                RoutingReason::NoEligibleEmployee,
                excluded_recipients,
            ));
        }

        let remaining_chain_budget = self
            .policy
            .max_chain_wakes
            .saturating_sub(message.chain().wake_count());
        let wake_limit = self.policy.max_recipients.min(remaining_chain_budget);

        RoutingPreparation::Semantic(SemanticRoutingRequest {
            message_id: message.id.clone(),
            body: message.body.clone(),
            candidates,
            policy_version: self.policy.version.clone(),
            policy_fingerprint: self.policy.fingerprint(),
            wake_limit,
            effective_thresholds,
            excluded_recipients,
        })
    }

    fn silent(
        &self,
        message: &MessageEnvelope,
        reason: RoutingReason,
        recipients: Vec<RecipientDecision>,
    ) -> RoutingDecision {
        RoutingDecision {
            message_id: message.id.clone(),
            mode: RoutingMode::Silent,
            summary_reason: reason,
            policy_version: self.policy.version.clone(),
            policy_fingerprint: self.policy.fingerprint(),
            recipients,
        }
    }
}

fn complete_semantic_decision(
    request: SemanticRoutingRequest,
    scores: Vec<SemanticScore>,
) -> RoutingDecision {
    let SemanticRoutingRequest {
        message_id,
        body: _,
        candidates,
        policy_version,
        policy_fingerprint,
        wake_limit,
        effective_thresholds,
        mut excluded_recipients,
    } = request;
    let mut scores_by_employee: BTreeMap<EmployeeId, SemanticScore> = scores
        .into_iter()
        .map(|score| (score.employee_id.clone(), score))
        .collect();
    let mut ranked = candidates
        .into_iter()
        .filter_map(|candidate| {
            let score = scores_by_employee.remove(&candidate.employee_id)?;
            let effective_threshold = effective_thresholds.get(&candidate.employee_id).copied()?;
            Some((candidate.employee_id, effective_threshold, score))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_id, _, left), (right_id, _, right)| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left_id.cmp(right_id))
    });

    let mut wakes = 0usize;
    for (employee_id, effective_threshold, score) in ranked {
        let (action, reason) = if score.score < effective_threshold {
            (RecipientAction::Drop, RoutingReason::BelowSemanticThreshold)
        } else if wakes >= wake_limit {
            (RecipientAction::Drop, RoutingReason::RecipientLimitReached)
        } else {
            wakes += 1;
            (RecipientAction::Wake, RoutingReason::SemanticMatch)
        };
        excluded_recipients.push(RecipientDecision {
            employee_id,
            action,
            reason,
            score: Some(score.score),
            evidence: score.evidence,
        });
    }

    let (mode, summary_reason) = if wakes > 0 {
        (RoutingMode::Semantic, RoutingReason::SemanticMatch)
    } else {
        (RoutingMode::Silent, RoutingReason::NoRelevantEmployee)
    };
    RoutingDecision {
        message_id,
        mode,
        summary_reason,
        policy_version,
        policy_fingerprint,
        recipients: excluded_recipients,
    }
}

fn semantic_failure_decision(
    request: SemanticRoutingRequest,
    reason: RoutingReason,
) -> RoutingDecision {
    let SemanticRoutingRequest {
        message_id,
        body: _,
        candidates,
        policy_version,
        policy_fingerprint,
        wake_limit: _,
        effective_thresholds: _,
        mut excluded_recipients,
    } = request;
    excluded_recipients.extend(candidates.into_iter().map(|candidate| RecipientDecision {
        employee_id: candidate.employee_id,
        action: RecipientAction::Drop,
        reason,
        score: None,
        evidence: Vec::new(),
    }));
    RoutingDecision {
        message_id,
        mode: RoutingMode::Silent,
        summary_reason: reason,
        policy_version,
        policy_fingerprint,
        recipients: excluded_recipients,
    }
}

fn deterministic_targets(
    message: &MessageEnvelope,
    catalog: &EmployeeCatalog,
) -> Option<(RoutingReason, Vec<EmployeeId>)> {
    if let ConversationContext::Direct {
        employee_participants,
        ..
    } = &message.conversation
    {
        return Some((RoutingReason::DirectMessage, employee_participants.clone()));
    }
    if !message.dispatch_targets.is_empty() {
        return Some((
            RoutingReason::StructuredDispatch,
            message.dispatch_targets.clone(),
        ));
    }
    if !message.structured_mentions.is_empty() {
        return Some((
            RoutingReason::StructuredMention,
            message.structured_mentions.clone(),
        ));
    }
    if let Some(employee_id) = message
        .reply_to
        .as_ref()
        .and_then(|reply| reply.origin.employee_id())
    {
        return Some((RoutingReason::ReplyToEmployee, vec![employee_id.clone()]));
    }
    if message.origin.allows_semantic_routing() {
        let aliases = explicit_alias_targets(&message.body, catalog);
        if !aliases.is_empty() {
            return Some((RoutingReason::ExplicitAlias, aliases));
        }
        if !message.assigned_employee_ids.is_empty() {
            return Some((
                RoutingReason::WorkAssignment,
                message.assigned_employee_ids.clone(),
            ));
        }
    }
    None
}

fn explicit_alias_targets(body: &str, catalog: &EmployeeCatalog) -> Vec<EmployeeId> {
    let normalized_body = normalize_message_text(body);
    let mut targets = BTreeSet::new();
    let mut longest_leading: Option<(usize, EmployeeId)> = None;
    let mut longest_mentions: BTreeMap<usize, (usize, EmployeeId)> = BTreeMap::new();

    for (alias, employee_id) in catalog.aliases() {
        if matches_leading_vocative(&normalized_body, alias)
            && longest_leading
                .as_ref()
                .is_none_or(|(length, _)| alias.len() > *length)
        {
            longest_leading = Some((alias.len(), employee_id.clone()));
        }
        for index in at_mention_positions(&normalized_body, alias) {
            let entry = longest_mentions
                .entry(index)
                .or_insert_with(|| (alias.len(), employee_id.clone()));
            if alias.len() > entry.0 {
                *entry = (alias.len(), employee_id.clone());
            }
        }
    }

    if let Some((_length, employee_id)) = longest_leading {
        targets.insert(employee_id);
    }
    targets.extend(
        longest_mentions
            .into_values()
            .map(|(_length, employee_id)| employee_id),
    );

    targets.into_iter().collect()
}

fn normalize_message_text(value: &str) -> String {
    // Alias normalization also performs NFKC, case folding, and whitespace
    // collapse. Preserve an interior @ while accepting a leading @ as explicit.
    let leading_at = value.trim_start().starts_with('@');
    let mut normalized = normalize_alias(value);
    if !leading_at {
        // normalize_alias only strips a leading @, so interior mentions survive.
        normalized = value
            .split_whitespace()
            .map(normalize_alias_fragment)
            .collect::<Vec<_>>()
            .join(" ");
    }
    normalized
}

fn normalize_alias_fragment(value: &str) -> String {
    if let Some(alias) = value.strip_prefix('@') {
        format!("@{}", normalize_alias(alias))
    } else {
        normalize_alias(value)
    }
}

fn matches_leading_vocative(body: &str, alias: &str) -> bool {
    body.strip_prefix(alias)
        .is_some_and(|suffix| suffix.is_empty() || starts_with_boundary(suffix))
}

fn at_mention_positions(body: &str, alias: &str) -> Vec<usize> {
    let needle = format!("@{alias}");
    body.match_indices(&needle)
        .filter_map(|(index, _)| {
            let suffix = &body[index + needle.len()..];
            (has_boundary_before(body, index)
                && (suffix.is_empty() || starts_with_boundary(suffix)))
            .then_some(index)
        })
        .collect()
}

fn has_boundary_before(value: &str, byte_index: usize) -> bool {
    byte_index == 0
        || value[..byte_index]
            .chars()
            .next_back()
            .is_some_and(is_boundary)
}

fn starts_with_boundary(value: &str) -> bool {
    value.chars().next().is_some_and(is_boundary)
}

fn is_boundary(character: char) -> bool {
    character.is_whitespace()
        || (!character.is_alphanumeric() && character != '_' && character != '-')
}

fn eligibility_reason(
    message: &MessageEnvelope,
    employee: Option<&Employee>,
) -> Option<RoutingReason> {
    let employee = match employee {
        Some(employee) => employee,
        None => return Some(RoutingReason::UnknownTarget),
    };
    if message.origin.employee_id() == Some(&employee.id) {
        return Some(RoutingReason::SelfOrigin);
    }
    if message.chain().has_visited(&employee.id) {
        return Some(RoutingReason::AlreadyVisited);
    }
    if employee.status != EmployeeStatus::Active {
        return Some(RoutingReason::EmployeeInactive);
    }
    if !employee.routing.enabled {
        return Some(RoutingReason::RoutingDisabled);
    }
    None
}

fn valid_score_set(scores: &[SemanticScore], candidates: &[SemanticCandidate]) -> bool {
    if scores.len() != candidates.len() {
        return false;
    }
    let eligible_ids = candidates
        .iter()
        .map(|candidate| candidate.employee_id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();

    scores.iter().all(|score| {
        score.validate().is_ok()
            && eligible_ids.contains(&score.employee_id)
            && seen.insert(score.employee_id.clone())
    })
}

fn stable_deduplicate(employee_ids: Vec<EmployeeId>) -> Vec<EmployeeId> {
    let mut seen = BTreeSet::new();
    employee_ids
        .into_iter()
        .filter(|employee_id| seen.insert(employee_id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ortak_domain::{
        ConversationContext, CredentialRef, DeliveryChain, EmployeeCatalog, EmployeeId,
        EmployeeManifest, EmployeeStatus, EvidenceLabel, MessageEnvelope, MessageKind,
        MessageOrigin, RecipientAction, ReplyContext, RoutingDecision, RoutingMode, RoutingPolicy,
        RoutingReason, SemanticScore,
    };

    use super::{Router, RoutingPreparation, SemanticRoutingRequest, SemanticScoringFailure};

    #[derive(Clone, Debug, Default)]
    struct StaticScorer {
        scores: BTreeMap<EmployeeId, f32>,
    }

    impl StaticScorer {
        fn new(scores: impl IntoIterator<Item = (&'static str, f32)>) -> Self {
            Self {
                scores: scores
                    .into_iter()
                    .map(|(id, score)| {
                        let id = EmployeeId::parse(id).expect("test employee id must be valid");
                        (id, score)
                    })
                    .collect(),
            }
        }
    }

    impl StaticScorer {
        fn score(&self, request: &SemanticRoutingRequest) -> Vec<SemanticScore> {
            request
                .candidates()
                .iter()
                .filter_map(|candidate| {
                    self.scores
                        .get(candidate.employee_id())
                        .copied()
                        .map(|score| SemanticScore {
                            employee_id: candidate.employee_id().clone(),
                            score,
                            evidence: vec![EvidenceLabel::parse("fixture_score")
                                .expect("fixture evidence label must be valid")],
                        })
                })
                .collect()
        }
    }

    fn fixtures() -> EmployeeCatalog {
        let cem: EmployeeManifest =
            serde_yaml::from_str(include_str!("../../../config/employees/cem.yaml"))
                .expect("Cem fixture must deserialize");
        let zeynep: EmployeeManifest =
            serde_yaml::from_str(include_str!("../../../config/employees/zeynep.yaml"))
                .expect("Zeynep fixture must deserialize");
        cem.validate().expect("Cem fixture must validate");
        zeynep.validate().expect("Zeynep fixture must validate");
        let employees = [cem.employee, zeynep.employee].map(|mut employee| {
            // Provisioning activates an immutable revision only after adapter
            // health checks. Router policy tests model that completed transition.
            employee.status = EmployeeStatus::Active;
            employee
        });
        EmployeeCatalog::new(employees).expect("fixtures must form a valid employee catalog")
    }

    fn employee_id(value: &str) -> EmployeeId {
        EmployeeId::parse(value).expect("test employee id must be valid")
    }

    fn default_router() -> Router {
        Router::new(RoutingPolicy::default()).expect("default policy must be valid")
    }

    fn final_decision(
        router: &Router,
        message: &MessageEnvelope,
        catalog: &EmployeeCatalog,
    ) -> RoutingDecision {
        match router.prepare(message, catalog) {
            RoutingPreparation::Final(decision) => decision,
            RoutingPreparation::Semantic(_) => {
                panic!("deterministic test unexpectedly requested semantic scoring")
            }
        }
    }

    fn semantic_decision(
        router: &Router,
        message: &MessageEnvelope,
        catalog: &EmployeeCatalog,
        scorer: &StaticScorer,
    ) -> RoutingDecision {
        let request = match router.prepare(message, catalog) {
            RoutingPreparation::Semantic(request) => request,
            RoutingPreparation::Final(decision) => {
                panic!("semantic test completed early: {decision:?}")
            }
        };
        let scores = scorer.score(&request);
        router.complete_semantic(request, Ok(scores))
    }

    #[test]
    fn leading_name_routes_only_to_cem_without_semantic_scoring() {
        let message =
            MessageEnvelope::human_channel("message-1", "sefa", "founders", "Cem selam nasılsın?");
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Deterministic);
        assert_eq!(decision.summary_reason, RoutingReason::ExplicitAlias);
        assert_eq!(
            decision.woken_employee_ids().collect::<Vec<_>>(),
            vec![&employee_id("cem")]
        );
    }

    #[test]
    fn longest_explicit_alias_wins_when_names_share_a_prefix() {
        let mut employees = fixtures().employees().cloned().collect::<Vec<_>>();
        let mut cem_yilmaz = employees
            .iter()
            .find(|employee| employee.id == employee_id("cem"))
            .expect("Cem fixture must exist")
            .clone();
        cem_yilmaz.id = employee_id("cem-yilmaz");
        cem_yilmaz.name = "Cem Yılmaz".to_owned();
        cem_yilmaz.aliases.clear();
        cem_yilmaz.office.public_key = "a".repeat(64);
        cem_yilmaz.office.signer_ref =
            CredentialRef::parse("credential://ortak-runtime/cem-yilmaz/office-signing-key")
                .expect("test signer reference must be valid");
        employees.push(cem_yilmaz);
        let catalog = EmployeeCatalog::new(employees).expect("catalog must be valid");
        let message = MessageEnvelope::human_channel(
            "message-longest-alias",
            "sefa",
            "office",
            "Cem Yılmaz, buna bakar mısın?",
        );
        let router = default_router();

        let decision = final_decision(&router, &message, &catalog);

        assert_eq!(
            decision.woken_employee_ids().collect::<Vec<_>>(),
            vec![&employee_id("cem-yilmaz")]
        );
    }

    #[test]
    fn structured_mention_has_priority_over_semantic_scores() {
        let mut message = MessageEnvelope::human_channel(
            "message-2",
            "sefa",
            "office",
            "Fitness uygulaması fikrini konuşalım",
        );
        message.structured_mentions = vec![employee_id("zeynep")];
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.summary_reason, RoutingReason::StructuredMention);
        assert_eq!(
            decision.woken_employee_ids().collect::<Vec<_>>(),
            vec![&employee_id("zeynep")]
        );
    }

    #[test]
    fn direct_conversation_without_employee_participants_never_requests_scoring() {
        let message = MessageEnvelope::root(
            "message-empty-direct",
            MessageKind::Text,
            MessageOrigin::Human("sefa".to_owned()),
            ConversationContext::Direct {
                conversation_id: "human-only".to_owned(),
                employee_participants: Vec::new(),
            },
            "This direct conversation has no employee participant",
        );
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::NoEligibleEmployee);
        assert!(decision.recipients.is_empty());
    }

    #[test]
    fn employee_origin_only_uses_explicit_structured_routes() {
        let cem = employee_id("cem");
        let zeynep = employee_id("zeynep");
        let chain = DeliveryChain::root("human-root")
            .advance_for_dispatch([&cem])
            .expect("initial human dispatch must advance server chain state");
        let policy = RoutingPolicy {
            max_hops: 3,
            ..RoutingPolicy::default()
        };
        let router = Router::new(policy).expect("policy must be valid");

        let raw_alias = MessageEnvelope::root(
            "employee-raw-alias",
            MessageKind::Text,
            MessageOrigin::Employee(cem.clone()),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "@Zeynep buna bakar mısın?",
        )
        .with_delivery_chain(chain.clone());
        assert_eq!(
            final_decision(&router, &raw_alias, &fixtures()).summary_reason,
            RoutingReason::OriginCannotFanOut
        );

        let mut structured_mention = MessageEnvelope::root(
            "employee-structured-mention",
            MessageKind::Text,
            MessageOrigin::Employee(cem.clone()),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Zeynep'e açık teslim",
        )
        .with_delivery_chain(chain.clone());
        structured_mention.structured_mentions = vec![zeynep.clone()];
        assert_eq!(
            final_decision(&router, &structured_mention, &fixtures()).summary_reason,
            RoutingReason::StructuredMention
        );

        let direct = MessageEnvelope::root(
            "employee-dm",
            MessageKind::Text,
            MessageOrigin::Employee(cem.clone()),
            ConversationContext::Direct {
                conversation_id: "cem-zeynep".to_owned(),
                employee_participants: vec![zeynep.clone()],
            },
            "Doğrudan Zeynep'e",
        )
        .with_delivery_chain(chain.clone());
        assert_eq!(
            final_decision(&router, &direct, &fixtures()).summary_reason,
            RoutingReason::DirectMessage
        );

        let mut reply = MessageEnvelope::root(
            "employee-reply",
            MessageKind::Text,
            MessageOrigin::Employee(cem),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Doğrudan yanıta devam",
        )
        .with_delivery_chain(chain);
        reply.reply_to = Some(ReplyContext {
            message_id: "zeynep-parent".to_owned(),
            origin: MessageOrigin::Employee(zeynep),
        });
        assert_eq!(
            final_decision(&router, &reply, &fixtures()).summary_reason,
            RoutingReason::ReplyToEmployee
        );
    }

    #[test]
    fn human_general_message_can_wake_a_bounded_semantic_set() {
        let message = MessageEnvelope::human_channel(
            "message-3",
            "sefa",
            "office",
            "App Store'da Solo Leveling gibi fitness app düşündüm.",
        );
        let router = default_router();
        let scorer = StaticScorer::new([("cem", 0.95), ("zeynep", 0.93)]);

        let decision = semantic_decision(&router, &message, &fixtures(), &scorer);

        assert_eq!(decision.mode, RoutingMode::Semantic);
        assert_eq!(decision.wake_count(), 2);
        assert_eq!(
            decision.woken_employee_ids().collect::<Vec<_>>(),
            vec![&employee_id("cem"), &employee_id("zeynep")]
        );
    }

    #[test]
    fn semantic_request_exposes_only_routing_metadata_and_redacts_debug_output() {
        let message = MessageEnvelope::human_channel(
            "message-private-semantic",
            "sefa",
            "office",
            "confidential acquisition plan",
        );
        let router = default_router();

        let request = match router.prepare(&message, &fixtures()) {
            RoutingPreparation::Semantic(request) => request,
            RoutingPreparation::Final(decision) => {
                panic!("semantic request unexpectedly completed: {decision:?}")
            }
        };

        assert_eq!(request.message_id(), "message-private-semantic");
        assert_eq!(request.body(), "confidential acquisition plan");
        let cem = request
            .candidates()
            .iter()
            .find(|candidate| candidate.employee_id() == &employee_id("cem"))
            .expect("Cem must be an eligible semantic candidate");
        assert_eq!(cem.name(), "Cem");
        assert_eq!(cem.title(), "Co-Founder");
        assert!(!cem.biography().is_empty());
        assert!(!cem.responsibilities().is_empty());
        assert!(!cem.domains().is_empty());

        let debug = format!("{request:?}");
        assert!(!debug.contains("confidential acquisition plan"));
        assert!(!debug.contains("credential://"));
        assert!(!debug.contains("/opt/data"));
        assert!(!debug.contains("allowed_tools"));
        assert!(debug.contains("body_bytes"));
        assert!(debug.contains("candidate_count"));
        assert!(debug.contains("cem"));
    }

    #[test]
    fn bounded_semantic_failures_are_preserved_and_fail_closed() {
        let router = default_router();

        for (failure, expected_reason) in [
            (
                SemanticScoringFailure::Unavailable,
                RoutingReason::SemanticScorerUnavailable,
            ),
            (
                SemanticScoringFailure::TimedOut,
                RoutingReason::SemanticScorerTimedOut,
            ),
            (
                SemanticScoringFailure::Cancelled,
                RoutingReason::SemanticScoringCancelled,
            ),
        ] {
            let message = MessageEnvelope::human_channel(
                format!("message-failure-{failure:?}"),
                "sefa",
                "office",
                "A general company question",
            );
            let request = match router.prepare(&message, &fixtures()) {
                RoutingPreparation::Semantic(request) => request,
                RoutingPreparation::Final(decision) => {
                    panic!("semantic request unexpectedly completed: {decision:?}")
                }
            };

            let decision = router.complete_semantic(request, Err(failure));

            assert_eq!(decision.mode, RoutingMode::Silent);
            assert_eq!(decision.summary_reason, expected_reason);
            assert_eq!(decision.wake_count(), 0);
            assert!(decision
                .recipients
                .iter()
                .all(|recipient| recipient.reason == expected_reason));
        }
    }

    #[test]
    fn employee_origin_never_enters_semantic_fanout() {
        let cem = employee_id("cem");
        let message = MessageEnvelope::root(
            "message-4",
            MessageKind::Text,
            MessageOrigin::Employee(cem),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Bu fitness fikrine ürün açısından bakabiliriz",
        );
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::OriginCannotFanOut);
        assert_eq!(decision.wake_count(), 0);
    }

    #[test]
    fn integration_and_system_origins_have_no_v0_dispatch_capability() {
        let router = default_router();
        let catalog = fixtures();

        for (id, origin) in [
            (
                "integration-message",
                MessageOrigin::Integration("calendar".to_owned()),
            ),
            ("system-message", MessageOrigin::System),
        ] {
            let mut message = MessageEnvelope::root(
                id,
                MessageKind::Text,
                origin,
                ConversationContext::Direct {
                    conversation_id: "internal-direct".to_owned(),
                    employee_participants: vec![employee_id("cem")],
                },
                "attempted structured dispatch",
            );
            message.dispatch_targets = vec![employee_id("cem")];

            let decision = final_decision(&router, &message, &catalog);
            assert_eq!(decision.mode, RoutingMode::Silent);
            assert_eq!(decision.summary_reason, RoutingReason::OriginCannotFanOut);
            assert_eq!(decision.wake_count(), 0);
        }
    }

    #[test]
    fn employee_can_explicitly_dispatch_once_but_not_revisit_a_peer() {
        let cem = employee_id("cem");
        let zeynep = employee_id("zeynep");
        let initial_chain = DeliveryChain::root("message-1")
            .advance_for_dispatch([&cem])
            .expect("first server dispatch must advance the chain");
        let mut message = MessageEnvelope::root(
            "message-5",
            MessageKind::Text,
            MessageOrigin::Employee(cem.clone()),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Zeynep'e yapılandırılmış bir görev",
        )
        .with_delivery_chain(initial_chain);
        message.dispatch_targets = vec![zeynep.clone()];
        let policy = RoutingPolicy {
            max_hops: 3,
            ..RoutingPolicy::default()
        };
        let router = Router::new(policy).expect("policy must be valid");

        let first = final_decision(&router, &message, &fixtures());
        assert_eq!(first.wake_count(), 1);

        let next_chain = message
            .chain()
            .advance_for_dispatch(first.woken_employee_ids())
            .expect("second server dispatch must advance the chain");
        let mut repeated_message = MessageEnvelope::root(
            "message-5-reply",
            MessageKind::Text,
            MessageOrigin::Employee(zeynep),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Cem'e geri dön",
        )
        .with_delivery_chain(next_chain);
        repeated_message.dispatch_targets = vec![cem];
        let repeated = final_decision(&router, &repeated_message, &fixtures());
        assert_eq!(repeated.wake_count(), 0);
        assert_eq!(repeated.recipients[0].reason, RoutingReason::AlreadyVisited);
    }

    #[test]
    fn hard_hop_limit_silences_even_an_explicit_dispatch() {
        let cem = employee_id("cem");
        let zeynep = employee_id("zeynep");
        let chain = DeliveryChain::root("message-1")
            .advance_for_dispatch([&cem])
            .and_then(|chain| chain.advance_for_dispatch([&zeynep]))
            .expect("server transitions must reach the configured hop limit");
        let mut message = MessageEnvelope::root(
            "message-6",
            MessageKind::Text,
            MessageOrigin::Employee(zeynep),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "go",
        )
        .with_delivery_chain(chain);
        message.dispatch_targets = vec![employee_id("cem")];
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::HopLimitReached);
    }

    #[test]
    fn exhausted_chain_wake_budget_is_a_hard_stop() {
        let prior = ["prior-a", "prior-b", "prior-c", "prior-d"].map(employee_id);
        let chain = DeliveryChain::root("message-root")
            .advance_for_dispatch(prior.iter())
            .expect("server transition must consume the configured wake budget");
        let mut message = MessageEnvelope::root(
            "message-budget-zero",
            MessageKind::Text,
            MessageOrigin::Employee(employee_id("prior-a")),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "Cem'e gönder",
        )
        .with_delivery_chain(chain);
        message.dispatch_targets = vec![employee_id("cem")];
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::WakeBudgetExhausted);
    }

    #[test]
    fn remaining_chain_budget_caps_deterministic_and_semantic_fanout() {
        let prior = ["prior-a", "prior-b", "prior-c"].map(employee_id);
        let chain = DeliveryChain::root("message-root")
            .advance_for_dispatch(prior.iter())
            .expect("server transition must leave one wake in the budget");

        let mut deterministic = MessageEnvelope::root(
            "message-budget-deterministic",
            MessageKind::Text,
            MessageOrigin::Employee(employee_id("prior-a")),
            ConversationContext::Channel {
                channel_id: "office".to_owned(),
            },
            "İki açık hedef",
        )
        .with_delivery_chain(chain.clone());
        deterministic.dispatch_targets = vec![employee_id("cem"), employee_id("zeynep")];
        let deterministic_router = default_router();
        let deterministic_decision =
            final_decision(&deterministic_router, &deterministic, &fixtures());
        assert_eq!(deterministic_decision.wake_count(), 1);
        assert!(deterministic_decision
            .recipients
            .iter()
            .any(|recipient| { recipient.reason == RoutingReason::RecipientLimitReached }));

        let semantic = MessageEnvelope::human_channel(
            "message-budget-semantic",
            "sefa",
            "office",
            "Genel ama ilgili bir konu",
        )
        .with_delivery_chain(chain);
        let semantic_router = default_router();
        let scorer = StaticScorer::new([("cem", 0.95), ("zeynep", 0.94)]);
        let semantic_decision =
            semantic_decision(&semantic_router, &semantic, &fixtures(), &scorer);
        assert_eq!(semantic_decision.wake_count(), 1);
        assert!(semantic_decision
            .recipients
            .iter()
            .any(|recipient| { recipient.reason == RoutingReason::RecipientLimitReached }));
    }

    #[test]
    fn semantic_recipient_cap_uses_score_then_stable_id_order() {
        let message = MessageEnvelope::human_channel(
            "message-7",
            "sefa",
            "office",
            "İkinizi de ilgilendirebilir",
        );
        let policy = RoutingPolicy {
            max_recipients: 1,
            ..RoutingPolicy::default()
        };
        let router = Router::new(policy).expect("policy must be valid");
        let scorer = StaticScorer::new([("cem", 0.91), ("zeynep", 0.93)]);

        let decision = semantic_decision(&router, &message, &fixtures(), &scorer);

        assert_eq!(
            decision.woken_employee_ids().collect::<Vec<_>>(),
            vec![&employee_id("zeynep")]
        );
        let cem = decision
            .recipients
            .iter()
            .find(|recipient| recipient.employee_id == employee_id("cem"))
            .expect("Cem must have an explainable candidate row");
        assert_eq!(cem.action, RecipientAction::Drop);
        assert_eq!(cem.reason, RoutingReason::RecipientLimitReached);
    }

    #[test]
    fn invalid_scorer_output_fails_closed() {
        let message = MessageEnvelope::human_channel(
            "message-8",
            "sefa",
            "office",
            "Genel bir şirket konusu",
        );
        let router = default_router();
        let scorer = StaticScorer::new([("cem", 1.2)]);

        let decision = semantic_decision(&router, &message, &fixtures(), &scorer);

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(
            decision.summary_reason,
            RoutingReason::SemanticScorerUnavailable
        );
        assert_eq!(decision.wake_count(), 0);
    }

    #[test]
    fn reaction_events_never_create_work() {
        let mut message = MessageEnvelope::human_channel("message-9", "sefa", "office", "👍");
        message.kind = MessageKind::Reaction;
        message.structured_mentions = vec![employee_id("cem")];
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::NonRoutableMessage);
    }

    #[test]
    fn inactive_explicit_target_is_explained_and_not_woken() {
        let mut employees = fixtures().employees().cloned().collect::<Vec<_>>();
        employees
            .iter_mut()
            .find(|employee| employee.id == employee_id("cem"))
            .expect("Cem fixture must exist")
            .status = EmployeeStatus::Paused;
        let catalog = EmployeeCatalog::new(employees).expect("catalog must remain valid");
        let message = MessageEnvelope::human_channel(
            "message-10",
            "sefa",
            "office",
            "Cem, buna bakar mısın?",
        );
        let router = default_router();

        let decision = final_decision(&router, &message, &catalog);

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::EmployeeInactive);
        assert_eq!(decision.wake_count(), 0);
        assert_eq!(
            decision.recipients[0].reason,
            RoutingReason::EmployeeInactive
        );
    }

    #[test]
    fn adoption_fixtures_cannot_wake_before_provisioning_activation() {
        let cem: EmployeeManifest =
            serde_yaml::from_str(include_str!("../../../config/employees/cem.yaml"))
                .expect("Cem fixture must deserialize");
        let zeynep: EmployeeManifest =
            serde_yaml::from_str(include_str!("../../../config/employees/zeynep.yaml"))
                .expect("Zeynep fixture must deserialize");
        let catalog = EmployeeCatalog::new([cem.employee, zeynep.employee])
            .expect("draft adoption fixtures must be valid definitions");
        let message = MessageEnvelope::human_channel(
            "message-adoption-gate",
            "sefa",
            "office",
            "Cem, buna bakar mısın?",
        );
        let router = default_router();

        let decision = final_decision(&router, &message, &catalog);

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::EmployeeInactive);
        assert_eq!(decision.wake_count(), 0);
    }

    #[test]
    fn email_like_text_does_not_count_as_an_explicit_mention() {
        let message = MessageEnvelope::human_channel(
            "message-11",
            "sefa",
            "office",
            "Lütfen foo@cem.com adresini kaydet",
        );
        let router = default_router();
        let scorer = StaticScorer::new([("cem", 0.1), ("zeynep", 0.1)]);

        let decision = semantic_decision(&router, &message, &fixtures(), &scorer);

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_ne!(decision.summary_reason, RoutingReason::ExplicitAlias);
        assert_eq!(decision.wake_count(), 0);
    }

    #[test]
    fn oversized_message_is_rejected_before_semantic_scoring() {
        let policy = RoutingPolicy {
            max_message_bytes: 8,
            ..RoutingPolicy::default()
        };
        let router = Router::new(policy).expect("policy must be valid");
        let message =
            MessageEnvelope::human_channel("message-12", "sefa", "office", "dokuz-byte-dan-uzun");

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::MessageTooLarge);
    }

    #[test]
    fn invalid_message_is_rejected_before_target_resolution_or_scoring() {
        let mut message =
            MessageEnvelope::human_channel("message-invalid", "sefa", "office", "not blank");
        message.body.clear();
        message.structured_mentions = vec![employee_id("cem")];
        let router = default_router();

        let decision = final_decision(&router, &message, &fixtures());

        assert_eq!(decision.mode, RoutingMode::Silent);
        assert_eq!(decision.summary_reason, RoutingReason::InvalidMessage);
        assert!(decision.recipients.is_empty());
    }
}
