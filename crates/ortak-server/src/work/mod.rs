//! Manual Work operations over the atomic, project-authorized core facade.
mod dto;
mod projection;
mod routes;

pub(crate) use routes::router;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    routes::ApiState,
    Role,
};
use ortak_work::{ApiWorkPrincipal, AuthorizedWork};

fn authorized(state: &ApiState, principal: &Principal) -> Result<AuthorizedWork> {
    let event = principal
        .auth_event_id
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::unavailable())?;
    let actor = ApiWorkPrincipal::new(
        state.config.community_id,
        principal.public_key.to_hex(),
        event,
        principal.grant.role == Role::Operator,
        principal.grant.can_create_projects,
        principal.grant.channel_ids.iter().copied().collect(),
        principal.grant.employee_ids.iter().cloned().collect(),
    )?;
    Ok(AuthorizedWork::new(
        state.control.clone(),
        principal.scope.clone(),
        actor,
    ))
}
