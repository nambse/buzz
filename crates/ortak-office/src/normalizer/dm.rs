//! Narrow canonical DM admission; outer gift wraps never reach this module.

use super::*;

pub(super) async fn conversation(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    channel: Uuid,
    author: &[u8; 32],
    eligible: &std::collections::BTreeSet<EmployeeId>,
) -> Result<Option<ConversationContext>> {
    let Some(direct) = ortak_control::postgres::direct_channel_on(
        connection,
        scope.company_id(),
        scope.community_id(),
        channel,
    )
    .await?
    else {
        return Ok(None);
    };
    if !direct.permits_execution()
        || !eligible.contains(&direct.employee_id)
        || (author != &direct.human_public_key && author != &direct.employee_public_key)
    {
        return Ok(None);
    }
    Ok(Some(ConversationContext::Direct {
        conversation_id: channel.to_string(),
        employee_participants: if author == &direct.human_public_key {
            vec![direct.employee_id]
        } else {
            Vec::new()
        },
    }))
}
