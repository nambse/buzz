use super::*;
use chrono::Utc;
use serde_json::Value;

impl HonchoMemoryAdapter {
    /// Inspect the new family only after recovering the exact original owned
    /// resource receipt. This is read-only and does not grant I/O readiness.
    pub async fn inspect_reviewed_employee_namespace(
        &self,
        receipt: &HonchoCreatedResourcesReceipt,
    ) -> Result<ReviewedEmployeeNamespace, MemoryError> {
        self.recover_created_resources(receipt).await?;
        self.bounded(async {
            let namespace = namespace_for(receipt)?;
            self.employee_namespace_current(&namespace).await?;
            Ok(namespace)
        })
        .await
    }

    pub(super) async fn employee_namespace_current(
        &self,
        namespace: &ReviewedEmployeeNamespace,
    ) -> Result<(), MemoryError> {
        let original = &namespace.original;
        let allowed = self.allowed(Some(&original.employee_id), &original.binding)?;
        if original.company_id != self.company_id
            || original.deployment_id != self.config.deployment.deployment_id
        {
            return Err(rejected("employee namespace selection changed"));
        }
        let expected = self
            .creation_receipts
            .lock()
            .map_err(|_| unavailable("employee namespace ownership unavailable"))?
            .get(&original.employee_id)
            .cloned()
            .ok_or_else(|| rejected("employee namespace original ownership is missing"))?;
        if expected.request_hash != original.request_hash
            || expected.native_ids != original.native_ids
        {
            return Err(rejected("employee namespace original ownership changed"));
        }
        self.require_owned(allowed).await?;
        let (status, value) = self
            .http
            .request_limited(
                Method::POST,
                &format!("{}/namespace", wire::path(namespace)),
                Some(wire::common(namespace)),
                32768,
                65536,
            )
            .await?;
        if status != reqwest::StatusCode::OK {
            return Err(rejected("employee namespace protocol is unavailable"));
        }
        validate_namespace_response(namespace, &value)
    }

    /// Explicit finite synthetic I/O. Persist `request` before calling. No
    /// periodic/per-read probe or implicit approval is scheduled by this method.
    /// On uncertain completion the caller retains this intent and invokes the
    /// cleanup-only recovery method; no witness is minted without readback+ACK.
    pub async fn validate_reviewed_employee_namespace(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        request: &EmployeeNamespaceDiagnostic,
    ) -> Result<EmployeeNamespaceWitness, MemoryError> {
        wire::diagnostic(request)?;
        self.bounded(async {
            self.employee_namespace_current(namespace).await?;
            let written = wire::diagnostic_hash(namespace, request, false)?;
            let attempt = async {
                let (_, value) = self
                    .employee_diagnostic_call(namespace, request, "write")
                    .await?;
                let response = diagnostic_response(namespace, request, value)?;
                if response.challenge.is_some()
                    || response.erased
                    || response.write_request_hash.as_deref() != Some(&written)
                    || response.withdraw_request_hash.is_some()
                    || response.tombstone_at.is_some()
                {
                    return Err(rejected("employee diagnostic write is not current"));
                }
                let (_, value) = self
                    .employee_diagnostic_call(namespace, request, "read")
                    .await?;
                let response = diagnostic_response(namespace, request, value)?;
                if response.challenge.as_deref() != Some(&request.challenge)
                    || response.erased
                    || response.write_request_hash.as_deref() != Some(&written)
                    || response.withdraw_request_hash.is_some()
                    || response.tombstone_at.is_some()
                {
                    return Err(rejected("employee diagnostic exact readback failed"));
                }
                Ok::<(), MemoryError>(())
            }
            .await;
            // Even an uncertain write/read attempts the same original cleanup.
            // Outer cancellation/timeout still propagates and leaves the caller's
            // retained intent as the recovery obligation; it never reports ready.
            let cleanup = self.employee_diagnostic_cleanup(namespace, request).await?;
            attempt?;
            if cleanup.write_request_hash.as_deref() != Some(&written) {
                return Err(rejected(
                    "employee diagnostic cleanup lacks the readback write",
                ));
            }
            self.employee_namespace_current(namespace).await?;
            Ok(EmployeeNamespaceWitness {
                namespace: namespace.clone(),
                receipt: cleanup,
                adapter_instance: self.employee_namespace_instance,
                expires: Instant::now() + Duration::from_secs(55),
                validated_at: Utc::now(),
            })
        })
        .await
    }

    /// Cleanup-only replay on the original owned namespace, including after
    /// source/revision/employee revocation. It never grants a new I/O witness.
    pub async fn recover_employee_namespace_diagnostic(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        request: &EmployeeNamespaceDiagnostic,
    ) -> Result<EmployeeNamespaceDiagnosticReceipt, MemoryError> {
        wire::diagnostic(request)?;
        self.bounded(async {
            self.employee_namespace_current(namespace).await?;
            self.employee_diagnostic_cleanup(namespace, request).await
        })
        .await
    }
    async fn employee_diagnostic_cleanup(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        request: &EmployeeNamespaceDiagnostic,
    ) -> Result<EmployeeNamespaceDiagnosticReceipt, MemoryError> {
        let (_, value) = self
            .employee_diagnostic_call(namespace, request, "withdraw")
            .await?;
        let response = diagnostic_response(namespace, request, value)?;
        let expected = wire::diagnostic_hash(namespace, request, true)?;
        let expected_write = wire::diagnostic_hash(namespace, request, false)?;
        if !response.erased
            || response.challenge.is_some()
            || response.withdraw_request_hash.as_deref() != Some(&expected)
            || response
                .write_request_hash
                .as_ref()
                .is_some_and(|value| value != &expected_write)
        {
            return Err(rejected("employee diagnostic cleanup was not proven"));
        }
        Ok(EmployeeNamespaceDiagnosticReceipt {
            operation_id: request.operation_id,
            employee_revision_id: request.employee_revision_id,
            employee_lifecycle_epoch: request.employee_lifecycle_epoch,
            challenge_hash: wire::hash(request.challenge.as_bytes()),
            write_request_hash: response.write_request_hash,
            withdraw_request_hash: expected,
            erased: true,
            tombstone_at: response
                .tombstone_at
                .ok_or_else(|| rejected("employee diagnostic tombstone missing"))?,
        })
    }
    async fn employee_diagnostic_call(
        &self,
        namespace: &ReviewedEmployeeNamespace,
        request: &EmployeeNamespaceDiagnostic,
        action: &str,
    ) -> Result<(reqwest::StatusCode, Value), MemoryError> {
        let mut fields = json!({"employee_revision_id":request.employee_revision_id,"employee_lifecycle_epoch":request.employee_lifecycle_epoch});
        if action == "write" {
            fields["challenge"] = json!(request.challenge);
        } else {
            fields["challenge_hash"] = json!(wire::hash(request.challenge.as_bytes()));
        }
        let result = self
            .http
            .request_limited(
                Method::POST,
                &format!(
                    "{}/diagnostics/{}/{action}",
                    wire::path(namespace),
                    request.operation_id
                ),
                Some(wire::extend(wire::common(namespace), fields)?),
                32768,
                65536,
            )
            .await?;
        if !(result.0 == reqwest::StatusCode::OK
            || action == "write" && result.0 == reqwest::StatusCode::CREATED)
        {
            return Err(rejected("unexpected employee diagnostic status"));
        }
        Ok(result)
    }
    /// Verify this adapter minted the fresh one-time registration witness.
    /// Ordinary publication/recall does not require another diagnostic.
    pub fn employee_witness_current(
        &self,
        witness: &EmployeeNamespaceWitness,
    ) -> Result<(), MemoryError> {
        if witness.adapter_instance != self.employee_namespace_instance
            || witness.remaining().is_zero()
            || witness.namespace.original.company_id != self.company_id
            || witness.namespace.original.deployment_id != self.config.deployment.deployment_id
            || !witness.receipt.erased
        {
            return Err(rejected(
                "employee namespace current I/O evidence unavailable",
            ));
        }
        Ok(())
    }
}

pub(super) fn namespace_for(
    receipt: &HonchoCreatedResourcesReceipt,
) -> Result<ReviewedEmployeeNamespace, MemoryError> {
    let binding =
        serde_json::to_value(&receipt.binding).map_err(|_| invalid("invalid employee binding"))?;
    if receipt.company_id.is_nil()
        || receipt.deployment_id.is_nil()
        || binding["options"] != json!({})
        || receipt.binding.adapter != "honcho"
        || receipt.binding.endpoint_ref.is_empty()
        || receipt.binding.endpoint_ref.len() > 256
        || !receipt
            .binding
            .endpoint_ref
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b))
        || !receipt.native_ids.matches_binding(&receipt.binding)
        || !wire::is_hash(&receipt.request_hash)
    {
        return Err(invalid("invalid owned employee namespace"));
    }
    let bytes = crate::wire::canonical(
        &json!({"company_id":receipt.company_id,"employee_id":receipt.employee_id,"format":"ortak-reviewed-employee-namespace/1"}),
    )?;
    let namespace_hash = wire::hash(&bytes);
    let binding_hash = crate::wire::fingerprint(
        &json!({"binding":binding,"namespace_hash":namespace_hash,"protocol":REVIEWED_EMPLOYEE_PROTOCOL}),
    )?;
    Ok(ReviewedEmployeeNamespace {
        original: receipt.clone(),
        namespace: String::from_utf8(bytes)
            .map_err(|_| invalid("invalid employee namespace UTF8"))?,
        namespace_hash,
        binding_hash,
    })
}
pub(super) fn validate_namespace_response(
    namespace: &ReviewedEmployeeNamespace,
    value: &Value,
) -> Result<(), MemoryError> {
    wire::bounded_response(value)?;
    let expected = wire::extend(
        wire::common(namespace),
        json!({"protocol":REVIEWED_EMPLOYEE_PROTOCOL,"namespace":namespace.namespace,"namespace_hash":namespace.namespace_hash,"binding_hash":namespace.binding_hash}),
    )?;
    if *value != expected {
        return Err(rejected("employee namespace protocol or ownership differs"));
    }
    Ok(())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticResponse {
    operation_id: Uuid,
    employee_revision_id: Uuid,
    employee_lifecycle_epoch: i64,
    challenge_hash: String,
    write_request_hash: Option<String>,
    withdraw_request_hash: Option<String>,
    challenge: Option<String>,
    erased: bool,
    tombstone_at: Option<chrono::DateTime<Utc>>,
}
fn diagnostic_response(
    namespace: &ReviewedEmployeeNamespace,
    request: &EmployeeNamespaceDiagnostic,
    mut value: Value,
) -> Result<DiagnosticResponse, MemoryError> {
    wire::bounded_response(&value)?;
    wire::timestamps(&value, &["tombstone_at"])?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| rejected("invalid employee diagnostic response"))?;
    let mut fields = serde_json::Map::new();
    for key in [
        "operation_id",
        "employee_revision_id",
        "employee_lifecycle_epoch",
        "challenge_hash",
        "write_request_hash",
        "withdraw_request_hash",
        "challenge",
        "erased",
        "tombstone_at",
    ] {
        fields.insert(
            key.into(),
            object
                .remove(key)
                .ok_or_else(|| rejected("employee diagnostic response field missing"))?,
        );
    }
    validate_namespace_response(namespace, &value)?;
    let response: DiagnosticResponse = serde_json::from_value(Value::Object(fields))
        .map_err(|_| rejected("invalid employee diagnostic metadata"))?;
    if response.operation_id != request.operation_id
        || response.employee_revision_id != request.employee_revision_id
        || response.employee_lifecycle_epoch != request.employee_lifecycle_epoch
        || response.challenge_hash != wire::hash(request.challenge.as_bytes())
        || response.erased != response.tombstone_at.is_some()
        || response
            .write_request_hash
            .as_ref()
            .is_some_and(|s| !wire::is_hash(s))
        || response
            .withdraw_request_hash
            .as_ref()
            .is_some_and(|s| !wire::is_hash(s))
        || response
            .challenge
            .as_ref()
            .is_some_and(|s| !wire::is_hash(s))
    {
        return Err(rejected("employee diagnostic identity differs"));
    }
    Ok(response)
}
