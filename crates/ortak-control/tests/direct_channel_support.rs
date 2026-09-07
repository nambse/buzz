//! Synthetic canonical DM storage shared by production-seam tests.
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create(pool: &PgPool, community: Uuid, keys: &[[u8; 32]]) -> Uuid {
    let references = keys.iter().map(|key| key.as_slice()).collect::<Vec<_>>();
    let hash = buzz_db::dm::compute_participant_hash(&references);
    let mut tx = pool.begin().await.unwrap();
    let channel = sqlx::query_scalar(
        "INSERT INTO channels(community_id,name,created_by,channel_type,visibility,participant_hash)
         VALUES($1,'Synthetic private DM',$2,'dm','private',$3) RETURNING id",
    ).bind(community).bind(keys[0].as_slice()).bind(hash.as_slice())
        .fetch_one(&mut *tx).await.unwrap();
    for key in keys {
        sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES($1,$2,$3)")
            .bind(community)
            .bind(channel)
            .bind(key.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    channel
}

pub async fn select(
    control: &ortak_control::PgControlPlane,
    scope: &ortak_control::CompanyScope,
    channel: Uuid,
    employee: &ortak_domain::EmployeeId,
) {
    let capture = control
        .begin_routing_capture(scope, &[channel], std::slice::from_ref(employee))
        .await
        .unwrap();
    let progress = control
        .start_inbox_reconciliation(scope, capture.capture_id, channel)
        .await
        .unwrap();
    if !progress.completed {
        let final_page = control
            .reconcile_inbox_batch(scope, capture.capture_id, channel, 256)
            .await
            .unwrap();
        assert!(
            final_page.completed,
            "synthetic fixture must fit one bounded page"
        );
    }
    control
        .enable_routing_cohort(scope, capture.capture_id)
        .await
        .unwrap();
}
