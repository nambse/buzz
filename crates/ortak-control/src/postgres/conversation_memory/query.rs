// A single snapshot avoids assembling an ancestry from independently changing
// reads. Each recursive edge is an exact community/channel/id/time lookup.
// LIMIT is defensive; recursion itself admits no more than source + 32 nodes.
pub(super) const RESOLVE: &str = r#"
WITH RECURSIVE visible AS MATERIALIZED (
 SELECT a.channel_id,statement_timestamp() AS observed_at,
        least(ch.ttl_deadline,b.valid_until) AS valid_before
 FROM companies co
 JOIN office_company_bindings office ON office.company_id=co.id AND office.community_id=$2
 JOIN communities cm ON cm.id=office.community_id AND cm.deleted_at IS NULL AND cm.deletion_state='active'
 JOIN projects p ON p.company_id=co.id AND p.id=$3 AND p.status='active'
 JOIN project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=cm.id
 JOIN project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id
   AND g.actor_pubkey=encode($5::bytea,'hex') AND g.revoked_at IS NULL
 JOIN channels ch ON ch.community_id=cm.id AND ch.id=a.channel_id AND ch.id=ANY($6::uuid[])
   AND ch.channel_type='stream' AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
   AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
 JOIN channel_members human ON human.community_id=cm.id AND human.channel_id=ch.id
   AND human.pubkey=$5 AND human.removed_at IS NULL AND human.role<>'bot'
 JOIN employees emp ON emp.company_id=co.id AND emp.id=$4 AND emp.status='active'
 JOIN employee_revisions rev ON rev.company_id=emp.company_id AND rev.employee_id=emp.id AND rev.id=emp.active_revision_id
 JOIN employee_office_bindings b ON b.company_id=emp.company_id AND b.employee_id=emp.id
   AND encode(b.public_key,'hex')=rev.manifest #>> '{office,public_key}'
   AND b.signer_ref=rev.manifest #>> '{office,signer_ref}'
   AND b.verified_at IS NOT NULL AND b.valid_from<=statement_timestamp()
   AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
 JOIN channel_members employee ON employee.community_id=cm.id AND employee.channel_id=ch.id
   AND employee.pubkey=b.public_key AND employee.removed_at IS NULL
 WHERE co.id=$1 AND co.status='active'
   AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=cm.id AND u.pubkey=$5
     AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
   AND NOT EXISTS(SELECT 1 FROM employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$5)
   AND NOT EXISTS(SELECT 1 FROM channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$5 AND bot.role='bot')
   AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
), source AS MATERIALIZED (
 SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
 FROM visible v JOIN office_inbox i ON i.company_id=$1 AND i.event_id=$7 AND i.state='decided'
   AND i.channel_id=v.channel_id
 JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
   AND e.pubkey=i.author_pubkey AND e.kind=i.event_kind AND e.channel_id=i.channel_id
 WHERE e.kind IN(9,40002) AND e.deleted_at IS NULL AND octet_length(e.content)<=$8
   AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
), ancestry AS (
 SELECT 0 AS hop,e.id,e.created_at,
   CASE WHEN octet_length(e.tags::text)<=$9 THEN e.tags END AS tags,
   t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
   t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
 FROM source s JOIN events e ON e.community_id=$2 AND e.id=s.id AND e.created_at=s.created_at
 LEFT JOIN thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
 UNION ALL
 SELECT a.hop+1,e.id,e.created_at,
   CASE WHEN octet_length(e.tags::text)<=$9 THEN e.tags END,
   t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
   t.root_event_id,t.root_event_created_at,t.depth
 FROM ancestry a JOIN events e ON e.community_id=$2 AND e.id=a.parent_event_id
   AND e.created_at=a.parent_event_created_at AND e.channel_id=(SELECT channel_id FROM source)
   AND e.deleted_at IS NULL AND e.kind IN(9,40002)
 LEFT JOIN thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
 WHERE a.hop<$10
)
SELECT a.*,s.channel_id,s.observed_at,s.valid_before,
 CASE WHEN a.hop=0 THEN s.content END AS source_content,
 CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
 CASE WHEN a.hop=0 THEN s.sig END AS source_signature,
 s.kind AS source_kind
FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
"#;
