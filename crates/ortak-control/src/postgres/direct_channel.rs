//! Canonical server-readable one-to-one DM authority, without content or keys.

use ortak_domain::EmployeeId;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::Result;

/// The exact retained participant pair of a private human/employee DM.
/// This is a read snapshot: callers must hold the Office fence or carry its
/// generation witness when using it to authorize admission or a response.
#[derive(Clone, Debug)]
pub struct DirectChannel {
    /// Canonical human participant; never inferred from message tags.
    pub human_public_key: [u8; 32],
    /// The employee's retained Office identity in this conversation.
    pub employee_public_key: [u8; 32],
    /// Durable company employee identity, independent of current lifecycle.
    pub employee_id: EmployeeId,
    /// Whether the human remains a current channel participant.
    pub human_present: bool,
    /// Whether the employee remains a current channel participant.
    pub employee_present: bool,
    /// Whether the channel currently admits execution (not archived/expired).
    pub channel_live: bool,
    /// Deactivated Office keys cannot admit execution, even with a live member row.
    pub employee_identity_live: bool,
}

impl DirectChannel {
    /// Both participants must remain present before routing or identity health.
    pub fn permits_execution(&self) -> bool {
        self.channel_live
            && self.human_present
            && self.employee_present
            && self.employee_identity_live
    }

    /// Historical Activity remains recoverable by its current human participant,
    /// including when its employee is disabled or its channel is archived.
    pub fn visible_to(&self, public_key: &[u8; 32]) -> bool {
        self.human_present && public_key == &self.human_public_key
    }
}

/// Resolves only a canonical private DM with exactly two retained membership
/// keys, one company employee and one non-automated human. Removed rows count
/// toward the fingerprint, so adding/replacing a participant cannot widen it.
/// No signed message content, secret, runtime, or provider is accessed.
pub async fn direct_channel_on(
    connection: &mut PgConnection,
    company_id: Uuid,
    community_id: Option<Uuid>,
    channel_id: Uuid,
) -> Result<Option<DirectChannel>> {
    let channel = sqlx::query(
        "SELECT c.community_id,c.participant_hash,
                c.archived_at IS NULL AND (c.ttl_deadline IS NULL OR c.ttl_deadline>clock_timestamp()) AS live
         FROM office_company_bindings b JOIN companies co ON co.id=b.company_id AND co.status='active'
         JOIN communities cm ON cm.id=b.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
         JOIN channels c ON c.community_id=b.community_id AND c.id=$2
         WHERE b.company_id=$1 AND ($3::uuid IS NULL OR b.community_id=$3)
           AND c.channel_type='dm' AND c.visibility='private' AND c.deleted_at IS NULL",
    ).bind(company_id).bind(channel_id).bind(community_id)
        .fetch_optional(&mut *connection).await?;
    let Some(channel) = channel else {
        return Ok(None);
    };
    let community: Uuid = channel.try_get("community_id")?;
    let expected: Option<Vec<u8>> = channel.try_get("participant_hash")?;
    let members = sqlx::query(
        "SELECT m.pubkey,m.removed_at IS NULL AS present,b.employee_id,
          NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=m.community_id AND u.pubkey=m.pubkey
            AND u.deactivated_at IS NOT NULL) AS identity_live,
          EXISTS(SELECT 1 FROM users u WHERE u.community_id=m.community_id AND u.pubkey=m.pubkey
            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
          OR EXISTS(SELECT 1 FROM channel_members bot WHERE bot.community_id=m.community_id
            AND bot.pubkey=m.pubkey AND bot.role='bot') AS human_refused
         FROM channel_members m LEFT JOIN employee_office_bindings b
           ON b.company_id=$1 AND b.public_key=m.pubkey
         WHERE m.community_id=$2 AND m.channel_id=$3 ORDER BY m.pubkey LIMIT 3",
    ).bind(company_id).bind(community).bind(channel_id)
        .fetch_all(connection).await?;
    if members.len() != 2 {
        return Ok(None);
    }
    let mut fingerprint = Sha256::new();
    let mut human = None;
    let mut employee = None;
    for member in members {
        let key: Vec<u8> = member.try_get("pubkey")?;
        let Ok(key) = <[u8; 32]>::try_from(key.as_slice()) else {
            return Ok(None);
        };
        fingerprint.update(key);
        let present: bool = member.try_get("present")?;
        if let Some(id) = member.try_get::<Option<String>, _>("employee_id")? {
            if employee.is_some() {
                return Ok(None);
            }
            employee = Some((
                key,
                EmployeeId::parse(id)?,
                present,
                member.try_get::<bool, _>("identity_live")?,
            ));
        } else {
            if human.is_some() || member.try_get::<bool, _>("human_refused")? {
                return Ok(None);
            }
            human = Some((key, present));
        }
    }
    if expected.as_deref() != Some(fingerprint.finalize().as_slice()) {
        return Ok(None);
    }
    let (
        Some((human_public_key, human_present)),
        Some((employee_public_key, employee_id, employee_present, employee_identity_live)),
    ) = (human, employee)
    else {
        return Ok(None);
    };
    Ok(Some(DirectChannel {
        human_public_key,
        employee_public_key,
        employee_id,
        human_present,
        employee_present,
        channel_live: channel.try_get("live")?,
        employee_identity_live,
    }))
}
