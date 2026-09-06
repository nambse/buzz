use super::*;
use ortak_domain::{CriterionEdit, EditWorkDefinition};

fn amendment(item: &WorkItemAggregate) -> WorkMutation {
    WorkMutation::EditDefinition {
        definition: EditWorkDefinition {
            title: Some("Amended title".into()),
            description: Some("Amended description".into()),
            criteria: item
                .item
                .criteria
                .iter()
                .map(|c| CriterionEdit {
                    id: c.id,
                    text: Some("Amended criterion".into()),
                })
                .collect(),
            additional_criteria: vec!["Appended criterion".into()],
        },
    }
}
#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn concurrent_definition_edit_commits_one_version_history_and_receipt_and_rolls_back_storage_failure(
) {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let mut input = f.item_input(project);
    input.criteria.push("Original criterion".into());
    let initial = f
        .api
        .create_work_item(Uuid::new_v4(), input)
        .await
        .unwrap()
        .item;
    let id = initial.item.id;
    let op = Uuid::new_v4();
    let action = amendment(&initial);
    let (a, b) = tokio::join!(
        f.api.mutate(op, id, 1, action.clone()),
        f.api.mutate(op, id, 1, action.clone())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a, b);
    assert_eq!(a.item.version, 2);
    assert_eq!(a.item.criteria.len(), 2);
    assert_eq!(a.item.criteria[0].id, initial.item.criteria[0].id);
    assert_eq!(a.history.len(), 2);
    assert_eq!(a.history[1].event.event_type(), "work.definition_edited");

    let failing_op = Uuid::new_v4();
    let name = format!("fixture_definition_receipt_{}", failing_op.simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture definition receipt failure' USING ERRCODE='serialization_failure'; END $$;
    CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN (NEW.operation_id='{failing_op}'::uuid) EXECUTE FUNCTION {name}();")))
        .execute(&f.company.pool).await.unwrap();
    let failed = f.api.mutate(failing_op, id, 2, amendment(&a)).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.company.pool)
    .await
    .unwrap();
    assert!(failed.is_err());
    assert_eq!(f.api.work_item(id).await.unwrap(), a);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$2",
    )
    .bind(f.company.scope.company_id())
    .bind(failing_op)
    .fetch_one(&f.company.pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    let recovered = f
        .api
        .mutate(failing_op, id, 2, amendment(&a))
        .await
        .unwrap();
    assert_eq!(recovered.item.version, 3);
    assert_eq!(recovered.item.criteria.len(), 3);
}
#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn definition_edit_preserves_original_promotion_retry_and_null_canonical_values() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let source = f.source(f.channel).await;
    let mut input = f.item_input(project);
    input.source_message_id = Some(source.to_hex());
    input.description = "Example: password=demo".into();
    input.criteria = vec!["Document api_key=example".into()];
    let initial = f
        .api
        .create_work_item(Uuid::new_v4(), input.clone())
        .await
        .unwrap()
        .item;
    let action = WorkMutation::EditDefinition {
        definition: EditWorkDefinition {
            title: Some("Updated title".into()),
            description: None,
            criteria: vec![CriterionEdit {
                id: initial.item.criteria[0].id,
                text: None,
            }],
            additional_criteria: vec!["New criterion".into()],
        },
    };
    let edited = f
        .api
        .mutate(Uuid::new_v4(), initial.item.id, 1, action)
        .await
        .unwrap();
    assert_eq!(edited.item.description, input.description);
    assert_eq!(edited.item.criteria[0].text, input.criteria[0]);
    let replay = f
        .company
        .service
        .create_work_item(&f.company.scope, input.clone(), human())
        .await
        .unwrap();
    assert!(!replay.created);
    assert_eq!(replay.item, edited);
    input.title = "Unrelated creation request".into();
    assert!(matches!(
        f.company
            .service
            .create_work_item(&f.company.scope, input, human())
            .await,
        Err(WorkError::PromotionConflict { .. })
    ));
}
#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn changed_pending_criterion_without_same_transaction_definition_history_is_refused() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let mut input = f.item_input(project);
    input.criteria.push("Unchanged criterion".into());
    let initial = f
        .api
        .create_work_item(Uuid::new_v4(), input)
        .await
        .unwrap()
        .item;
    let error = sqlx::query("UPDATE work_acceptance_criteria SET text='Unjournaled edit' WHERE company_id=$1 AND work_item_id=$2")
        .bind(f.company.scope.company_id()).bind(initial.item.id).execute(&f.company.pool).await;
    assert!(error.is_err());
    assert_eq!(f.api.work_item(initial.item.id).await.unwrap(), initial);
}
