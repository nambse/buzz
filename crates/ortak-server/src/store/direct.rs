//! Current participant-only Activity audience for bounded configured DM channels.
use super::*;

pub(super) async fn visible_direct_channels_on(
    connection: &mut PgConnection,
    principal: &Principal,
    community: Uuid,
) -> Result<Vec<Uuid>> {
    let channels: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM channels WHERE community_id=$1 AND id=ANY($2) AND channel_type='dm' ORDER BY id LIMIT 64",
    ).bind(community).bind(&principal.grant.channel_ids).fetch_all(&mut *connection).await?;
    let mut visible = Vec::new();
    for channel in channels {
        let direct = ortak_control::postgres::direct_channel_on(
            connection,
            principal.scope.company_id(),
            Some(community),
            channel,
        )
        .await
        .map_err(|_| ApiError::unavailable())?;
        if direct.is_some_and(|direct| {
            direct.visible_to(&principal.public_key.to_bytes())
                && principal.grant.employee_ids.contains(&direct.employee_id)
        }) {
            visible.push(channel);
        }
    }
    Ok(visible)
}
