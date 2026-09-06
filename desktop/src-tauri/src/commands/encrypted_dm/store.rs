//! Dedicated ciphertext-only store. No ordinary archive, draft or message cache
//! receives these rows. FULL-sync transactions freeze both copies before send.

use super::{
    codec::{self, Frozen},
    Error, Result,
};
use nostr::{nips::nip44, Keys};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

pub(super) struct Store(PathBuf);

#[path = "store_schema.rs"]
mod schema;

/// Content-free status of one immutable two-copy send.
#[derive(Serialize)]
pub struct Pending {
    pub operation_id: String,
    pub scope: String,
    pub rumor_id: String,
    pub outer_ids: [String; 2],
    pub acknowledged: [bool; 2],
    pub retired_at: Option<i64>,
}
pub(super) struct Entry {
    pub pending: Pending,
    pub frozen: Frozen,
}
impl Entry {
    pub(super) fn require_sendable(&self, scope: &str) -> Result<()> {
        if self.pending.scope != scope || self.pending.retired_at.is_some() {
            return Err(Error::Revoked);
        }
        Ok(())
    }
}

/// Volatile result of opening one scope-bound protected draft.
#[derive(Serialize)]
pub struct Draft {
    pub version: u64,
    pub text: String,
}

impl Store {
    pub(super) fn at(app_data: &Path) -> Result<Self> {
        let root = app_data.join("ortak-encrypted-dm-v1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
            if !root.try_exists().map_err(|_| Error::Storage)? {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder.create(&root).map_err(|_| Error::Storage)?;
            }
            let meta = fs::symlink_metadata(&root).map_err(|_| Error::Storage)?;
            if !meta.is_dir()
                || meta.uid() != rustix::process::geteuid().as_raw()
                || meta.mode() & 0o777 != 0o700
            {
                return Err(Error::Storage);
            }
            let path = root.join("ciphertext.sqlite");
            if !path.try_exists().map_err(|_| Error::Storage)? {
                let file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&path)
                    .map_err(|_| Error::Storage)?;
                file.sync_all().map_err(|_| Error::Storage)?;
                fs::File::open(&root)
                    .and_then(|f| f.sync_all())
                    .map_err(|_| Error::Storage)?;
            }
            let meta = fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
            if !meta.is_file()
                || meta.nlink() != 1
                || meta.uid() != rustix::process::geteuid().as_raw()
                || meta.mode() & 0o777 != 0o600
                || meta.len() > 12 * 1024 * 1024
            {
                return Err(Error::Storage);
            }
            let value = Self(path);
            value.connection()?;
            Ok(value)
        }
        #[cfg(not(unix))]
        {
            let _ = root;
            Err(Error::Unavailable)
        }
    }
    fn connection(&self) -> Result<Connection> {
        let mut connection = Connection::open_with_flags(
            &self.0,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| Error::Storage)?;
        connection
            .busy_timeout(Duration::from_millis(200))
            .map_err(|_| Error::Storage)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;",
            )
            .map_err(|_| Error::Storage)?;
        schema::migrate(&mut connection)?;
        Ok(connection)
    }
    pub(super) fn pending(&self, base: &str) -> Result<Option<Entry>> {
        let connection = self.connection()?;
        connection.query_row("SELECT operation,scope,frozen,ack0,ack1,retired_at FROM sends WHERE base=?1 AND retired=0 AND (ack0=0 OR ack1=0)", [base], row).optional().map_err(|_| Error::Storage)
    }
    pub(super) fn entry(&self, base: &str, operation: &str) -> Result<Option<Entry>> {
        self.connection()?
            .query_row(
                "SELECT operation,scope,frozen,ack0,ack1,retired_at FROM sends WHERE base=?1 AND operation=?2",
                params![base, operation],
                row,
            )
            .optional()
            .map_err(|_| Error::Storage)
    }
    pub(super) fn retired(&self, base: &str) -> Result<Vec<Pending>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT operation,scope,frozen,ack0,ack1,retired_at FROM sends WHERE base=?1 AND retired=1 ORDER BY retired_at DESC,operation LIMIT 16").map_err(|_| Error::Storage)?;
        let rows = statement
            .query_map([base], row)
            .map_err(|_| Error::Storage)?;
        rows.map(|r| r.map(|entry| entry.pending).map_err(|_| Error::Storage))
            .collect()
    }
    /// Retire only this exact owned intent. Replay returns the same receipt and
    /// cannot consume a later draft. No event is sent, erased or claimed undelivered.
    pub(super) fn retire(&self, base: &str, scope: &str, operation: &str) -> Result<Pending> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| Error::Storage)?;
        let entry = tx.query_row("SELECT operation,scope,frozen,ack0,ack1,retired_at FROM sends WHERE base=?1 AND operation=?2 AND scope=?3", params![base,operation,scope], row).optional().map_err(|_| Error::Storage)?.ok_or(Error::Conflict)?;
        if entry.pending.retired_at.is_none() {
            if entry.pending.acknowledged == [true, true] {
                return Err(Error::Conflict);
            }
            let now = chrono::Utc::now().timestamp();
            if !(0..=253_402_300_799).contains(&now) {
                return Err(Error::Storage);
            }
            if tx.execute("UPDATE sends SET retired=1,retired_at=?4 WHERE base=?1 AND operation=?2 AND scope=?3 AND retired=0", params![base,operation,scope,now]).map_err(|_| Error::Storage)? != 1 {
                return Err(Error::Conflict);
            }
            tx.execute("DELETE FROM drafts WHERE base=?1 AND EXISTS(SELECT 1 FROM sends s WHERE s.operation=?2 AND s.base=drafts.base AND s.scope=drafts.scope AND s.draft_version=drafts.version AND s.retired=1)", params![base,operation]).map_err(|_| Error::Storage)?;
        }
        tx.commit().map_err(|_| Error::Storage)?;
        Ok(self.entry(base, operation)?.ok_or(Error::Storage)?.pending)
    }
    pub(super) fn freeze(
        &self,
        base: &str,
        scope: &str,
        operation: &str,
        draft_version: u64,
        value: &Frozen,
    ) -> Result<Entry> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| Error::Storage)?;
        let count: u64 = tx
            .query_row("SELECT count(*) FROM sends", [], |r| r.get(0))
            .map_err(|_| Error::Storage)?;
        if count >= 64 {
            return Err(Error::Bounds);
        }
        let raw = serde_json::to_string(value).map_err(|_| Error::Encoding)?;
        let draft: Option<u64> = tx
            .query_row(
                "SELECT version FROM drafts WHERE base=?1 AND scope=?2",
                params![base, scope],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| Error::Storage)?;
        if draft != Some(draft_version) {
            return Err(Error::Conflict);
        }
        tx.execute(
            "INSERT INTO sends(operation,base,scope,frozen,draft_version) VALUES(?1,?2,?3,?4,?5)",
            params![operation, base, scope, raw, draft_version],
        )
        .map_err(|_| Error::Conflict)?;
        tx.commit().map_err(|_| Error::Storage)?;
        self.entry(base, operation)?.ok_or(Error::Storage)
    }
    pub(super) fn ack(&self, base: &str, operation: &str, ordinal: usize) -> Result<()> {
        let query = match ordinal {
            0 => "UPDATE sends SET ack0=1 WHERE base=?1 AND operation=?2 AND retired=0",
            1 => "UPDATE sends SET ack1=1 WHERE base=?1 AND operation=?2 AND retired=0",
            _ => return Err(Error::Encoding),
        };
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| Error::Storage)?;
        if tx
            .execute(query, params![base, operation])
            .map_err(|_| Error::Storage)?
            != 1
        {
            return Err(Error::Storage);
        }
        // Completing the send consumes exactly its protected draft atomically.
        // A crash cannot reopen a delivered message as a fresh unsent draft.
        tx.execute("DELETE FROM drafts WHERE base=?1 AND EXISTS(SELECT 1 FROM sends s WHERE s.operation=?2 AND s.base=drafts.base AND s.scope=drafts.scope AND s.draft_version=drafts.version AND s.ack0=1 AND s.ack1=1)", params![base,operation]).map_err(|_| Error::Storage)?;
        tx.commit().map_err(|_| Error::Storage)
    }
    pub(super) fn draft(&self, base: &str, scope: &str, keys: &Keys) -> Result<Draft> {
        let value: Option<(u64, String, String)> = self
            .connection()?
            .query_row(
                "SELECT version,scope,ciphertext FROM drafts WHERE base=?1",
                [base],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| Error::Storage)?;
        let Some((version, stored_scope, ciphertext)) = value else {
            return Ok(Draft {
                version: 0,
                text: String::new(),
            });
        };
        if stored_scope != scope {
            return Ok(Draft {
                version,
                text: String::new(),
            });
        }
        let bytes = Zeroizing::new(
            nip44::decrypt_to_bytes(keys.secret_key(), &keys.public_key(), ciphertext)
                .map_err(|_| Error::Encoding)?,
        );
        let clear: DraftWire = serde_json::from_slice(&bytes).map_err(|_| Error::Encoding)?;
        if clear.format != "ortak-native-dm-draft/1"
            || clear.scope != scope
            || clear.base != base
            || clear.version != version
        {
            return Err(Error::Encoding);
        }
        codec::text(&clear.text.0, true)?;
        Ok(Draft {
            version,
            text: clear.text.0.to_string(),
        })
    }
    pub(super) fn save_draft(
        &self,
        base: &str,
        scope: &str,
        keys: &Keys,
        version: u64,
        text: &str,
    ) -> Result<u64> {
        codec::text(text, true)?;
        if version >= 9_007_199_254_740_991 {
            return Err(Error::Bounds);
        }
        #[derive(Serialize)]
        struct Seal<'a> {
            format: &'a str,
            base: &'a str,
            scope: &'a str,
            version: u64,
            text: &'a str,
        }
        let next = version + 1;
        let bytes = Zeroizing::new(
            serde_json::to_vec(&Seal {
                format: "ortak-native-dm-draft/1",
                base,
                scope,
                version: next,
                text,
            })
            .map_err(|_| Error::Encoding)?,
        );
        let ciphertext = nip44::encrypt(
            keys.secret_key(),
            &keys.public_key(),
            bytes.as_slice(),
            nip44::Version::V2,
        )
        .map_err(|_| Error::Encoding)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| Error::Storage)?;
        let pending: u64 = tx
            .query_row(
                "SELECT count(*) FROM sends WHERE base=?1 AND retired=0 AND (ack0=0 OR ack1=0)",
                [base],
                |r| r.get(0),
            )
            .map_err(|_| Error::Storage)?;
        if pending != 0 {
            return Err(Error::Conflict);
        }
        let current: Option<u64> = tx
            .query_row("SELECT version FROM drafts WHERE base=?1", [base], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| Error::Storage)?;
        if current.unwrap_or(0) != version {
            return Err(Error::Conflict);
        }
        if current.is_none() {
            let count: u64 = tx
                .query_row("SELECT count(*) FROM drafts", [], |r| r.get(0))
                .map_err(|_| Error::Storage)?;
            if count >= 64 {
                return Err(Error::Bounds);
            }
        }
        tx.execute("INSERT INTO drafts(base,scope,version,ciphertext) VALUES(?1,?2,?3,?4) ON CONFLICT(base) DO UPDATE SET scope=excluded.scope,version=excluded.version,ciphertext=excluded.ciphertext", params![base,scope,next,ciphertext]).map_err(|_| Error::Storage)?;
        tx.commit().map_err(|_| Error::Storage)?;
        Ok(next)
    }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let raw: String = r.get(2)?;
    let frozen: Frozen = serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(Entry {
        pending: Pending {
            operation_id: r.get(0)?,
            scope: r.get(1)?,
            rumor_id: frozen.rumor_id.clone(),
            outer_ids: frozen.outer_ids.clone(),
            acknowledged: [r.get(3)?, r.get(4)?],
            retired_at: r.get(5)?,
        },
        frozen,
    })
}
struct Secret(Zeroizing<String>);
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftWire {
    format: String,
    base: String,
    scope: String,
    version: u64,
    text: Secret,
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
