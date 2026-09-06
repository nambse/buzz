//! Private DM routing uses the same canonical pair and current Office fence.
use super::*;
#[path = "../../../../ortak-control/tests/direct_channel_support.rs"]
mod support;

async fn selected(f: &Fixture) -> (Uuid, ApiConfig, Router) {
    let key = Keys::generate().public_key().to_bytes();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES($1,'cem',$2,'adopt',$3,'credential://synthetic/routing-private-dm',clock_timestamp())")
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
    cfg.humans[0]
        .employee_ids
        .push(EmployeeId::parse("hidden-private").unwrap());
    let mut outsider = cfg.humans[0].clone();
    outsider.public_key = f.reader.public_key().to_hex();
    cfg.humans.push(outsider);
    let app = product_router(f.control.clone(), cfg.clone(), Arc::new(Replay::default())).unwrap();
    (channel, cfg, app)
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with private DM authority migration"]
async fn routing_read_private_dm_scopes_participant_counterpart_and_retained_recovery() {
    let f = Fixture::new().await;
    let (channel, cfg, app) = selected(&f).await;
    let pending = source(&f, channel).await;
    let absent = read(&app, &f.operator, channel, pending).await;
    assert_eq!(absent.0, StatusCode::OK, "{absent:?}");
    assert!(absent.1["decision"].is_null());
    let message = record(&f, channel, true).await;
    let result = read(&app, &f.operator, channel, message).await;
    assert_eq!(result.0, StatusCode::OK, "{result:?}");
    assert_eq!(
        result.1["decision"]["recipients"].as_array().unwrap().len(),
        1
    );
    assert_eq!(result.1["decision"]["recipients"][0]["employee_id"], "cem");
    assert!(
        !result.1.to_string().contains("hidden-private"),
        "a different granted employee is outside this DM"
    );
    assert_eq!(
        read(&app, &f.reader, channel, message).await.0,
        StatusCode::NOT_FOUND
    );
    for employee_grant in [false, true] {
        let mut restricted = cfg.clone();
        if employee_grant {
            restricted.humans[0].channel_ids = vec![f.channel];
        } else {
            restricted.humans[0].employee_ids = vec![EmployeeId::parse("hidden-private").unwrap()];
        }
        let restricted =
            product_router(f.control.clone(), restricted, Arc::new(Replay::default())).unwrap();
        assert_eq!(
            read(&restricted, &f.operator, channel, message).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let foreign = Fixture::new().await;
    assert_eq!(
        read(&foreign.app, &foreign.operator, channel, message)
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
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        read(&app, &f.operator, channel, message).await.0,
        StatusCode::OK,
        "retained Activity stays recoverable by the current human"
    );
    sqlx::query("UPDATE events SET kind=1059 WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(message.to_bytes().as_slice())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        read(&app, &f.operator, channel, message).await.0,
        StatusCode::NOT_FOUND,
        "gift-wrap bytes are never a supported routing read source"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with private DM authority migration"]
async fn routing_read_private_dm_revocation_waits_for_office_fence() {
    let f = Fixture::new().await;
    let (channel, _, app) = selected(&f).await;
    let message = record(&f, channel, false).await;
    assert_eq!(
        read(&app, &f.operator, channel, message).await.0,
        StatusCode::OK
    );
    let mut held = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&mut *held).await.unwrap();
    let request = signed(&f.operator, "GET", &path(channel, message), "", false);
    let task = tokio::spawn(async move { response(&app, request).await });
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(
        !task.is_finished(),
        "private DM read escaped the Office authority fence"
    );
    held.commit().await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.0, StatusCode::NOT_FOUND, "{result:?}");
    assert_eq!(result.1, json!({"error":{"code":"not_found"}}));
}
