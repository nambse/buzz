use super::*;

// All audience and source checks precede LIMIT, including mixed-project queues.
const QUEUE_SQL:&str="SELECT w.id,w.project_id,w.title,w.priority,w.state,w.version,
 w.source_message_id,w.created_at,w.updated_at,s.role AS assignment_role
 FROM work_assignments s JOIN work_items w ON w.company_id=s.company_id AND w.id=s.work_item_id
 JOIN projects p ON p.company_id=w.company_id AND p.id=w.project_id AND p.status='active'
 JOIN project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=$2
 JOIN project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id AND g.actor_pubkey=$5 AND g.revoked_at IS NULL
 JOIN channels c ON c.community_id=a.community_id AND c.id=a.channel_id AND c.channel_type::text='stream' AND c.deleted_at IS NULL
 JOIN channel_members m ON m.community_id=c.community_id AND m.channel_id=c.id AND m.pubkey=$4 AND m.removed_at IS NULL
 WHERE w.company_id=$1 AND a.channel_id=ANY($3) AND s.employee_id=$6 AND s.status='active'
 AND w.state IN('proposed','ready','in_progress','blocked','review')
 AND (w.source_message_id IS NULL OR EXISTS(
   SELECT 1 FROM office_inbox i JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
    AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
   WHERE i.company_id=w.company_id AND i.event_id=w.source_message_id AND i.state='decided'
    AND i.channel_id=a.channel_id AND e.kind IN(9,40002) AND e.deleted_at IS NULL))
 AND ($7::timestamptz IS NULL OR (w.created_at,w.id)<($7,$8::uuid))
 AND ($10::uuid[] IS NULL OR w.id=ANY($10))
 ORDER BY w.created_at DESC,w.id DESC LIMIT $9";

impl AuthorizedWork {
    pub(super) async fn queue_rows(
        &self,
        c: &mut PgConnection,
        employee: &EmployeeId,
        cursor: Option<WorkListCursor>,
        limit: u32,
        ids: Option<&[Uuid]>,
    ) -> Result<Vec<PgRow>> {
        Ok(sqlx::query(QUEUE_SQL)
            .bind(self.scope.company_id())
            .bind(self.principal.community_id)
            .bind(
                self.principal
                    .channel_ids
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .bind(&self.principal.key_bytes)
            .bind(&self.principal.public_key)
            .bind(employee.as_str())
            .bind(cursor.map(|c| c.created_at()))
            .bind(cursor.map(|c| c.id()))
            .bind(i64::from(limit) + 1)
            .bind(ids)
            .fetch_all(c)
            .await?)
    }
}
