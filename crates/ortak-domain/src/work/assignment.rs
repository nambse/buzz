use super::*;

#[cfg(test)]
mod tests;

fn reason(value: String) -> Result<String, DomainError> {
    validate_reason(Some(value))?.ok_or(DomainError::InvalidField {
        field: "work.assignment.reason",
    })
}

impl WorkItem {
    /// Release an active assignment, retaining its history even if the employee
    /// is no longer eligible. This does not satisfy any human review gate.
    pub fn release_assignment(
        &mut self,
        employee_id: &EmployeeId,
        explanation: String,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        let reason = reason(explanation)?;
        let version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        let old = self
            .assignments
            .iter_mut()
            .find(|a| &a.employee_id == employee_id && a.status == AssignmentStatus::Active)
            .ok_or(DomainError::AssignmentNotActive)?;
        old.status = AssignmentStatus::Released;
        self.version = version;
        Ok(WorkEvent::AssignmentReleased {
            employee_id: employee_id.clone(),
            reason,
        })
    }

    /// Replace an active assignment or change its role in one version/event.
    /// The caller must authorize the replacement's current employee lifecycle.
    pub fn reassign(
        &mut self,
        employee_id: &EmployeeId,
        replacement: EmployeeId,
        role: AssignmentRole,
        explanation: String,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        let reason = reason(explanation)?;
        let version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        // Stage on a bounded copy: every rejected command leaves the aggregate intact.
        let mut next = self.assignments.clone();
        let old = next
            .iter_mut()
            .find(|a| &a.employee_id == employee_id && a.status == AssignmentStatus::Active)
            .ok_or(DomainError::AssignmentNotActive)?;
        if employee_id == &replacement && old.role == role {
            return Err(DomainError::DuplicateAssignment);
        }
        old.status = AssignmentStatus::Released;
        if let Some(target) = next.iter_mut().find(|a| a.employee_id == replacement) {
            if target.status == AssignmentStatus::Active {
                return Err(DomainError::DuplicateAssignment);
            }
            target.status = AssignmentStatus::Active;
            target.role = role;
        } else {
            if next.len() >= MAX_WORK_ASSIGNMENTS {
                return Err(DomainError::InvalidField {
                    field: "work.assignments",
                });
            }
            next.push(Assignment {
                employee_id: replacement.clone(),
                role,
                status: AssignmentStatus::Active,
            });
        }
        self.assignments = next;
        self.version = version;
        Ok(WorkEvent::AssignmentReassigned {
            employee_id: employee_id.clone(),
            replacement_employee_id: replacement,
            role,
            reason,
        })
    }
}
