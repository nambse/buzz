//! Root-run Rust/SQL production resolver parity tests. Requires a disposable
//! database migrated through 74 with docs/ortak/sql/conversation_source75.sql
//! installed, explicitly selected at localhost:55432. Never runs a migrator or
//! installs the SQL fragment. This does not require an applied migration 75.
//! The epochs submodule additionally requires storage75 and epochs75 installed.
//! Every fixture and fault lives in a transaction which is rolled back.

use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;

mod epochs;
mod fixture;
mod parity;
use fixture::Fixture;

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn canonical_source_evidence_and_explicit_audience_are_server_derived() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    let thread = f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .unwrap();
    let channel = f
        .resolve(source, ConversationAudienceKind::Channel)
        .await
        .unwrap();
    assert_eq!(
        thread.audience().thread_root(),
        Some(thread.provenance().source())
    );
    assert_eq!(channel.audience().thread_root(), None);
    assert_ne!(
        thread.audience().audience_hash().unwrap(),
        channel.audience().audience_hash().unwrap()
    );
    // Independent literal field order: changing the production preimage must
    // not silently rewrite the source identity or accept a legacy message hash.
    let expected = format!(
        "{{\"author_pubkey\":\"{}\",\"channel_id\":\"{}\",\"community_id\":\"{}\",\"company_id\":\"{}\",\"content\":\"canonical evidence\\nÖ\\n\",\"event_created_at\":\"{}\",\"event_id\":\"{}\",\"format\":\"ortak-reviewed-conversation-evidence/1\",\"kind\":9,\"sig\":\"{}\",\"tags\":[[\"h\",\"{}\"]]}}",
        hex::encode(f.human), f.channel, f.community, f.scope.company_id(),
        f.time(0).format("%Y-%m-%dT%H:%M:%S%.6fZ"), source.to_hex(), "09".repeat(64), f.channel
    );
    let digest: [u8; 32] = Sha256::digest(expected.as_bytes()).into();
    assert_eq!(
        thread.provenance().source_evidence_hash().as_bytes(),
        &digest
    );
    assert_eq!(
        thread.provenance().source_evidence_hash(),
        channel.provenance().source_evidence_hash()
    );
    assert!(!format!("{thread:?}").contains("canonical evidence"));
    assert!(thread
        .valid_before()
        .is_some_and(|t| t > thread.observed_at()));
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn exact_nested_ancestry_has_one_audience_and_distinct_source_provenance() {
    let mut f = Fixture::new().await;
    let root = f.event(0, None).await;
    let child = f.event(1, Some((root, 0, root, 0, 1))).await;
    let grandchild = f.event(2, Some((child, 1, root, 0, 2))).await;
    let a = f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .unwrap();
    let b = f
        .resolve(grandchild, ConversationAudienceKind::Thread)
        .await
        .unwrap();
    assert_eq!(a.audience(), b.audience());
    assert_eq!(a.audience().thread_root().unwrap().event_id(), root);
    assert_eq!(a.audience().thread_root().unwrap().created_at(), f.time(0));
    assert_ne!(a.provenance().source_hash(), b.provenance().source_hash());
    // Routing delivery-chain state is neither required nor authoritative.
    sqlx::query("INSERT INTO delivery_chains(company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes) VALUES($1,$2,'synthetic-v1','sha256:'||repeat('01',32),1,1)")
        .bind(f.scope.company_id()).bind(grandchild.as_bytes().as_slice()).execute(&mut *f.tx).await.unwrap();
    assert_eq!(
        b.audience(),
        f.resolve(grandchild, ConversationAudienceKind::Thread)
            .await
            .unwrap()
            .audience()
    );
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn source_partition_and_decided_plaintext_agreement_refuse_each_mismatch() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    let faults = [
        "UPDATE office_inbox SET event_created_at=event_created_at+interval '1 microsecond' WHERE company_id=$1",
        "UPDATE office_inbox SET author_pubkey=decode(repeat('07',32),'hex') WHERE company_id=$1",
        "UPDATE office_inbox SET event_kind=40002 WHERE company_id=$1",
        "UPDATE office_inbox SET channel_id=gen_random_uuid() WHERE company_id=$1",
        "UPDATE office_inbox SET state='dropped' WHERE company_id=$1",
        "UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$2",
        "UPDATE events SET content=repeat('x',65537) WHERE community_id=$2",
        "UPDATE events SET tags=jsonb_build_array(jsonb_build_array('p',repeat('x',16385))) WHERE community_id=$2",
        "UPDATE events SET tags='{}'::jsonb WHERE community_id=$2",
        "UPDATE events SET kind=1059 WHERE community_id=$2",
    ];
    for fault in faults {
        f.fault(fault).await;
        assert!(
            f.resolve(source, ConversationAudienceKind::Thread)
                .await
                .is_none(),
            "source fault must refuse: {fault}"
        );
        f.restore().await;
    }
    assert!(f
        .resolve(source, ConversationAudienceKind::Thread)
        .await
        .is_some());
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn current_read_scope_and_both_readers_refuse_revocation_and_reclassification() {
    let mut f = Fixture::new().await;
    let source = f.event(0, None).await;
    let faults = [
        "UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$2 AND pubkey=decode(repeat('03',32),'hex')",
        "UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$2 AND pubkey=decode(repeat('04',32),'hex')",
        "UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1",
        "UPDATE users SET agent_type='synthetic-agent' WHERE community_id=$2 AND pubkey=decode(repeat('03',32),'hex')",
        "UPDATE users SET deactivated_at=clock_timestamp() WHERE community_id=$2",
        "UPDATE channels SET ttl_deadline=clock_timestamp()-interval '1 second' WHERE community_id=$2",
        "UPDATE channels SET archived_at=clock_timestamp() WHERE community_id=$2",
        "UPDATE channels SET channel_type='dm',visibility='private' WHERE community_id=$2",
        "UPDATE employees SET status='paused' WHERE company_id=$1",
        "UPDATE employee_office_bindings SET valid_until=clock_timestamp()-interval '1 second',valid_from=clock_timestamp()-interval '1 hour' WHERE company_id=$1",
        "UPDATE projects SET status='archived',archived_at=clock_timestamp(),version=version+1 WHERE company_id=$1",
    ];
    for fault in faults {
        f.fault(fault).await;
        assert!(
            f.resolve(source, ConversationAudienceKind::Channel)
                .await
                .is_none(),
            "scope fault must refuse: {fault}"
        );
        f.restore().await;
    }
    assert!(f
        .resolve_with(source, &[], std::slice::from_ref(&f.employee.clone()))
        .await
        .is_none());
    assert!(f.resolve_with(source, &[f.channel], &[]).await.is_none());
    let other = CompanyScope::new(Uuid::new_v4(), Some(f.community));
    let employee = f.employee.clone();
    let human = f.human;
    let channels = f.channels.clone();
    let employees = f.employees.clone();
    let request = ConversationReadRequest {
        scope: &other,
        project_id: f.project,
        employee_id: &employee,
        human_public_key: &human,
        channel_grants: &channels,
        employee_grants: &employees,
        source_message_id: source,
        audience_kind: ConversationAudienceKind::Thread,
    };
    assert!(resolve_conversation_on(&mut f.tx, &request)
        .await
        .unwrap()
        .is_none());
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn thread_metadata_never_repairs_missing_or_conflicting_canonical_references() {
    let mut f = Fixture::new().await;
    let root = f.event(0, None).await;
    let child = f.event(1, Some((root, 0, root, 0, 1))).await;
    let faults = [
        "UPDATE thread_metadata SET parent_event_created_at=parent_event_created_at+interval '1 microsecond' WHERE community_id=$2",
        "UPDATE thread_metadata SET root_event_created_at=root_event_created_at+interval '1 microsecond' WHERE community_id=$2",
        "UPDATE thread_metadata SET root_event_created_at=NULL WHERE community_id=$2",
        "UPDATE thread_metadata SET depth=2 WHERE community_id=$2",
        "UPDATE thread_metadata SET parent_event_id=event_id,parent_event_created_at=event_created_at WHERE community_id=$2",
        "DELETE FROM thread_metadata WHERE community_id=$2",
        "UPDATE events SET tags='[]'::jsonb WHERE community_id=$2",
        "UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$2 AND content='canonical evidence'||chr(10)||'Ö'||chr(10)",
    ];
    for fault in faults {
        f.fault(fault).await;
        for kind in [
            ConversationAudienceKind::Thread,
            ConversationAudienceKind::Channel,
        ] {
            assert!(
                f.resolve(child, kind).await.is_none(),
                "no fallback on invalid ancestry: {fault}"
            );
        }
        f.restore().await;
    }
    // Root stub must be complete and parentless. A forged root cannot carry a
    // hidden unresolved reference even when all descendant metadata agrees.
    sqlx::query("UPDATE events SET tags=jsonb_build_array(jsonb_build_array('e',$3,'','root')) WHERE community_id=$2 AND id=$4 AND $1::uuid IS NOT NULL")
        .bind(f.scope.company_id()).bind(f.community).bind(child.to_hex()).bind(root.as_bytes().as_slice())
        .execute(&mut *f.tx).await.unwrap();
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn ancestry_allows_32_edges_but_refuses_33_without_time_order_guessing() {
    let mut f = Fixture::new().await;
    let root = f.event(0, None).await;
    let mut parent = root;
    for depth in 1..=33 {
        // Fixture timestamps descend with depth: client clocks do not prove ancestry.
        let child = f
            .event(depth, Some((parent, depth - 1, root, 0, depth as i32)))
            .await;
        let result = f.resolve(child, ConversationAudienceKind::Thread).await;
        if depth <= 32 {
            assert_eq!(
                result.unwrap().audience().thread_root().unwrap().event_id(),
                root
            );
        } else {
            assert!(result.is_none());
        }
        parent = child;
    }
    f.tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 74 plus conversation_source75.sql"]
async fn top_level_metadata_and_parent_channel_use_exact_canonical_identity() {
    let mut f = Fixture::new().await;
    let root = f.event(0, None).await;
    sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id) VALUES($1,$2,$3,$4)")
        .bind(f.community).bind(root.as_bytes().as_slice()).bind(f.time(0)).bind(f.channel)
        .execute(&mut *f.tx).await.unwrap();
    assert!(f
        .resolve(root, ConversationAudienceKind::Thread)
        .await
        .is_some());
    // An explicit self-root is equivalent only with its exact partition; both
    // valid parentless encodings are accepted, neither can name another root.
    sqlx::query("UPDATE thread_metadata SET root_event_id=event_id,root_event_created_at=event_created_at WHERE community_id=$1")
        .bind(f.community).execute(&mut *f.tx).await.unwrap();
    let child = f.event(1, Some((root, 0, root, 0, 1))).await;
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_some());
    f.fault("UPDATE thread_metadata SET root_event_created_at=root_event_created_at+interval '1 microsecond' WHERE community_id=$2 AND depth=0").await;
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.restore().await;
    let other = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels(community_id,id,name,created_by) VALUES($1,$2,'other-source',$3)",
    )
    .bind(f.community)
    .bind(other)
    .bind(f.human.as_slice())
    .execute(&mut *f.tx)
    .await
    .unwrap();
    sqlx::query("SAVEPOINT conversation_fault")
        .execute(&mut *f.tx)
        .await
        .unwrap();
    sqlx::query("UPDATE thread_metadata SET channel_id=$2 WHERE community_id=$1 AND depth=0")
        .bind(f.community)
        .bind(other)
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.restore().await;
    sqlx::query("UPDATE events SET channel_id=$3 WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(root.as_bytes().as_slice())
        .bind(other)
        .execute(&mut *f.tx)
        .await
        .unwrap();
    assert!(f
        .resolve(child, ConversationAudienceKind::Thread)
        .await
        .is_none());
    f.tx.rollback().await.unwrap();
}
