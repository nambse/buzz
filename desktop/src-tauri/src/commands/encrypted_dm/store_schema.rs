//! Atomic upgrade of the original implicit-version0 ciphertext store. The path
//! stays unchanged so cold recovery continues preserving the same opaque bytes.

use super::{Error, Result};
use rusqlite::{Connection, TransactionBehavior};

pub(super) const LEGACY: &str = r#"
CREATE TABLE IF NOT EXISTS drafts(base TEXT PRIMARY KEY,scope TEXT NOT NULL,version INTEGER NOT NULL,ciphertext TEXT NOT NULL CHECK(length(ciphertext)<=32768));
CREATE TABLE IF NOT EXISTS sends(operation TEXT PRIMARY KEY,base TEXT NOT NULL,scope TEXT NOT NULL,draft_version INTEGER NOT NULL,frozen TEXT NOT NULL CHECK(length(frozen)<=140000),ack0 INTEGER NOT NULL DEFAULT 0 CHECK(ack0 IN(0,1)),ack1 INTEGER NOT NULL DEFAULT 0 CHECK(ack1 IN(0,1)));
CREATE UNIQUE INDEX IF NOT EXISTS one_pending ON sends(base) WHERE ack0=0 OR ack1=0;
CREATE TRIGGER IF NOT EXISTS frozen_send_immutable BEFORE UPDATE OF operation,base,scope,draft_version,frozen ON sends BEGIN SELECT RAISE(ABORT,'immutable encrypted send'); END;
CREATE TRIGGER IF NOT EXISTS send_ack_monotonic BEFORE UPDATE ON sends WHEN NEW.ack0<OLD.ack0 OR NEW.ack1<OLD.ack1 BEGIN SELECT RAISE(ABORT,'encrypted ACK cannot regress'); END;
CREATE TRIGGER IF NOT EXISTS send_retained BEFORE DELETE ON sends BEGIN SELECT RAISE(ABORT,'encrypted send retained'); END;
"#;

const RETIREMENT: &str = r#"
ALTER TABLE sends ADD COLUMN retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN(0,1));
ALTER TABLE sends ADD COLUMN retired_at INTEGER CHECK((retired=0 AND retired_at IS NULL) OR (retired=1 AND retired_at IS NOT NULL AND retired_at BETWEEN 0 AND 253402300799));
DROP INDEX one_pending;
CREATE UNIQUE INDEX one_pending ON sends(base) WHERE retired=0 AND (ack0=0 OR ack1=0);
CREATE TRIGGER send_retirement_monotonic BEFORE UPDATE ON sends
WHEN (OLD.retired=1 AND (NEW.retired<>1 OR NEW.retired_at IS NOT OLD.retired_at OR NEW.ack0<>OLD.ack0 OR NEW.ack1<>OLD.ack1))
  OR (NEW.retired=1 AND (NEW.ack0<>OLD.ack0 OR NEW.ack1<>OLD.ack1))
BEGIN SELECT RAISE(ABORT,'retired encrypted send is terminal'); END;
PRAGMA user_version=2;
"#;

fn columns(connection: &Connection, query: &str, expected: &[&str]) -> Result<()> {
    let mut statement = connection.prepare(query).map_err(|_| Error::Storage)?;
    let rows = statement
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|_| Error::Storage)?;
    let actual = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::Storage)?;
    if actual != expected {
        return Err(Error::Storage);
    }
    Ok(())
}

pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| Error::Storage)?;
    let version: u32 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| Error::Storage)?;
    if version == 0 {
        tx.execute_batch(LEGACY).map_err(|_| Error::Storage)?;
        columns(
            &tx,
            "PRAGMA table_info(sends)",
            &[
                "operation",
                "base",
                "scope",
                "draft_version",
                "frozen",
                "ack0",
                "ack1",
            ],
        )?;
        tx.execute_batch(RETIREMENT).map_err(|_| Error::Storage)?;
    } else if version != 2 {
        return Err(Error::Storage);
    }
    columns(
        &tx,
        "PRAGMA table_info(drafts)",
        &["base", "scope", "version", "ciphertext"],
    )?;
    columns(
        &tx,
        "PRAGMA table_info(sends)",
        &[
            "operation",
            "base",
            "scope",
            "draft_version",
            "frozen",
            "ack0",
            "ack1",
            "retired",
            "retired_at",
        ],
    )?;
    tx.commit().map_err(|_| Error::Storage)
}
