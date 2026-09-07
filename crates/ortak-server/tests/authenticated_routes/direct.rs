use super::*;
use futures_util::StreamExt;
use std::time::Duration;

use crate::direct_channel_support as support;

async fn selected(f: &Fixture) -> (Uuid, Router) {
    let key = Keys::generate().public_key().to_bytes();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES($1,'cem',$2,'adopt',$3,'credential://synthetic/private-dm',clock_timestamp())")
        .bind(f.company).bind(f.revision).bind(key.as_slice()).execute(&f.pool).await.unwrap();
    let channel = support::create(
        &f.pool,
        f.community,
        &[f.operator.public_key().to_bytes(), key],
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    support::select(
        &f.control,
        &scope,
        channel,
        &EmployeeId::parse("cem").unwrap(),
    )
    .await;
    let mut cfg = config(f.community, &f.operator, channel);
    let mut outsider = cfg.humans[0].clone();
    outsider.public_key = f.reader.public_key().to_hex();
    assert!(outsider.role == Role::Operator);
    cfg.humans.push(outsider);
    (
        channel,
        product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_activity_is_participant_only_and_archived_recovery_remains_visible() {
    let f = Fixture::new().await;
    let (channel, app) = selected(&f).await;
    let run = f.run(channel).await;
    for endpoint in [
        format!("/api/v1/runs/{run}"),
        format!("/api/v1/runs/{run}/events"),
    ] {
        assert_eq!(
            response(&app, signed(&f.operator, "GET", &endpoint, "", false))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            response(&app, signed(&f.reader, "GET", &endpoint, "", false))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let (_, list) = response(&app, signed(&f.reader, "GET", "/api/v1/runs", "", false)).await;
    assert_eq!(list["runs"], json!([]));
    let other = Fixture::new().await;
    assert_eq!(
        response(
            &other.app,
            signed(
                &other.operator,
                "GET",
                &format!("/api/v1/runs/{run}"),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query(
        "UPDATE channels SET archived_at=clock_timestamp() WHERE community_id=$1 AND id=$2",
    )
    .bind(f.community)
    .bind(channel)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        response(
            &app,
            signed(
                &f.operator,
                "GET",
                &format!("/api/v1/runs/{run}"),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        response(
            &app,
            signed(
                &f.operator,
                "POST",
                &format!("/api/v1/runs/{run}/cancel"),
                "{}",
                true
            )
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    assert_eq!(
        response(
            &app,
            signed(
                &f.operator,
                "GET",
                &format!("/api/v1/runs/{run}"),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_activity_stream_revokes_current_participant_and_closes() {
    let f = Fixture::new().await;
    let (channel, app) = selected(&f).await;
    let run = f.run(channel).await;
    let path = format!("/api/v1/runs/{run}/stream");
    assert_eq!(
        response(&app, signed(&f.reader, "GET", &path, "", false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let response = app
        .clone()
        .oneshot(signed(&f.operator, "GET", &path, "", false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(first.to_vec())
        .unwrap()
        .contains("event: activity"));
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    let mut revoked = false;
    for _ in 0..4 {
        let frame = tokio::time::timeout(Duration::from_secs(6), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = String::from_utf8(frame.to_vec()).unwrap();
        if text.contains("\"code\":\"revoked\"") {
            revoked = true;
            assert!(!text.contains("entries"));
            break;
        }
        assert!(
            text.contains("event: activity"),
            "only a preceding authorized snapshot may be buffered"
        );
    }
    assert!(revoked);
    assert!(tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .is_none());
}
