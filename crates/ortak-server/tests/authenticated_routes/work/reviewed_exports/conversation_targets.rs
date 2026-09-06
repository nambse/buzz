//! Production advertisement and75 target guards; no remote I/O or fabricated use.
use super::*;

async fn state(x: &ExportFixture) -> (bool, bool, i64, bool, i64, Option<Uuid>) {
    sqlx::query_as(
        "SELECT enabled,runtime_consumption_enabled,consumption_epoch,
        conversation_consumption_enabled,conversation_consumption_epoch,conversation_channel_id
        FROM reviewed_memory_targets WHERE company_id=$1 AND project_id=$2 AND employee_id=$3",
    )
    .bind(x.scope.company_id())
    .bind(x.project)
    .bind(x.target.employee_id.as_str())
    .fetch_one(&x.f.pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable port55432 PostgreSQL migrated through75"]
async fn conversation_target_refresh_and_opt_out_keep_independent_monotonic_epochs() {
    let x = ExportFixture::new(Duration::from_secs(86400), false).await;
    let mut target = x.target.clone();
    target.runtime_consumption_enabled = true;
    let targets = [target];
    let selections = [ReviewedConversationTarget {
        project_id: x.project,
        employee_id: x.target.employee_id.clone(),
        channel_id: x.f.channel,
    }];
    for _ in 0..2 {
        assert_eq!(
            exports::advertise_targets_with_conversations(
                &x.f.control,
                &x.scope,
                &targets,
                &selections,
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(state(&x).await, (true, true, 0, true, 0, Some(x.f.channel)));
    }
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM conversation_memory_authorities WHERE company_id=$1 AND project_id=$2")
        .bind(x.scope.company_id()).bind(x.project).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(scopes, 1);
    assert_eq!(
        x.counts().await,
        (0, 0, 0),
        "advertisement must not publish a fact"
    );

    exports::advertise_targets(&x.f.control, &x.scope, &targets)
        .await
        .unwrap();
    assert_eq!(
        state(&x).await,
        (true, true, 0, false, 1, Some(x.f.channel))
    );
    exports::advertise_targets_with_conversations(&x.f.control, &x.scope, &targets, &selections)
        .await
        .unwrap();
    assert_eq!(state(&x).await, (true, true, 0, true, 1, Some(x.f.channel)));

    let mut foreign = selections[0].clone();
    foreign.channel_id = x.f.hidden;
    assert!(
        exports::advertise_targets_with_conversations(&x.f.control, &x.scope, &targets, &[foreign])
            .await
            .is_err()
    );
    assert_eq!(
        state(&x).await,
        (true, true, 0, true, 1, Some(x.f.channel)),
        "wrong channel rolls back the complete advertisement"
    );

    exports::advertise_targets(&x.f.control, &x.scope, &[])
        .await
        .unwrap();
    assert_eq!(
        state(&x).await,
        (false, false, 1, false, 2, Some(x.f.channel))
    );
}
