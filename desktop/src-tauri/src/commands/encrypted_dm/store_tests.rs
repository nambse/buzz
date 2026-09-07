use super::*;

#[cfg(unix)]
#[tokio::test]
async fn durable_freeze_lost_ack_and_draft_ciphertext_use_production_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path()).unwrap();
    let human = Keys::generate();
    let employee = Keys::generate();
    let plaintext = "synthetic private draft sentinel";
    let v = store
        .save_draft("base", "epoch-a", &human, 0, plaintext)
        .unwrap();
    let frozen = codec::freeze(&human, &employee.public_key().to_hex(), plaintext)
        .await
        .unwrap();
    store
        .freeze("base", "epoch-a", "operation", v, &frozen)
        .unwrap();
    store.ack("base", "operation", 0).unwrap();
    drop(store);
    let store = Store::at(dir.path()).unwrap();
    let retained = store.pending("base").unwrap().unwrap();
    assert_eq!(retained.frozen.outer_json, frozen.outer_json);
    assert_eq!(retained.pending.acknowledged, [true, false]);
    assert!(store
        .freeze("base", "epoch-a", "different", v, &frozen)
        .is_err());
    assert!(store
        .connection()
        .unwrap()
        .execute("UPDATE sends SET frozen='{}'", [])
        .is_err());
    assert!(store
        .connection()
        .unwrap()
        .execute("UPDATE sends SET ack0=0", [])
        .is_err());
    assert!(store
        .connection()
        .unwrap()
        .execute("DELETE FROM sends", [])
        .is_err());
    assert!(store
        .save_draft("base", "epoch-a", &human, 0, "stale overwrite")
        .is_err());
    assert_eq!(
        store.draft("base", "epoch-a", &human).unwrap().text,
        plaintext
    );
    assert_eq!(store.draft("base", "epoch-b", &human).unwrap().text, "");
    assert!(store.draft("base", "epoch-a", &employee).is_err());
    store.ack("base", "operation", 1).unwrap();
    assert!(store.pending("base").unwrap().is_none());
    assert_eq!(store.draft("base", "epoch-a", &human).unwrap().text, "");
    assert_eq!(store.draft("base", "epoch-a", &human).unwrap().version, 0);
    for item in fs::read_dir(dir.path().join("ortak-encrypted-dm-v1")).unwrap() {
        let bytes = fs::read(item.unwrap().path()).unwrap();
        assert!(!bytes
            .windows(plaintext.len())
            .any(|v| v == plaintext.as_bytes()));
    }
}

#[cfg(unix)]
#[test]
fn app_data_parent_alias_preserves_protected_store_checks() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let aliases = tempfile::tempdir().unwrap();
    let alias = aliases.path().join("app-data");
    symlink(dir.path(), &alias).unwrap();
    let store = Store::at(&alias).unwrap();
    assert_eq!(
        store.0,
        dir.path()
            .canonicalize()
            .unwrap()
            .join("ortak-encrypted-dm-v1/ciphertext.sqlite")
    );
    drop(store);
    let database = dir.path().join("ortak-encrypted-dm-v1/ciphertext.sqlite");
    let moved = dir.path().join("moved.sqlite");
    fs::rename(&database, &moved).unwrap();
    symlink(&moved, &database).unwrap();
    assert!(Store::at(&alias).is_err());
}

#[cfg(unix)]
#[test]
fn protected_store_refuses_symlink_and_hardlinked_database() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    symlink(elsewhere.path(), dir.path().join("ortak-encrypted-dm-v1")).unwrap();
    assert!(Store::at(dir.path()).is_err());
    fs::remove_file(dir.path().join("ortak-encrypted-dm-v1")).unwrap();
    let store = Store::at(dir.path()).unwrap();
    fs::hard_link(&store.0, elsewhere.path().join("linked.sqlite")).unwrap();
    assert!(Store::at(dir.path()).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn legacy_store_upgrade_retirement_preserves_bytes_and_replay_cannot_clear_fresh_draft() {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let human = Keys::generate();
    let employee = Keys::generate();
    let source_dir = tempfile::tempdir().unwrap();
    let source = Store::at(source_dir.path()).unwrap();
    let version = source
        .save_draft("base", "old-scope", &human, 0, "old protected text")
        .unwrap();
    let frozen = codec::freeze(
        &human,
        &employee.public_key().to_hex(),
        "old protected text",
    )
    .await
    .unwrap();
    source
        .freeze("base", "old-scope", "old-operation", version, &frozen)
        .unwrap();
    source.ack("base", "old-operation", 0).unwrap();
    // Copy only production-generated ciphertext into the exact original schema,
    // whose real deployed version was zero. No alternate draft/wrap serializer.
    let source_connection = source.connection().unwrap();
    let draft: String = source_connection
        .query_row("SELECT ciphertext FROM drafts", [], |r| r.get(0))
        .unwrap();
    let raw: String = source_connection
        .query_row("SELECT frozen FROM sends", [], |r| r.get(0))
        .unwrap();
    let legacy_dir = tempfile::tempdir().unwrap();
    let root = legacy_dir.path().join("ortak-encrypted-dm-v1");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("ciphertext.sqlite");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    let legacy = Connection::open(&path).unwrap();
    legacy.execute_batch(schema::LEGACY).unwrap();
    legacy
        .execute(
            "INSERT INTO drafts VALUES('base','old-scope',?1,?2)",
            params![version, draft],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO sends VALUES('old-operation','base','old-scope',?1,?2,1,0)",
            params![version, raw],
        )
        .unwrap();
    assert_eq!(
        legacy
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(legacy);

    let store = Store::at(legacy_dir.path()).unwrap();
    assert_eq!(
        store
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let old = store.pending("base").unwrap().unwrap();
    assert_eq!(old.pending.acknowledged, [true, false]);
    assert_eq!(old.frozen.outer_json, frozen.outer_json);
    assert_eq!(old.pending.retired_at, None);
    assert!(old.require_sendable("new-scope").is_err());
    assert!(store
        .retire("other-base", "old-scope", "old-operation")
        .is_err());
    assert!(store
        .retire("base", "wrong-scope", "old-operation")
        .is_err());
    let receipt = store.retire("base", "old-scope", "old-operation").unwrap();
    assert!(receipt.retired_at.is_some());
    assert_eq!(receipt.acknowledged, [true, false]);
    assert!(store.pending("base").unwrap().is_none());
    assert_eq!(store.draft("base", "new-scope", &human).unwrap().version, 0);
    assert_eq!(store.draft("base", "new-scope", &human).unwrap().text, "");
    let connection = store.connection().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT frozen FROM sends", [], |r| r.get::<_, String>(0))
            .unwrap(),
        raw
    );
    for sql in [
        "UPDATE sends SET retired=0,retired_at=NULL",
        "UPDATE sends SET retired_at=retired_at+1",
        "UPDATE sends SET ack1=1",
        "UPDATE sends SET scope='new-scope'",
        "UPDATE sends SET frozen='{}'",
        "DELETE FROM sends",
    ] {
        assert!(connection.execute(sql, []).is_err());
    }
    assert!(store.ack("base", "old-operation", 1).is_err());
    for scope in ["old-scope", "new-scope"] {
        assert!(store
            .entry("base", "old-operation")
            .unwrap()
            .unwrap()
            .require_sendable(scope)
            .is_err());
    }
    let next = store
        .save_draft("base", "new-scope", &human, 0, "fresh protected text")
        .unwrap();
    let replay = store.retire("base", "old-scope", "old-operation").unwrap();
    assert_eq!(replay.retired_at, receipt.retired_at);
    assert_eq!(
        store.draft("base", "new-scope", &human).unwrap().text,
        "fresh protected text"
    );
    let fresh = codec::freeze(
        &human,
        &employee.public_key().to_hex(),
        "fresh protected text",
    )
    .await
    .unwrap();
    assert!(store
        .freeze("base", "new-scope", "old-operation", next, &fresh)
        .is_err());
    store
        .freeze("base", "new-scope", "new-operation", next, &fresh)
        .unwrap();
    drop(store);
    let reopened = Store::at(legacy_dir.path()).unwrap();
    assert_eq!(
        reopened
            .pending("base")
            .unwrap()
            .unwrap()
            .pending
            .operation_id,
        "new-operation"
    );
    let retained = reopened.retired("base").unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].operation_id, "old-operation");
    assert_eq!(retained[0].acknowledged, [true, false]);
    assert_eq!(retained[0].retired_at, receipt.retired_at);
    assert_eq!(
        reopened
            .entry("base", "old-operation")
            .unwrap()
            .unwrap()
            .frozen
            .outer_json,
        frozen.outer_json
    );
}
