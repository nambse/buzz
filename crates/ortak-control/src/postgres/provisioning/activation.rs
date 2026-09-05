//! Short prepare and final commit checks for fresh activation authority.
use super::*;
use crate::office_authority::OfficeAuthority;

pub(super) async fn configure(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SET LOCAL lock_timeout='500ms'")
        .execute(&mut *c)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='2s'")
        .execute(c)
        .await?;
    Ok(())
}
async fn baseline(
    c: &mut PgConnection,
    scope: &CompanyScope,
    employee: &EmployeeId,
) -> Result<(EmployeeStatus, Option<Uuid>)> {
    let row=sqlx::query("SELECT e.status,e.active_revision_id FROM employees e JOIN companies co ON co.id=e.company_id WHERE e.company_id=$1 AND e.id=$2 AND co.status='active' FOR SHARE OF e")
        .bind(scope.company_id()).bind(employee.as_str()).fetch_optional(c).await?
        .ok_or_else(||ControlError::InvalidData("activation employee/company is unavailable".into()))?;
    let status: String = row.try_get("status")?;
    Ok((
        parse_column("employees.status", &status)?,
        row.try_get("active_revision_id")?,
    ))
}
async fn current(
    c: &mut PgConnection,
    scope: &CompanyScope,
    id: Uuid,
) -> Result<ProvisioningOperation> {
    // Direct step mutations also wait behind these locks. The repository's
    // normal writers already lock their parent operation first.
    sqlx::query("SELECT step_index FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 ORDER BY step_index FOR SHARE")
        .bind(scope.company_id()).bind(id).fetch_all(&mut *c).await?;
    load_operation_on(c, scope.company_id(), id)
        .await?
        .ok_or(ProvisioningError::UnknownOperation { operation_id: id }.into())
}
pub(super) async fn prepare(
    control: &PgControlPlane,
    scope: &CompanyScope,
    id: Uuid,
    running: &StepRecord,
    lifetime: std::time::Duration,
) -> Result<ActivationTarget> {
    let mut tx = control.pool.begin().await?;
    configure(&mut tx).await?;
    let office = super::super::lock_office_authority_on(&mut tx, scope).await?;
    sqlx::query("SELECT id FROM provisioning_operations WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(scope.company_id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ProvisioningError::UnknownOperation { operation_id: id })?;
    let operation = current(&mut tx, scope, id).await?;
    let (status, revision) = baseline(&mut tx, scope, &operation.employee_id).await?;
    let now = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let target = ActivationTarget::issue(
        scope, &operation, running, status, revision, office, now, lifetime,
    )?;
    tx.commit().await?;
    Ok(target)
}
pub(super) async fn validate(
    c: &mut PgConnection,
    scope: &CompanyScope,
    id: Uuid,
    activation: &RevisionActivation,
    office: &OfficeAuthority,
) -> Result<()> {
    let target = activation
        .target
        .as_ref()
        .ok_or_else(|| ControlError::InvalidData("fresh activation target is required".into()))?;
    if target.office().company_id() != scope.company_id()
        || target.office().generation() != office.generation()
    {
        return Err(ProvisioningError::Superseded {
            operation_id: id,
            detail: "activation Office authority changed",
        }
        .into());
    }
    let operation = current(&mut *c, scope, id).await?;
    let (status, revision) = baseline(&mut *c, scope, &operation.employee_id).await?;
    let now = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *c)
        .await?;
    target.validate_current(scope, &operation, status, revision, now)?;
    target.validate_activation(activation)
}
