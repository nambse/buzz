SELECT encode(ev.id,'hex') AS message_id, ev.created_at, ev.content,
       encode(sha256(convert_to(ev.content,'UTF8')),'hex') AS content_hash,
       encode(ev.pubkey,'hex') AS author_public_key,
       author.employee_id AS author_employee_id,
       coalesce(nullif(author.name,''),nullif(u.display_name,''),encode(ev.pubkey,'hex')) AS author_name,
       encode(tm.parent_event_id,'hex') AS parent_id,
       encode(tm.root_event_id,'hex') AS thread_id
FROM events ev
LEFT JOIN thread_metadata tm
  ON tm.community_id=ev.community_id AND tm.event_id=ev.id AND tm.event_created_at=ev.created_at
LEFT JOIN users u ON u.community_id=ev.community_id AND u.pubkey=ev.pubkey
LEFT JOIN LATERAL (
    SELECT b.employee_id, r.manifest->>'name' AS name
    FROM employee_office_bindings b JOIN employees e ON e.company_id=b.company_id AND e.id=b.employee_id
    JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
    WHERE b.company_id=$8 AND b.public_key=ev.pubkey ORDER BY b.employee_id LIMIT 1
) author ON true
WHERE ev.community_id=$1 AND ev.channel_id=$2 AND ev.kind IN(9,40002)
  AND ev.deleted_at IS NULL AND ev.created_at <= $3 AND ev.received_at <= $4 AND ev.id<>$5
  AND ($7::text IS NULL OR encode(ev.id,'hex')=$7 OR encode(tm.root_event_id,'hex')=$7)
ORDER BY CASE WHEN encode(ev.id,'hex')=$6 THEN 0 WHEN encode(ev.id,'hex')=$7 THEN 1 ELSE 2 END,
         ev.created_at DESC, ev.id DESC
LIMIT $9
