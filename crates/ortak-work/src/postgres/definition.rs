//! Definition persistence shares the authorized facade's receipt transaction.
use super::*;
use ortak_domain::EditWorkDefinition;

pub(super) async fn edit_on(
    c: &mut PgConnection,
    scope: &CompanyScope,
    id: Uuid,
    version: i64,
    input: &EditWorkDefinition,
    actor: &WorkActor,
) -> Result<WorkItemAggregate> {
    verify_actor(c, scope, actor).await?;
    let mut item = lock_item(c, scope, id, version, ProjectLock::Share).await?;
    let old_criteria = item.criteria.clone();
    let ids: Vec<_> = input
        .additional_criteria
        .iter()
        .map(|_| Uuid::new_v4())
        .collect();
    let event = item.edit_definition(input, &ids)?;
    for (old, criterion) in old_criteria.iter().zip(&item.criteria) {
        if old.text == criterion.text {
            continue;
        }
        let updated = sqlx::query("UPDATE work_acceptance_criteria SET text=$4 WHERE company_id=$1 AND work_item_id=$2 AND id=$3 AND status='pending'")
            .bind(scope.company_id()).bind(id).bind(criterion.id).bind(&criterion.text)
            .execute(&mut *c).await?.rows_affected();
        if updated != 1 {
            return Err(invalid("criterion row disagrees with definition"));
        }
    }
    for criterion in item.criteria.iter().skip(old_criteria.len()) {
        sqlx::query("INSERT INTO work_acceptance_criteria(company_id,work_item_id,id,position,text) VALUES($1,$2,$3,$4,$5)")
            .bind(scope.company_id()).bind(id).bind(criterion.id).bind(criterion.position as i16)
            .bind(&criterion.text).execute(&mut *c).await?;
    }
    persist_event(c, scope, &item, version, actor, &event).await?;
    require_aggregate(c, scope, id).await
}
