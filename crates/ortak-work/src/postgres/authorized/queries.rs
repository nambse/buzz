//! Bounded authorized read queries. Source visibility is applied before pagination.
use super::*;

impl AuthorizedWork {
    /// Read an explicitly granted project with current channel membership.
    pub async fn project(&self, id: Uuid) -> Result<ApiProject> {
        bounded(async {
            let (mut tx, deadline) = self.begin().await?;
            let project = self.project_on(&mut tx, id).await?;
            self.finish(tx, deadline).await?;
            Ok(project)
        })
        .await
    }
    /// Read one coherent work aggregate after its project and canonical source gates.
    /// Consumers must project attachments/history separately before exposing execution records.
    pub async fn work_item(&self, id: Uuid) -> Result<WorkItemAggregate> {
        bounded(async {
            let (mut tx, deadline) = self.begin().await?;
            let (_, item) = self.item_on(&mut tx, id, false).await?;
            self.finish(tx, deadline).await?;
            Ok(item)
        })
        .await
    }
    /// List at most 25 currently granted projects; no legacy project is implicitly adopted.
    pub async fn list_projects(&self, cursor: Option<&str>, limit: u32) -> Result<ApiProjectPage> {
        bounded(async {
            let cursor=cursor.map(WorkListCursor::decode).transpose()?;
            let limit=limit.clamp(1,25);
            let(mut tx,deadline)=self.begin().await?;
            let rows=sqlx::query("SELECT p.*,a.channel_id,g.role FROM projects p
 JOIN project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=$2
 JOIN project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id AND g.actor_pubkey=$5 AND g.revoked_at IS NULL
 JOIN channels c ON c.community_id=a.community_id AND c.id=a.channel_id AND c.deleted_at IS NULL AND c.channel_type::text='stream'
 JOIN channel_members m ON m.community_id=c.community_id AND m.channel_id=c.id AND m.pubkey=$4 AND m.removed_at IS NULL
 WHERE p.company_id=$1 AND a.channel_id=ANY($3)
 AND ($6::timestamptz IS NULL OR (p.created_at,p.id)<($6,$7::uuid))
 ORDER BY p.created_at DESC,p.id DESC LIMIT $8")
                .bind(self.scope.company_id()).bind(self.principal.community_id)
                .bind(self.principal.channel_ids.iter().copied().collect::<Vec<_>>()).bind(&self.principal.key_bytes).bind(&self.principal.public_key)
                .bind(cursor.map(|c|c.created_at())).bind(cursor.map(|c|c.id())).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
            let mut items=Vec::new();
            // Initial authorization filters before LIMIT. Then acquire each parent fence
            // and recheck grants in a fresh statement, including the bounded lookahead.
            for row in &rows {
                let project=self.project_on(&mut tx,row.try_get("id")?).await?;
                if items.len()<limit as usize {items.push(project);}
            }
            let next_cursor=if rows.len()>limit as usize {items.last().map(|p|format!("{}:{}",p.record.created_at.timestamp_micros(),p.record.project.id.as_simple()))}else{None};
            self.finish(tx,deadline).await?;Ok(ApiProjectPage{items,next_cursor})
        }).await
    }
    /// List a project's visible work, filtering canonical source scope before LIMIT.
    pub async fn list_project_work(&self, id: Uuid, query: &WorkListQuery) -> Result<WorkListPage> {
        bounded(async {
            let(mut tx,deadline)=self.begin().await?;
            let project=self.project_on(&mut tx,id).await?;
            let limit=query.page_size().min(25);
            let mut sql=sqlx::QueryBuilder::new("SELECT w.id,w.project_id,w.title,w.priority,w.state,w.version,w.source_message_id,w.created_at,w.updated_at FROM work_items w WHERE w.company_id=$1 AND w.project_id=$4 AND ");
            sql.push(authority::SOURCE_VISIBLE).push(" AND ($5::text[] IS NULL OR w.state=ANY($5)) AND ($6::timestamptz IS NULL OR (w.created_at,w.id)<($6,$7::uuid)) ORDER BY w.created_at DESC,w.id DESC LIMIT $8");
            let rows=sql.build().bind(self.scope.company_id()).bind(self.principal.community_id).bind(project.channel_id).bind(id)
                .bind(query.state_filter()).bind(query.cursor.map(|c|c.created_at())).bind(query.cursor.map(|c|c.id())).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
            let mut items=rows.iter().take(limit as usize).map(summary_from_row).collect::<Result<Vec<_>>>()?;
            let next_cursor=if rows.len()>limit as usize {items.last().map(WorkListCursor::after)}else{None};
            self.finish(tx,deadline).await?;Ok(WorkListPage{items:std::mem::take(&mut items),next_cursor})
        }).await
    }
}
