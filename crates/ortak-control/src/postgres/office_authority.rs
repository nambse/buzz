//! Shared-fence reads used before routing inputs and at authoritative commit.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};

use crate::office_authority::OfficeAuthority;
use crate::{CompanyScope, Result};

/// Acquires the coordinated Office fence for this transaction and reads a
/// witness with the database clock. Call before taking inbox/run/root locks.
///
/// The caller must use READ COMMITTED and retain the same transaction until
/// its protected work commits. No network operation belongs in that interval.
pub async fn lock_office_authority_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
) -> Result<OfficeAuthority> {
    // Separate statements are essential: the second snapshot must be taken
    // after a waiting advisory-lock acquisition, not before it.
    let generation: i64 = sqlx::query_scalar("SELECT ortak_lock_office_authority($1)")
        .bind(scope.company_id())
        .fetch_one(&mut *connection)
        .await?;
    let valid_before: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT min(boundary) FROM employee_office_bindings b
         CROSS JOIN LATERAL (VALUES (b.valid_from), (b.valid_until)) AS dates(boundary)
         WHERE b.company_id = $1 AND boundary > clock_timestamp()",
    )
    .bind(scope.company_id())
    .fetch_one(&mut *connection)
    .await?;
    Ok(OfficeAuthority::new(
        scope.company_id(),
        generation,
        valid_before,
    ))
}

/// Checks a carried witness under the same shared fence, including expiry at
/// the database clock. A false result requires new inputs, never a patched
/// generation on the stale proposal.
pub async fn office_authority_matches_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    witness: &OfficeAuthority,
) -> Result<bool> {
    if witness.company_id() != scope.company_id() {
        return Ok(false);
    }
    let current = lock_office_authority_on(connection, scope).await?;
    let unexpired: bool =
        sqlx::query("SELECT $1::timestamptz IS NULL OR clock_timestamp() < $1 AS live")
            .bind(witness.valid_before())
            .fetch_one(&mut *connection)
            .await?
            .try_get("live")?;
    Ok(current.generation() == witness.generation() && unexpired)
}
