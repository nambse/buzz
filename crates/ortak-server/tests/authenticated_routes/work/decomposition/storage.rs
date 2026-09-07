use super::*;
#[path = "retention.rs"]
mod retention;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_failed_receipt_rolls_back_both_items_and_direct_attach_or_mutation_is_refused(
) {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let parent = item(&f, &app, project).await;
    let existing = item(&f, &app, project).await;
    let before = snapshot(&f).await;
    let request = body(&parent);
    let op = request["operation_id"].as_str().unwrap();
    let name = format!("decomposition_failure_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN(NEW.operation_id='{op}'::uuid) EXECUTE FUNCTION {name}();"))).execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path(&parent), &request).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(snapshot(&f).await, before);
    assert!(sqlx::query("INSERT INTO work_decomposition(company_id,project_id,parent_id,child_id,parent_version,depth,actor_pubkey,operation_id) VALUES($1,$2,$3,$4,2,1,$5,$6)")
        .bind(f.company).bind(project).bind(id(&parent)).bind(id(&existing)).bind(f.operator.public_key().to_hex()).bind(Uuid::new_v4())
        .execute(&f.pool).await.is_err(),"an existing proposed item is not a fresh child");
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO work_decomposition(company_id,project_id,parent_id,child_id,parent_version,depth,actor_pubkey,operation_id) VALUES($1,$2,$3,$4,2,1,$5,$6)")
        .bind(f.company).bind(project).bind(id(&parent)).bind(Uuid::new_v4()).bind(f.operator.public_key().to_hex()).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    assert!(
        tx.commit().await.is_err(),
        "a link cannot commit without its fresh child, history and receipt"
    );
    assert_eq!(snapshot(&f).await, before);
    let retried = post(&app, &f.operator, &path(&parent), &request).await;
    assert_eq!(retried.0, StatusCode::CREATED);
    let saved = snapshot(&f).await;
    for sql in [
        "UPDATE work_decomposition SET depth=depth+1 WHERE company_id=$1",
        "DELETE FROM work_decomposition WHERE company_id=$1",
    ] {
        assert!(sqlx::query(sql)
            .bind(f.company)
            .execute(&f.pool)
            .await
            .is_err());
    }
    assert!(sqlx::query("TRUNCATE work_decomposition")
        .execute(&f.pool)
        .await
        .is_err());
    assert_eq!(snapshot(&f).await, saved);
}
