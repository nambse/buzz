use super::*;

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL with private DM authority proposal"]
async fn private_dm_identity_health_requires_the_selected_canonical_pair_without_publication() {
    let mut f = Fixture::new().await;
    let public = OfficePublicKey::parse_hex(&f.entry().office.public_key).unwrap();
    let human = nostr::Keys::generate().public_key().to_bytes();
    let revision = Uuid::new_v4();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
        VALUES($1,$2,$3,1,'{}',$4,'adopt')")
        .bind(f.config.company_id).bind(revision).bind(f.entry().employee_id.as_str()).bind([0u8;32].as_slice())
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES($1,$2,$3,'adopt',$4,$5,clock_timestamp())")
        .bind(f.config.company_id).bind(f.entry().employee_id.as_str()).bind(revision)
        .bind(public.as_bytes().as_slice()).bind(f.entry().office.signer_ref.as_str()).execute(&f.pool).await.unwrap();
    let channel = buzz_db::dm::create_dm(
        &f.pool,
        buzz_core::CommunityId::from_uuid(f.config.community_id),
        &[human.as_slice(), public.as_bytes().as_slice()],
        human.as_slice(),
    )
    .await
    .unwrap()
    .id;
    f.config.employees[0].channels = vec![channel];
    f.config.employees[0].office.home_channel_ref = None;
    let adapter = f.adapter();
    assert!(adapter
        .membership_health(&public)
        .await
        .unwrap()
        .is_healthy());
    assert_eq!(
        f.journal_count().await,
        0,
        "health must not publish or mint a receipt"
    );
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.config.community_id).bind(channel).bind(human.as_slice()).execute(&f.pool).await.unwrap();
    assert!(adapter.membership_health(&public).await.is_err());
    assert_eq!(f.journal_count().await, 0);
}
