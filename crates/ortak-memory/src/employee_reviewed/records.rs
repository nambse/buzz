use super::*;
use chrono::{DateTime, SecondsFormat, Utc};
use ortak_control::memory::employee::{EmployeeMemoryKind, EmployeeMemoryProvenanceV1};
use serde_json::Value;

impl HonchoMemoryAdapter {
    /// Publish reviewed bytes on an inspected owned namespace. The caller proves
    /// retained registration and current sharing authority; no new diagnostic is
    /// performed or required for ordinary publication.
    pub async fn publish_reviewed_employee(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        publication: &ReviewedEmployeePublication,
    ) -> Result<ReviewedEmployeeAcknowledgement, MemoryError> {
        let body = publication_body(namespace, publication)?;
        self.bounded(async {
            self.employee_namespace_current(namespace).await?;
            let (status, value) = self
                .http
                .request_limited(
                    Method::POST,
                    &format!(
                        "{}/records/{}/publish",
                        wire::path(namespace),
                        publication.commitment.fact_id
                    ),
                    Some(body),
                    32768,
                    65536,
                )
                .await?;
            if !matches!(
                status,
                reqwest::StatusCode::OK | reqwest::StatusCode::CREATED
            ) {
                return Err(rejected("unexpected employee publication status"));
            }
            acknowledgement(
                namespace,
                &publication.commitment,
                value,
                false,
                status == reqwest::StatusCode::CREATED,
            )
        })
        .await
    }
    /// Same-key cleanup of one original owned export. No current I/O witness,
    /// employee lifecycle, source visibility or replacement binding is acquired.
    /// The returned ACK proves only the referenced remote content is absent.
    pub async fn withdraw_reviewed_employee(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        commitment: &ReviewedEmployeeCommitment,
    ) -> Result<ReviewedEmployeeAcknowledgement, MemoryError> {
        wire::commitment(commitment)?;
        self.bounded(async {
            self.employee_namespace_current(namespace).await?;
            let body = export_body(namespace, commitment, true)?;
            let (status, value) = self
                .http
                .request_limited(
                    Method::POST,
                    &format!(
                        "{}/records/{}/withdraw",
                        wire::path(namespace),
                        commitment.fact_id
                    ),
                    Some(body),
                    32768,
                    65536,
                )
                .await?;
            if status != reqwest::StatusCode::OK {
                return Err(rejected("unexpected employee withdrawal status"));
            }
            acknowledgement(namespace, commitment, value, true, false)
        })
        .await
    }
    /// Strict explicit-ID primitive, not runtime integration. The caller must
    /// resolve current requester/destination authority and recheck frozen pins.
    /// Missing remote content stays absent; no local approval text is substituted.
    pub async fn recall_selected_reviewed_employee(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        destination: Uuid,
        human: Option<&str>,
        selected: &[ReviewedEmployeeCommitment],
    ) -> Result<ReviewedEmployeeRecall, MemoryError> {
        if destination.is_nil()
            || human.is_some_and(|h| !wire::is_hash(h))
            || selected.is_empty()
            || selected.len() > 8
        {
            return Err(invalid("invalid selected employee recall"));
        }
        let mut unique = BTreeSet::new();
        for value in selected {
            wire::commitment(value)?;
            if value.destination_channel_id != destination || !unique.insert(value.fact_id) {
                return Err(invalid("employee selection audience differs"));
            }
        }
        self.bounded(async {
            self.employee_namespace_current(namespace).await?;
            let body = wire::extend(
                wire::common(namespace),
                json!({"destination_channel_id":destination,"human_public_key":human,
                "record_ids":selected.iter().map(|v|v.fact_id).collect::<Vec<_>>()}),
            )?;
            let (status, value) = self
                .http
                .request_limited(
                    Method::POST,
                    &format!("{}/recall-selected", wire::path(namespace)),
                    Some(body),
                    32768,
                    65536,
                )
                .await?;
            if status != reqwest::StatusCode::OK {
                return Err(rejected("unexpected employee recall status"));
            }
            wire::bounded_response(&value)?;
            if let Some(records) = value.get("records").and_then(Value::as_array) {
                for record in records {
                    wire::timestamps(record, &["expires_at", "tombstone_at"])?;
                }
            }
            let result: ReviewedEmployeeRecall = serde_json::from_value(value)
                .map_err(|_| rejected("invalid employee selected response"))?;
            if result.records.len() > selected.len() {
                return Err(rejected("employee recall record bound exceeded"));
            }
            let (mut next, mut total) = (0usize, 0usize);
            for record in &result.records {
                let relative = selected[next..]
                    .iter()
                    .position(|p| p.fact_id == record.record_id)
                    .ok_or_else(|| rejected("unselected or reordered employee record"))?;
                next += relative;
                record_current(namespace, &selected[next], record, true)?;
                next += 1;
                if record.status != crate::ReviewedProjectStatus::Active {
                    return Err(rejected("inactive employee recall record"));
                }
                let provenance = provenance(record)?;
                if provenance.audience().kind() == EmployeeMemoryKind::Relationship
                    && provenance
                        .audience()
                        .human_public_key()
                        .map(|h| h.to_hex())
                        .as_deref()
                        != human
                {
                    return Err(rejected("employee relationship requester differs"));
                }
                total += record.content.as_ref().map_or(0, String::len);
                if total > 8192 {
                    return Err(rejected("employee recall text bound exceeded"));
                }
            }
            Ok(result)
        })
        .await
    }
}

fn export_body(
    namespace: &ReviewedEmployeeNamespace,
    value: &ReviewedEmployeeCommitment,
    withdraw: bool,
) -> Result<Value, MemoryError> {
    wire::commitment(value)?;
    let action = if withdraw { "withdraw" } else { "publish" };
    wire::extend(
        wire::common(namespace),
        json!({"target_id":value.target_id,"destination_channel_id":value.destination_channel_id,
        "idempotency_key":format!("employee-reviewed:{action}:{}:{}",namespace.original.company_id,value.fact_id),
        "content_hash":value.content_hash,"source_hash":value.source_hash,"sharing_hash":value.sharing_hash}),
    )
}
fn publication_body(
    namespace: &ReviewedEmployeeNamespace,
    publication: &ReviewedEmployeePublication,
) -> Result<Value, MemoryError> {
    let commitment = &publication.commitment;
    let provenance = &publication.provenance;
    if !wire::text(&publication.content)
        || wire::hash(publication.content.as_bytes()) != commitment.content_hash
    {
        return Err(invalid("employee publication text differs from approval"));
    }
    provenance_matches(namespace, commitment, provenance)?;
    let bytes = provenance
        .canonical_bytes()
        .map_err(|_| invalid("invalid employee provenance"))?;
    let text = String::from_utf8(bytes).map_err(|_| invalid("invalid employee provenance UTF8"))?;
    wire::extend(
        export_body(namespace, commitment, false)?,
        json!({"content":publication.content,"provenance":text}),
    )
}
fn provenance_matches(
    namespace: &ReviewedEmployeeNamespace,
    commitment: &ReviewedEmployeeCommitment,
    value: &EmployeeMemoryProvenanceV1,
) -> Result<(), MemoryError> {
    if value.audience().company_id() != namespace.original.company_id
        || value.audience().employee_id() != &namespace.original.employee_id
        || value.audience().destination_channel_id() != commitment.destination_channel_id
        || value.source().author_public_key() != value.approval().approved_by()
        || value.approval().content_hash().to_hex() != commitment.content_hash
        || value
            .source_hash()
            .map_err(|_| invalid("invalid employee source hash"))?
            .to_hex()
            != commitment.source_hash
        || value
            .sharing_hash()
            .map_err(|_| invalid("invalid employee sharing hash"))?
            .to_hex()
            != commitment.sharing_hash
    {
        return Err(rejected(
            "employee provenance differs from selected commitment",
        ));
    }
    Ok(())
}
fn provenance(record: &ReviewedEmployeeRecord) -> Result<EmployeeMemoryProvenanceV1, MemoryError> {
    EmployeeMemoryProvenanceV1::from_canonical_bytes(
        record
            .provenance
            .as_ref()
            .ok_or_else(|| rejected("employee record provenance missing"))?
            .as_bytes(),
    )
    .map_err(|_| rejected("employee record provenance is not canonical"))
}
fn record_current(
    namespace: &ReviewedEmployeeNamespace,
    commitment: &ReviewedEmployeeCommitment,
    value: &ReviewedEmployeeRecord,
    include_text: bool,
) -> Result<(), MemoryError> {
    let r = &namespace.original;
    if value.protocol != REVIEWED_EMPLOYEE_PROTOCOL
        || value.company_id != r.company_id
        || value.employee_id != r.employee_id
        || value.deployment_id != r.deployment_id
        || value.workspace_id != r.binding.workspace
        || value.record_id != commitment.fact_id
        || value.target_id != commitment.target_id
        || value.destination_channel_id != commitment.destination_channel_id
        || value.namespace_hash != namespace.namespace_hash
        || value.binding_hash != namespace.binding_hash
        || value.content_hash != commitment.content_hash
        || value.source_hash != commitment.source_hash
        || value.sharing_hash != commitment.sharing_hash
        || value.erased_from_reviewed_store != value.tombstone_at.is_some()
        || value.provenance.is_some() != value.expires_at.is_some()
        || (value.status == crate::ReviewedProjectStatus::Withdrawn)
            != value.erased_from_reviewed_store
    {
        return Err(rejected("employee remote record identity or state differs"));
    }
    if let Some(expires) = value.expires_at {
        let p = provenance(value)?;
        provenance_matches(namespace, commitment, &p)?;
        if p.approval().expires_at() != expires {
            return Err(rejected("employee remote expiry differs from approval"));
        }
    } else if value.status != crate::ReviewedProjectStatus::Withdrawn {
        return Err(rejected("employee remote publication is missing"));
    }
    if value.status == crate::ReviewedProjectStatus::Active {
        if value.erased_from_reviewed_store
            || value.expires_at.is_none_or(|t| t <= Utc::now())
            || include_text != value.content.is_some()
        {
            return Err(rejected("employee remote active text is unavailable"));
        }
    } else if value.content.is_some() {
        return Err(rejected("employee remote ended text was disclosed"));
    }
    if let Some(content) = &value.content {
        if !wire::text(content) || wire::hash(content.as_bytes()) != commitment.content_hash {
            return Err(rejected("employee recalled content differs from approval"));
        }
    }
    Ok(())
}
fn acknowledgement(
    namespace: &ReviewedEmployeeNamespace,
    commitment: &ReviewedEmployeeCommitment,
    mut value: Value,
    withdraw: bool,
    created: bool,
) -> Result<ReviewedEmployeeAcknowledgement, MemoryError> {
    wire::bounded_response(&value)?;
    for key in ["expires_at", "tombstone_at"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            let parsed = DateTime::parse_from_rfc3339(text)
                .map_err(|_| rejected("invalid employee receipt time"))?
                .with_timezone(&Utc);
            if parsed.to_rfc3339_opts(SecondsFormat::Micros, true) != text {
                return Err(rejected("noncanonical employee receipt time"));
            }
        }
    }
    let request_hash = value
        .as_object_mut()
        .and_then(|v| v.remove("request_hash"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| rejected("employee ACK commitment missing"))?;
    let expected = wire::employee_reviewed_request_hash(
        &namespace.namespace_hash,
        &namespace.binding_hash,
        namespace.original.company_id,
        &namespace.original.employee_id,
        commitment,
        withdraw,
    )?;
    if request_hash != expected {
        return Err(rejected("employee ACK commitment differs"));
    }
    let record: ReviewedEmployeeRecord =
        serde_json::from_value(value).map_err(|_| rejected("invalid employee ACK record"))?;
    record_current(namespace, commitment, &record, false)?;
    if withdraw
        && (!record.erased_from_reviewed_store
            || record.status != crate::ReviewedProjectStatus::Withdrawn)
    {
        return Err(rejected("employee withdrawal was not proven"));
    }
    if !withdraw && record.provenance.is_none() {
        return Err(rejected("employee publication ACK lacks provenance"));
    }
    Ok(ReviewedEmployeeAcknowledgement {
        record,
        request_hash,
        created,
    })
}
