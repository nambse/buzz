use super::*;
use chrono::Utc;
use ortak_domain::{Employee, EmployeeCatalog, EmployeeManifest, EmployeeStatus, RoutingPolicy};
use ortak_router::{Router, RoutingPreparation};

use crate::inbox::{InboxEvent, InboxRow};
use crate::office_authority::OfficeAuthority;
use crate::ports::RosterEmployee;
use crate::routing::EmployeeRecord;
use crate::ClaimGeneration;

pub(crate) struct Fixture {
    pub scope: CompanyScope,
    pub claim: InboxClaim,
    pub snapshot: RoutingSnapshot,
    pub envelope: MessageEnvelope,
    pub eligible: BTreeSet<EmployeeId>,
}

impl Fixture {
    pub(crate) fn new(message: MessageId, body: &str) -> Self {
        let scope = CompanyScope::new(Uuid::new_v4(), None);
        let employees: Vec<Employee> = ["cem", "zeynep"]
            .iter()
            .map(|name| {
                let yaml = std::fs::read_to_string(format!(
                    "{}/../../config/employees/{name}.yaml",
                    env!("CARGO_MANIFEST_DIR"),
                ))
                .expect("fixture file");
                let manifest: EmployeeManifest =
                    serde_yaml::from_str(&yaml).expect("fixture manifest");
                let mut employee = manifest.employee;
                employee.status = EmployeeStatus::Active;
                employee
            })
            .collect();
        let now = Utc::now();
        let event = InboxEvent {
            event_id: message,
            event_created_at: now,
            event_kind: 9,
            author_pubkey: [3; 32],
            channel_id: Some(Uuid::new_v4()),
        };
        let expires = now + chrono::Duration::seconds(60);
        let claim = InboxClaim {
            company_id: scope.company_id(),
            message_id: message,
            claim_generation: ClaimGeneration(1),
            claimed_by: "semantic-fixture".to_owned(),
            claim_expires_at: expires,
            attempt_count: 1,
            event: event.clone(),
        };
        let inbox = InboxRow {
            event,
            state: InboxState::Claimed,
            claim_generation: ClaimGeneration(1),
            claimed_by: Some(claim.claimed_by.clone()),
            claim_expires_at: Some(expires),
            attempt_count: 1,
            retry_after: None,
            last_error: None,
            received_at: now,
            finalized_at: None,
        };
        let eligible = employees
            .iter()
            .map(|employee| employee.id.clone())
            .collect();
        let roster = employees
            .into_iter()
            .map(|employee| RosterEmployee {
                record: EmployeeRecord {
                    id: employee.id.clone(),
                    status: employee.status,
                    active_revision_id: Some(Uuid::new_v4()),
                    routing_enabled: employee.routing.enabled,
                },
                employee: Some(employee),
            })
            .collect();
        let snapshot = RoutingSnapshot {
            office_authority: OfficeAuthority::new(scope.company_id(), 1, None),
            inbox,
            policy: RoutingPolicy::default(),
            roster,
        };
        let envelope = MessageEnvelope::human_channel(message.to_hex(), "human", "office", body);
        Self {
            scope,
            claim,
            snapshot,
            envelope,
            eligible,
        }
    }

    pub(crate) fn request(&self) -> SemanticRoutingRequest {
        let router = Router::new(self.snapshot.policy.clone()).expect("router");
        let catalog = EmployeeCatalog::new(
            self.snapshot
                .roster
                .iter()
                .filter_map(|entry| entry.employee.clone()),
        )
        .expect("catalog");
        let RoutingPreparation::Semantic(request) =
            router.prepare_with_conversation_eligibility(&self.envelope, &catalog, &self.eligible)
        else {
            panic!("untargeted human message should request scoring")
        };
        request
    }

    pub(crate) fn input(&self) -> SemanticScoringInput {
        self.bind(self.request()).expect("sealed input")
    }

    fn bind(&self, request: SemanticRoutingRequest) -> Result<SemanticScoringInput> {
        SemanticScoringInput::new(
            &self.scope,
            &self.claim,
            &self.snapshot,
            &self.envelope,
            &self.eligible,
            request,
        )
    }
}

pub(crate) fn fixture_input(request: SemanticRoutingRequest) -> SemanticScoringInput {
    let fixture = Fixture::new(
        MessageId::parse_hex(request.message_id()).expect("message id"),
        request.body(),
    );
    fixture
        .bind(request)
        .expect("fixture request matches snapshot")
}

#[test]
fn input_is_company_source_policy_and_exact_revision_bound_without_debug_text() {
    let mut fixture = Fixture::new(MessageId::from_bytes([7; 32]), "sensitive message fixture");
    let input = fixture.input();
    assert_eq!(input.company_id(), fixture.scope.company_id());
    assert_eq!(input.message_id(), fixture.claim.message_id);
    assert_eq!(input.policy_version(), fixture.snapshot.policy.version);
    assert_eq!(
        input.policy_fingerprint(),
        fixture.snapshot.policy.fingerprint()
    );
    assert_eq!(input.candidates().len(), input.request().candidates().len());
    assert!(input
        .candidates()
        .windows(2)
        .all(|pair| pair[0].employee_id < pair[1].employee_id));
    assert!(!format!("{input:?}").contains("sensitive message fixture"));
    assert!(!format!("{input:?}").contains("biography"));

    fixture.snapshot.roster[0].record.active_revision_id = Some(Uuid::new_v4());
    assert_ne!(input.input_hash(), fixture.input().input_hash());
    fixture.snapshot.policy.semantic_threshold = 0.88;
    assert_ne!(input.input_hash(), fixture.input().input_hash());
    assert_ne!(
        input.policy_fingerprint(),
        fixture.input().policy_fingerprint()
    );

    let request = fixture.request();
    fixture.snapshot.roster[0].record.active_revision_id = None;
    assert!(
        fixture.bind(request).is_err(),
        "a missing revision must not be filtered away"
    );
}

#[test]
fn mismatched_company_definition_policy_or_duplicate_roster_refuses_before_scoring() {
    let mut fixture = Fixture::new(
        MessageId::from_bytes([8; 32]),
        "General question for the team",
    );
    let request = fixture.request();
    fixture.scope = CompanyScope::new(Uuid::new_v4(), None);
    assert!(fixture.bind(request).is_err());

    let mut fixture = Fixture::new(
        MessageId::from_bytes([9; 32]),
        "General question for the team",
    );
    let request = fixture.request();
    fixture.snapshot.policy.semantic_threshold = 0.9;
    assert!(fixture.bind(request).is_err());

    let mut fixture = Fixture::new(
        MessageId::from_bytes([10; 32]),
        "General question for the team",
    );
    let request = fixture.request();
    fixture.snapshot.roster[0]
        .employee
        .as_mut()
        .expect("employee")
        .biography = "Changed biography".to_owned();
    assert!(fixture.bind(request).is_err());

    let mut fixture = Fixture::new(
        MessageId::from_bytes([11; 32]),
        "General question for the team",
    );
    let request = fixture.request();
    fixture
        .snapshot
        .roster
        .push(fixture.snapshot.roster[0].clone());
    assert!(fixture.bind(request).is_err());
}

#[tokio::test]
async fn public_capture_fixture_uses_the_service_and_preserves_independent_cache_inputs() {
    let source = Fixture::new(
        MessageId::from_bytes([31; 32]),
        "A general question for the team",
    );
    let capture = crate::fakes::SemanticScoringFixture::new();
    let first = capture
        .capture(
            source.snapshot.policy.clone(),
            source.snapshot.roster.clone(),
            source.envelope.clone(),
            source.eligible.clone(),
        )
        .await
        .expect("service capture");
    let repeated = capture
        .capture(
            source.snapshot.policy.clone(),
            source.snapshot.roster.clone(),
            source.envelope.clone(),
            source.eligible.clone(),
        )
        .await
        .expect("same source");
    assert_eq!(first.company_id(), capture.scope().company_id());
    assert_eq!(first.input_hash(), repeated.input_hash());
    assert_eq!(first.candidates(), repeated.candidates());
    let mut roster = source.snapshot.roster.clone();
    roster[0].record.active_revision_id = Some(Uuid::new_v4());
    let revised = capture
        .capture(
            source.snapshot.policy.clone(),
            roster,
            source.envelope.clone(),
            source.eligible.clone(),
        )
        .await
        .expect("new revision");
    assert_eq!(first.company_id(), revised.company_id());
    assert_ne!(first.input_hash(), revised.input_hash());
    assert_ne!(first.candidates(), revised.candidates());
    let mut directed = source.envelope.clone();
    directed.structured_mentions = vec![source.snapshot.roster[0].record.id.clone()];
    assert!(
        capture
            .capture(
                source.snapshot.policy.clone(),
                source.snapshot.roster.clone(),
                directed,
                source.eligible.clone()
            )
            .await
            .is_err(),
        "deterministic routing never calls the capture scorer"
    );
}
