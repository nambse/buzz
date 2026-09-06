//! Production epoch triggers on a valid registered scope, without fabricated
//! approval/fact rows. Requires source75, storage75 and epochs75 installed by
//! the root operator (or their assembled migration). No DDL/migrator runs here.
//! All mutations and local deletion-executor settings roll back with Fixture.

use super::*;

async fn register(f: &mut Fixture, source: MessageId) {
    assert!(f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .is_some());
    let epoch: i64 =
        sqlx::query_scalar("SELECT ortak_register_conversation_authority($1,$2,$3,$4)")
            .bind(f.scope.company_id())
            .bind(f.community)
            .bind(f.project)
            .bind(f.channel)
            .fetch_one(&mut *f.tx)
            .await
            .unwrap();
    assert_eq!(epoch, 0);
}

async fn authority(f: &mut Fixture) -> (i64, String) {
    sqlx::query_as("SELECT epoch,last_change_reason FROM conversation_memory_authorities WHERE company_id=$1 AND project_id=$2 AND channel_id=$3")
        .bind(f.scope.company_id()).bind(f.project).bind(f.channel)
        .fetch_one(&mut *f.tx).await.unwrap()
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with source75, storage75 and epochs75 installed"]
async fn source_edit_and_removal_restore_cannot_restore_the_old_epoch() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    let original = f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .unwrap();
    register(&mut f, source).await;

    sqlx::query("UPDATE events SET content='edited canonical evidence' WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community).bind(source.as_bytes().as_slice()).bind(f.time(0))
        .execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await, (1, "event_changed".into()));
    assert_ne!(
        original.provenance().source_hash(),
        f.resolve(source, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .provenance()
            .source_hash()
    );
    sqlx::query("UPDATE events SET content=$4 WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community)
        .bind(source.as_bytes().as_slice())
        .bind(f.time(0))
        .bind("canonical evidence\nÖ\n")
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert_eq!(authority(&mut f).await.0, 2);
    assert_eq!(
        original.provenance().source_hash(),
        f.resolve(source, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .provenance()
            .source_hash()
    );

    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community).bind(source.as_bytes().as_slice()).bind(f.time(0))
        .execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await.0, 3);
    assert!(f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .is_none());
    sqlx::query(
        "UPDATE events SET deleted_at=NULL WHERE community_id=$1 AND id=$2 AND created_at=$3",
    )
    .bind(f.community)
    .bind(source.as_bytes().as_slice())
    .bind(f.time(0))
    .execute(&mut *f.tx)
    .await
    .unwrap();
    assert_eq!(authority(&mut f).await.0, 4);
    assert_eq!(
        original.provenance().source_hash(),
        f.resolve(source, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .provenance()
            .source_hash()
    );
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with source75, storage75 and epochs75 installed"]
async fn ordinary_native_reply_is_neutral_but_changed_ancestry_retires_the_scope() {
    let mut f = Fixture::new().await;
    let root = f.event(0, None).await;
    register(&mut f, root).await;
    let reply = MessageId::from_bytes(Sha256::digest(Uuid::new_v4().as_bytes()).into());
    // Native publishes only a direct reply reference, not a manufactured root
    // tag. Insert all normal canonical rows; no trigger is disabled or bypassed.
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,'new unrelated reply',$6,$7)")
        .bind(f.community).bind(reply.as_bytes().as_slice()).bind(f.human.as_slice()).bind(f.time(1))
        .bind(json!([["h", f.channel.to_string()], ["e", root.to_hex(), "", "reply"]]))
        .bind([9u8;64].as_slice()).bind(f.channel).execute(&mut *f.tx).await.unwrap();
    sqlx::query("INSERT INTO office_inbox(company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id,state,finalized_at) VALUES($1,$2,$3,9,$4,$5,'decided',clock_timestamp())")
        .bind(f.scope.company_id()).bind(reply.as_bytes().as_slice()).bind(f.time(1))
        .bind(f.human.as_slice()).bind(f.channel).execute(&mut *f.tx).await.unwrap();
    sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,$5,$6,$5,$6,1)")
        .bind(f.community).bind(reply.as_bytes().as_slice()).bind(f.time(1)).bind(f.channel)
        .bind(root.as_bytes().as_slice()).bind(f.time(0)).execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await, (0, "registered".into()));
    assert_eq!(
        f.resolve(reply, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .audience()
            .thread_root()
            .unwrap()
            .event_id(),
        root
    );

    // Parentless alone is insufficient: this non-neutral INSERT changes an
    // already referenced canonical ancestor and must retire the old epoch.
    sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,depth) VALUES($1,$2,$3,$4,1)")
        .bind(f.community).bind(root.as_bytes().as_slice()).bind(f.time(0)).bind(f.channel)
        .execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await, (1, "thread_changed".into()));
    assert!(f
        .resolve(reply, ConversationAudienceKind::Thread)
        .await
        .is_none());
    sqlx::query("UPDATE thread_metadata SET depth=0 WHERE community_id=$1 AND event_id=$2 AND event_created_at=$3")
        .bind(f.community).bind(root.as_bytes().as_slice()).bind(f.time(0))
        .execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await.0, 2);
    assert!(f
        .resolve(reply, ConversationAudienceKind::Thread)
        .await
        .is_some());
    sqlx::query(
        "DELETE FROM thread_metadata WHERE community_id=$1 AND event_id=$2 AND event_created_at=$3",
    )
    .bind(f.community)
    .bind(root.as_bytes().as_slice())
    .bind(f.time(0))
    .execute(&mut *f.tx)
    .await
    .unwrap();
    assert_eq!(authority(&mut f).await.0, 3);
    // A genuinely neutral root stub is equivalent to absent metadata, while
    // the earlier invalidation remains monotonic through this restoration.
    sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id) VALUES($1,$2,$3,$4)")
        .bind(f.community).bind(root.as_bytes().as_slice()).bind(f.time(0)).bind(f.channel)
        .execute(&mut *f.tx).await.unwrap();
    assert_eq!(authority(&mut f).await.0, 3);
    assert!(f
        .resolve(reply, ConversationAudienceKind::Thread)
        .await
        .is_some());
    f.tx.rollback().await.unwrap();
}

async fn revision(f: &mut Fixture, number: i64, model: &str, user_peer: &str) -> Uuid {
    let selected = Uuid::new_v4();
    let manifest = json!({
        "office": {"public_key": hex::encode([4u8;32]), "signer_ref": "secret://synthetic/conversation"},
        "runtime": {"model": model},
        "memory": {"adapter":"fixture-memory", "endpoint_ref":"fixture://memory", "workspace":"conversation", "user_peer":user_peer, "employee_peer":"employee", "options":{}}
    });
    let fingerprint: [u8; 32] = Sha256::digest(serde_json::to_vec(&manifest).unwrap()).into();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES($1,$2,$3,$4,$5,$6,'adopt')")
        .bind(f.scope.company_id()).bind(selected).bind(f.employee.as_str()).bind(number)
        .bind(manifest).bind(fingerprint.as_slice()).execute(&mut *f.tx).await.unwrap();
    sqlx::query("INSERT INTO employee_memory_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,endpoint_ref,workspace,user_peer,employee_peer,validated_at) VALUES($1,$2,$3,'fixture-memory','adopt','fixture://memory','conversation',$4,'employee',clock_timestamp())")
        .bind(f.scope.company_id()).bind(selected).bind(f.employee.as_str()).bind(user_peer)
        .execute(&mut *f.tx).await.unwrap();
    selected
}

async fn select_revision(f: &mut Fixture, revision: Uuid) {
    sqlx::query("UPDATE employees SET active_revision_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company_id())
        .bind(f.employee.as_str())
        .bind(revision)
        .execute(&mut *f.tx)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with source75, storage75 and epochs75 installed"]
async fn model_only_revision_preserves_epoch_but_memory_identity_change_advances() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    let baseline = revision(&mut f, 2, "model-before", "human").await;
    select_revision(&mut f, baseline).await;
    register(&mut f, source).await;
    let before = f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .unwrap();

    let model_only = revision(&mut f, 3, "model-after", "human").await;
    assert_eq!(
        authority(&mut f).await.0,
        0,
        "inactive binding preparation is neutral"
    );
    select_revision(&mut f, model_only).await;
    assert_eq!(authority(&mut f).await, (0, "registered".into()));
    let selected: Uuid = sqlx::query_scalar(
        "SELECT active_revision_id FROM employees WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company_id())
    .bind(f.employee.as_str())
    .fetch_one(&mut *f.tx)
    .await
    .unwrap();
    assert_eq!(selected, model_only);
    assert_eq!(
        before.audience(),
        f.resolve(source, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .audience()
    );

    let new_memory = revision(&mut f, 4, "model-after", "different-human-peer").await;
    assert_eq!(authority(&mut f).await.0, 0);
    select_revision(&mut f, new_memory).await;
    assert_eq!(authority(&mut f).await, (1, "identity_changed".into()));
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with source75, storage75 and epochs75 installed"]
async fn first_community_close_retires_once_and_closed_history_needs_no_old_lease() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    register(&mut f, source).await;
    // Use the existing exact tombstone transition credentials, never disable
    // a trigger. This is a transaction-local trigger test, not a claim that the
    // full deletion coordinator/approval workflow was exercised.
    sqlx::query("SELECT set_config('buzz.deletion_executor_community',$1,true),set_config('buzz.deletion_fence_generation',deletion_fence_generation::text,true) FROM communities WHERE id=$2")
        .bind(f.community.to_string()).bind(f.community).execute(&mut *f.tx).await.unwrap();
    sqlx::query("UPDATE communities SET deletion_state='quiescing' WHERE id=$1")
        .bind(f.community)
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert_eq!(authority(&mut f).await, (1, "scope_closed".into()));
    assert!(f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .is_none());
    sqlx::query("UPDATE communities SET deletion_state='fenced' WHERE id=$1")
        .bind(f.community)
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert_eq!(authority(&mut f).await, (1, "scope_closed".into()));
    sqlx::query("SELECT set_config('buzz.deletion_executor_community','',true),set_config('buzz.deletion_fence_generation','',true)")
        .execute(&mut *f.tx).await.unwrap();
    // Company-owned identity can still change without an old community's
    // deletion lease. Its retained closed scope must not be written again.
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company_id())
        .bind(f.employee.as_str())
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert_eq!(authority(&mut f).await, (1, "scope_closed".into()));
    assert!(f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.tx.rollback().await.unwrap();
}
