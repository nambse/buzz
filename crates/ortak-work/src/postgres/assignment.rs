//! Assignment changes persist one aggregate version, history event and API receipt.
use super::*;
use authorized::WorkMutation;

pub(super) async fn change_on(
    c: &mut PgConnection,
    scope: &CompanyScope,
    id: Uuid,
    version: i64,
    action: &WorkMutation,
    actor: &WorkActor,
) -> Result<WorkItemAggregate> {
    verify_actor(c, scope, actor).await?;
    let mut item = lock_item(c, scope, id, version, ProjectLock::Share).await?;
    let old = item.assignments.clone();
    let event = match action {
        WorkMutation::ReleaseAssignment {
            employee_id,
            reason,
        } => item.release_assignment(employee_id, reason.clone())?,
        WorkMutation::Reassign {
            employee_id,
            replacement_employee_id,
            role,
            reason,
        } => {
            if !employee_is_active(c, scope, replacement_employee_id).await? {
                return Err(WorkError::EmployeeNotAssignable {
                    employee_id: replacement_employee_id.clone(),
                });
            }
            item.reassign(
                employee_id,
                replacement_employee_id.clone(),
                *role,
                reason.clone(),
            )?
        }
        _ => return Err(WorkError::InvalidQuery("not an assignment change")),
    };
    for assignment in &item.assignments {
        if old.contains(assignment) {
            continue;
        }
        if assignment.status == AssignmentStatus::Released {
            let count = sqlx::query("UPDATE work_assignments SET status='released',released_at=clock_timestamp(),updated_at=clock_timestamp()
                WHERE company_id=$1 AND work_item_id=$2 AND employee_id=$3 AND status='active'")
                .bind(scope.company_id()).bind(id).bind(assignment.employee_id.as_str())
                .execute(&mut *c).await?.rows_affected();
            if count != 1 {
                return Err(invalid("assignment release row disagrees with aggregate"));
            }
        } else {
            sqlx::query("INSERT INTO work_assignments(company_id,work_item_id,employee_id,role,status,assigned_by_type,assigned_by_id)
                VALUES($1,$2,$3,$4,'active',$5,$6) ON CONFLICT(company_id,work_item_id,employee_id) DO UPDATE
                SET role=EXCLUDED.role,status='active',released_at=NULL,assigned_by_type=EXCLUDED.assigned_by_type,
                    assigned_by_id=EXCLUDED.assigned_by_id,assigned_at=clock_timestamp(),updated_at=clock_timestamp()")
                .bind(scope.company_id()).bind(id).bind(assignment.employee_id.as_str()).bind(assignment.role.as_str())
                .bind(actor.type_str()).bind(actor.id_str()).execute(&mut *c).await?;
        }
    }
    persist_event(c, scope, &item, version, actor, &event).await?;
    require_aggregate(c, scope, id).await
}
