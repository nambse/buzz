use super::*;
use sha2::{Digest, Sha256};
impl AuthorizedWork {
    pub(super) fn queue_context(&self, employee: &EmployeeId) -> Result<String> {
        let value = serde_json::to_vec(&(
            "employee-work/v1/active-assignments-active-projects-nonterminal/newest",
            self.scope.company_id(),
            self.principal.community_id,
            &self.principal.public_key,
            self.principal.operator,
            &self.principal.channel_ids,
            &self.principal.employee_ids,
            employee,
        ))
        .map_err(|_| WorkError::InvalidQuery("invalid queue context"))?;
        Ok(hex::encode(Sha256::digest(value)))
    }
}
pub(super) fn decode(value: Option<&str>, context: &str) -> Result<Option<WorkListCursor>> {
    value
        .map(|value| {
            if value.len() > 200 {
                return Err(WorkError::InvalidQuery("invalid queue cursor"));
            }
            let (bound, position) = value
                .split_once('/')
                .ok_or(WorkError::InvalidQuery("invalid queue cursor"))?;
            if bound != context {
                return Err(WorkError::InvalidQuery(
                    "queue cursor belongs to another audience",
                ));
            }
            WorkListCursor::decode(position)
        })
        .transpose()
}
pub(super) fn encode(context: &str, summary: &WorkSummary) -> String {
    format!("{context}/{}", WorkListCursor::after(summary).encode())
}
