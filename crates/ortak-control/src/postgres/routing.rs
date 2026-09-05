use std::collections::{BTreeMap, BTreeSet};

use ortak_domain::{
    Employee, EmployeeId, EmployeeRoutingPolicy, EmployeeStatus, MessageOrigin, RecipientAction,
    RecipientDecision, RoutingMode, RoutingPolicy, RoutingReason,
};
use serde::Deserialize;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::company::parse_policy;
use super::inbox::inbox_row;
use super::{bytes32, column_value, is_unique_violation, parse_column, PgControlPlane};
use crate::error::{ControlError, Result};
use crate::ids::{ClaimGeneration, CompanyScope, MessageId};
use crate::inbox::InboxState;
use crate::outbox::DispatchTicket;
use crate::ports::{RosterEmployee, RoutingRepository, RoutingSnapshot};
use crate::routing::{
    reapply_guards, revalidate_inputs, ChainCounters, ChainState, CommittedDecision,
    EmployeeRecord, RoutingCommitOutcome, RoutingProposal, StoredDecision,
};

/// Minimal manifest view read inside the transaction: only the routing
/// participation flag affects refreshed eligibility.
#[derive(Deserialize)]
struct ManifestRoutingView {
    routing: EmployeeRoutingPolicy,
}

const ROSTER_SQL: &str = "SELECT e.id, e.status, e.active_revision_id, r.manifest
       FROM employees e
       LEFT JOIN employee_revisions r
         ON r.company_id = e.company_id
        AND r.employee_id = e.id
        AND r.id = e.active_revision_id
      WHERE e.company_id = $1
      ORDER BY e.id";

fn employee_record(row: &PgRow) -> Result<(EmployeeRecord, Option<serde_json::Value>)> {
    let id: String = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    let manifest: Option<serde_json::Value> = row.try_get("manifest")?;
    let routing_enabled = match &manifest {
        Some(manifest) => serde_json::from_value::<ManifestRoutingView>(manifest.clone())
            .map(|view| view.routing.enabled)
            .map_err(|_| ControlError::UnreadableManifest {
                employee_id: id.clone(),
            })?,
        None => false,
    };
    Ok((
        EmployeeRecord {
            id: EmployeeId::parse(id)?,
            status: parse_column::<EmployeeStatus>("employees.status", &status)?,
            active_revision_id: row.try_get("active_revision_id")?,
            routing_enabled,
        },
        manifest,
    ))
}

async fn load_roster(
    connection: &mut PgConnection,
    scope: &CompanyScope,
) -> Result<BTreeMap<EmployeeId, EmployeeRecord>> {
    let rows = sqlx::query(ROSTER_SQL)
        .bind(scope.company_id())
        .fetch_all(&mut *connection)
        .await?;
    rows.iter()
        .map(|row| employee_record(row).map(|(record, _)| (record.id.clone(), record)))
        .collect()
}

fn chain_from_row(row: &PgRow, visited: BTreeSet<EmployeeId>) -> Result<ChainState> {
    let root: Vec<u8> = row.try_get("root_message_id")?;
    let max_hops: i16 = row.try_get("max_hops")?;
    let max_wakes: i32 = row.try_get("max_wakes")?;
    let hop_count: i16 = row.try_get("hop_count")?;
    let wake_count: i32 = row.try_get("wake_count")?;
    let wake_count = usize::try_from(wake_count)
        .map_err(|_| ControlError::InvalidData("negative wake_count".to_owned()))?;
    if wake_count != visited.len() {
        return Err(ControlError::InvalidData(format!(
            "delivery chain wake_count {wake_count} disagrees with {} visits",
            visited.len()
        )));
    }
    Ok(ChainState {
        root_message_id: MessageId::try_from_slice(&root)?,
        policy_version: row.try_get("policy_version")?,
        policy_fingerprint: row.try_get("policy_fingerprint")?,
        max_hops: u8::try_from(max_hops)
            .map_err(|_| ControlError::InvalidData("max_hops out of range".to_owned()))?,
        max_wakes: usize::try_from(max_wakes)
            .map_err(|_| ControlError::InvalidData("max_wakes out of range".to_owned()))?,
        hop_count: u8::try_from(hop_count)
            .map_err(|_| ControlError::InvalidData("hop_count out of range".to_owned()))?,
        wake_count,
        visited,
    })
}

async fn load_visits(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    root_message_id: MessageId,
) -> Result<BTreeSet<EmployeeId>> {
    let rows = sqlx::query(
        "SELECT employee_id FROM delivery_chain_visits
          WHERE company_id = $1 AND root_message_id = $2",
    )
    .bind(scope.company_id())
    .bind(root_message_id.as_bytes().as_slice())
    .fetch_all(&mut *connection)
    .await?;
    rows.iter()
        .map(|row| {
            let id: String = row.try_get("employee_id")?;
            Ok(EmployeeId::parse(id)?)
        })
        .collect()
}

fn origin_columns(origin: &MessageOrigin) -> (&'static str, Option<String>) {
    match origin {
        MessageOrigin::Human(id) => ("human", Some(id.clone())),
        MessageOrigin::Employee(id) => ("employee", Some(id.to_string())),
        MessageOrigin::Integration(id) => ("integration", Some(id.clone())),
        MessageOrigin::System => ("system", None),
    }
}

fn recipient_from_row(row: &PgRow) -> Result<RecipientDecision> {
    let employee_id: String = row.try_get("employee_id")?;
    let action: String = row.try_get("action")?;
    let reason: String = row.try_get("reason")?;
    let evidence: serde_json::Value = row.try_get("evidence")?;
    Ok(RecipientDecision {
        employee_id: EmployeeId::parse(employee_id)?,
        action: parse_column::<RecipientAction>("routing_recipients.action", &action)?,
        reason: parse_column::<RoutingReason>("routing_recipients.reason", &reason)?,
        score: row.try_get::<Option<f32>, _>("score")?,
        evidence: serde_json::from_value(evidence)?,
    })
}

impl RoutingRepository for PgControlPlane {
    async fn routing_snapshot(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<RoutingSnapshot>> {
        let mut connection = self.pool.acquire().await?;
        let inbox = sqlx::query(
            "SELECT event_id, event_created_at, event_kind, author_pubkey, channel_id, state,
                    claim_generation, claimed_by, claim_expires_at, attempt_count, retry_after,
                    last_error, received_at, finalized_at
               FROM office_inbox WHERE company_id = $1 AND event_id = $2",
        )
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .fetch_optional(&mut *connection)
        .await?;
        let Some(inbox) = inbox.as_ref().map(inbox_row).transpose()? else {
            return Ok(None);
        };

        let company = sqlx::query("SELECT routing_policy FROM companies WHERE id = $1")
            .bind(scope.company_id())
            .fetch_one(&mut *connection)
            .await?;
        let policy = parse_policy(company.try_get("routing_policy")?)?;

        let rows = sqlx::query(ROSTER_SQL)
            .bind(scope.company_id())
            .fetch_all(&mut *connection)
            .await?;
        let mut roster = Vec::with_capacity(rows.len());
        for row in &rows {
            let (record, manifest) = employee_record(row)?;
            let employee = match manifest {
                Some(manifest) => {
                    let mut employee: Employee =
                        serde_json::from_value(manifest).map_err(|_| {
                            ControlError::UnreadableManifest {
                                employee_id: record.id.to_string(),
                            }
                        })?;
                    if employee.id != record.id {
                        return Err(ControlError::UnreadableManifest {
                            employee_id: record.id.to_string(),
                        });
                    }
                    // The lifecycle column is authoritative over the manifest copy.
                    employee.status = record.status;
                    Some(employee)
                }
                None => None,
            };
            roster.push(RosterEmployee { record, employee });
        }

        Ok(Some(RoutingSnapshot {
            inbox,
            policy,
            roster,
        }))
    }

    async fn chain_state(
        &self,
        scope: &CompanyScope,
        root_message_id: MessageId,
    ) -> Result<Option<ChainState>> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query(
            "SELECT root_message_id, policy_version, policy_fingerprint, max_hops, max_wakes, hop_count, wake_count
               FROM delivery_chains
              WHERE company_id = $1 AND root_message_id = $2",
        )
        .bind(scope.company_id())
        .bind(root_message_id.as_bytes().as_slice())
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let visited = load_visits(&mut connection, scope, root_message_id).await?;
        chain_from_row(&row, visited).map(Some)
    }

    async fn commit_routing(
        &self,
        scope: &CompanyScope,
        proposal: &RoutingProposal,
    ) -> Result<RoutingCommitOutcome> {
        // Fail closed before any transaction or write: a proposal prepared
        // under one company can never be committed under another scope.
        if proposal.company_id != scope.company_id() {
            return Err(ControlError::InvalidProposal(
                "proposal company does not match the company scope",
            ));
        }
        if proposal.decision.message_id != proposal.message_id.to_hex() {
            return Err(ControlError::InvalidProposal(
                "decision message id does not match the proposal",
            ));
        }
        if proposal.decision.mode == RoutingMode::Semantic && proposal.scorer.is_none() {
            return Err(ControlError::InvalidProposal(
                "semantic decisions must pin scorer provenance",
            ));
        }

        let mut tx = self.pool.begin().await?;
        let company_id = scope.company_id();
        let message_bytes = proposal.message_id.as_bytes().as_slice();
        let root_bytes = proposal.root_message_id.as_bytes().as_slice();

        // 1. Fence the inbox claim under the row lock.
        let inbox = sqlx::query(
            "SELECT state, claim_generation FROM office_inbox
              WHERE company_id = $1 AND event_id = $2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(message_bytes)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(inbox) = inbox else {
            return Err(ControlError::InvalidData(format!(
                "no inbox row for message {}",
                proposal.message_id
            )));
        };
        let observed_state_raw: String = inbox.try_get("state")?;
        let observed_state = InboxState::parse(&observed_state_raw).ok_or_else(|| {
            ControlError::InvalidData(format!("office_inbox.state holds {observed_state_raw:?}"))
        })?;
        let observed_generation = ClaimGeneration(inbox.try_get("claim_generation")?);

        // 2. One dispatching decision per (company, message).
        let existing = sqlx::query(
            "SELECT id FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
        )
        .bind(company_id)
        .bind(message_bytes)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(existing) = existing {
            tx.rollback().await?;
            return Ok(RoutingCommitOutcome::AlreadyDecided {
                decision_id: existing.try_get("id")?,
            });
        }
        if observed_state != InboxState::Claimed || observed_generation != proposal.claim_generation
        {
            tx.rollback().await?;
            return Ok(RoutingCommitOutcome::StaleClaim {
                observed_state,
                observed_generation,
            });
        }

        // 3. Current company policy.
        let company = sqlx::query("SELECT status, routing_policy FROM companies WHERE id = $1")
            .bind(company_id)
            .fetch_one(&mut *tx)
            .await?;
        let company_status: String = company.try_get("status")?;
        if company_status != "active" {
            tx.rollback().await?;
            return Err(ControlError::CompanySuspended { company_id });
        }
        let policy: RoutingPolicy = parse_policy(company.try_get("routing_policy")?)?;

        // 4. Lock or create the root chain row; siblings serialize here.
        sqlx::query(
            "INSERT INTO delivery_chains
                 (company_id, root_message_id, policy_version, policy_fingerprint, max_hops, max_wakes)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (company_id, root_message_id) DO NOTHING",
        )
        .bind(company_id)
        .bind(root_bytes)
        .bind(&policy.version)
        .bind(policy.fingerprint())
        .bind(i16::from(policy.max_hops))
        .bind(i32::try_from(policy.max_chain_wakes).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
        let chain_row = sqlx::query(
            "SELECT root_message_id, policy_version, policy_fingerprint, max_hops, max_wakes, hop_count, wake_count
               FROM delivery_chains
              WHERE company_id = $1 AND root_message_id = $2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(root_bytes)
        .fetch_one(&mut *tx)
        .await?;
        let visited = load_visits(&mut tx, scope, proposal.root_message_id).await?;
        let chain = chain_from_row(&chain_row, visited)?;

        // 5. Refresh the roster and revalidate scoring inputs.
        let employees = load_roster(&mut tx, scope).await?;
        if let Some(failure) = revalidate_inputs(proposal, &policy, &employees, &chain) {
            tx.rollback().await?;
            return Ok(RoutingCommitOutcome::InputsChanged(failure));
        }

        // 6. Reapply guards against the locked chain and refreshed employees.
        let guarded = reapply_guards(
            &proposal.decision,
            &proposal.origin,
            &chain,
            policy.max_recipients,
            &employees,
            &proposal.eligible_employee_ids,
        );
        let hop_consumed = guarded.wake_count > 0;
        let next_hop = if hop_consumed {
            chain.hop_count + 1
        } else {
            chain.hop_count
        };
        let next_wakes = chain.wake_count + guarded.wake_count;

        // 7. Persist the decision.
        let (origin_type, origin_id) = origin_columns(&proposal.origin);
        let candidate_revision_ids = serde_json::Value::Array(
            proposal
                .candidates
                .iter()
                .map(|candidate| serde_json::Value::String(candidate.revision_id.to_string()))
                .collect(),
        );
        let scorer = proposal.scorer.as_ref();
        let decision_insert = sqlx::query(
            "INSERT INTO routing_decisions
                 (company_id, message_id, root_message_id, inbox_claim_generation,
                  origin_type, origin_id, mode, summary_reason,
                  policy_version, policy_fingerprint, input_hash,
                  candidate_revision_ids, excluded_targets,
                  scorer_adapter, scorer_model, scorer_prompt_version, scorer_version,
                  scorer_latency_ms, scorer_usage,
                  wake_count, hop_consumed, chain_hop_count, chain_wake_count)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                     $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
             RETURNING id",
        )
        .bind(company_id)
        .bind(message_bytes)
        .bind(root_bytes)
        .bind(proposal.claim_generation.0)
        .bind(origin_type)
        .bind(origin_id)
        .bind(column_value(&guarded.mode)?)
        .bind(column_value(&guarded.summary_reason)?)
        .bind(&proposal.decision.policy_version)
        .bind(&proposal.decision.policy_fingerprint)
        .bind(proposal.input_hash.as_slice())
        .bind(candidate_revision_ids)
        .bind(serde_json::to_value(&guarded.excluded_targets)?)
        .bind(scorer.map(|scorer| scorer.adapter.clone()))
        .bind(scorer.and_then(|scorer| scorer.model.clone()))
        .bind(scorer.and_then(|scorer| scorer.prompt_version.clone()))
        .bind(scorer.map(|scorer| scorer.version.clone()))
        .bind(scorer.and_then(|scorer| scorer.latency_ms))
        .bind(scorer.and_then(|scorer| scorer.usage.clone()))
        .bind(i32::try_from(guarded.wake_count).unwrap_or(i32::MAX))
        .bind(hop_consumed)
        .bind(i16::from(next_hop))
        .bind(i32::try_from(next_wakes).unwrap_or(i32::MAX))
        .fetch_one(&mut *tx)
        .await;
        let decision_id: Uuid = match decision_insert {
            Ok(row) => row.try_get("id")?,
            Err(error) if is_unique_violation(&error) => {
                // Backstop: the pre-check above runs under the inbox row lock,
                // so this only fires if a decision was written without one.
                tx.rollback().await?;
                let existing = sqlx::query(
                    "SELECT id FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                )
                .bind(company_id)
                .bind(message_bytes)
                .fetch_one(&self.pool)
                .await?;
                return Ok(RoutingCommitOutcome::AlreadyDecided {
                    decision_id: existing.try_get("id")?,
                });
            }
            Err(error) => return Err(error.into()),
        };

        // 8. Recipients in stable order.
        for (position, recipient) in guarded.recipients.iter().enumerate() {
            sqlx::query(
                "INSERT INTO routing_recipients
                     (company_id, routing_decision_id, employee_id, position, action, reason,
                      score, evidence, employee_revision_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(company_id)
            .bind(decision_id)
            .bind(recipient.decision.employee_id.as_str())
            .bind(i16::try_from(position).unwrap_or(i16::MAX))
            .bind(column_value(&recipient.decision.action)?)
            .bind(column_value(&recipient.decision.reason)?)
            .bind(recipient.decision.score)
            .bind(serde_json::to_value(&recipient.decision.evidence)?)
            .bind(recipient.revision_id)
            .execute(&mut *tx)
            .await?;
        }

        // 9. Unique visit reservations, counters, and dispatch outbox rows.
        let mut dispatches = Vec::with_capacity(guarded.wake_count);
        for recipient in &guarded.recipients {
            if recipient.decision.action != RecipientAction::Wake {
                continue;
            }
            let employee_id = recipient.decision.employee_id.as_str();
            let visit = sqlx::query(
                "INSERT INTO delivery_chain_visits
                     (company_id, root_message_id, employee_id, routing_decision_id,
                      recipient_action, batch_hop)
                 VALUES ($1, $2, $3, $4, 'wake', $5)",
            )
            .bind(company_id)
            .bind(root_bytes)
            .bind(employee_id)
            .bind(decision_id)
            .bind(i16::from(next_hop))
            .execute(&mut *tx)
            .await;
            if let Err(error) = visit {
                if is_unique_violation(&error) {
                    tx.rollback().await?;
                    return Err(ControlError::VisitConflict {
                        employee_id: employee_id.to_owned(),
                    });
                }
                return Err(error.into());
            }

            let dedup_key = format!("run_dispatch:{decision_id}:{employee_id}");
            let payload = serde_json::json!({
                "routing_decision_id": decision_id,
                "message_id": proposal.message_id.to_hex(),
                "root_message_id": proposal.root_message_id.to_hex(),
                "employee_id": employee_id,
                "employee_revision_id": recipient.revision_id,
                "reason": column_value(&recipient.decision.reason)?,
                "batch_hop": next_hop,
            });
            let outbox = sqlx::query(
                "INSERT INTO outbox
                     (company_id, kind, dedup_key, routing_decision_id, employee_id, payload)
                 VALUES ($1, 'run_dispatch', $2, $3, $4, $5)
                 RETURNING id",
            )
            .bind(company_id)
            .bind(&dedup_key)
            .bind(decision_id)
            .bind(employee_id)
            .bind(payload)
            .fetch_one(&mut *tx)
            .await?;
            dispatches.push(DispatchTicket {
                outbox_id: outbox.try_get("id")?,
                employee_id: employee_id.to_owned(),
                dedup_key,
            });
        }

        if hop_consumed {
            sqlx::query(
                "UPDATE delivery_chains
                    SET hop_count = $3, wake_count = $4, updated_at = now()
                  WHERE company_id = $1 AND root_message_id = $2",
            )
            .bind(company_id)
            .bind(root_bytes)
            .bind(i16::from(next_hop))
            .bind(i32::try_from(next_wakes).unwrap_or(i32::MAX))
            .execute(&mut *tx)
            .await?;
        }

        // 10. Finalize the inbox row under the same fence.
        let finalized = sqlx::query(
            "UPDATE office_inbox
                SET state = 'decided', finalized_at = now(), last_error = NULL
              WHERE company_id = $1 AND event_id = $2
                AND state = 'claimed' AND claim_generation = $3",
        )
        .bind(company_id)
        .bind(message_bytes)
        .bind(proposal.claim_generation.0)
        .execute(&mut *tx)
        .await?;
        if finalized.rows_affected() != 1 {
            tx.rollback().await?;
            return Err(ControlError::InvalidData(format!(
                "inbox row for {} changed under its row lock",
                proposal.message_id
            )));
        }

        tx.commit().await?;

        Ok(RoutingCommitOutcome::Committed(CommittedDecision {
            decision_id,
            mode: guarded.mode,
            summary_reason: guarded.summary_reason,
            recipients: guarded
                .recipients
                .iter()
                .map(|recipient| recipient.decision.clone())
                .collect(),
            excluded_targets: guarded.excluded_targets,
            wake_count: guarded.wake_count,
            hop_consumed,
            chain: ChainCounters {
                hop_count: next_hop,
                wake_count: next_wakes,
                max_hops: chain.max_hops,
                max_wakes: chain.max_wakes,
            },
            dispatches,
            refreshed: guarded.refreshed,
        }))
    }

    async fn decision_for_message(
        &self,
        scope: &CompanyScope,
        message_id: MessageId,
    ) -> Result<Option<StoredDecision>> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query(
            "SELECT id, message_id, root_message_id, inbox_claim_generation, mode, summary_reason,
                    policy_version, policy_fingerprint, input_hash, candidate_revision_ids,
                    wake_count, hop_consumed, scorer_adapter, decided_at
               FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
        )
        .bind(scope.company_id())
        .bind(message_id.as_bytes().as_slice())
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: Uuid = row.try_get("id")?;
        let recipients = sqlx::query(
            "SELECT employee_id, action, reason, score, evidence
               FROM routing_recipients
              WHERE company_id = $1 AND routing_decision_id = $2
              ORDER BY position",
        )
        .bind(scope.company_id())
        .bind(id)
        .fetch_all(&mut *connection)
        .await?;

        let message: Vec<u8> = row.try_get("message_id")?;
        let root: Vec<u8> = row.try_get("root_message_id")?;
        let input_hash: Vec<u8> = row.try_get("input_hash")?;
        let mode: String = row.try_get("mode")?;
        let summary_reason: String = row.try_get("summary_reason")?;
        let candidate_revision_ids: serde_json::Value = row.try_get("candidate_revision_ids")?;
        let wake_count: i32 = row.try_get("wake_count")?;
        Ok(Some(StoredDecision {
            id,
            message_id: MessageId::try_from_slice(&message)?,
            root_message_id: MessageId::try_from_slice(&root)?,
            inbox_claim_generation: ClaimGeneration(row.try_get("inbox_claim_generation")?),
            mode: parse_column::<RoutingMode>("routing_decisions.mode", &mode)?,
            summary_reason: parse_column::<RoutingReason>(
                "routing_decisions.summary_reason",
                &summary_reason,
            )?,
            policy_version: row.try_get("policy_version")?,
            policy_fingerprint: row.try_get("policy_fingerprint")?,
            input_hash: bytes32("routing_decisions.input_hash", &input_hash)?,
            candidate_revision_ids: serde_json::from_value(candidate_revision_ids)?,
            wake_count: usize::try_from(wake_count).unwrap_or(0),
            hop_consumed: row.try_get("hop_consumed")?,
            scorer_adapter: row.try_get("scorer_adapter")?,
            recipients: recipients
                .iter()
                .map(recipient_from_row)
                .collect::<Result<Vec<_>>>()?,
            decided_at: row.try_get("decided_at")?,
        }))
    }
}
