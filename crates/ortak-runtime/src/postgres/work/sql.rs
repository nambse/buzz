pub(super) const SOURCE: &str = "SELECT x.*, r.status AS run_status,r.runtime_adapter,r.runtime_run_ref,
 r.routing_decision_id,r.message_id,r.root_message_id,r.work_item_id AS run_work_item_id,
 r.employee_id AS run_employee_id,r.employee_revision_id AS run_revision_id,
 r.employee_lifecycle_epoch=e.lifecycle_epoch AS current_lifecycle,
 w.state AS work_state,w.version AS work_version,w.source_message_id,
 p.status AS project_status,a.community_id,a.channel_id,g.generation,
 EXISTS(SELECT 1 FROM project_access_grants acl WHERE acl.company_id=x.company_id AND acl.project_id=x.project_id
    AND acl.actor_pubkey=x.requested_by AND acl.role IN('contributor','owner') AND acl.revoked_at IS NULL) AS can_contribute,
 EXISTS(SELECT 1 FROM channel_members h WHERE h.community_id=a.community_id AND h.channel_id=a.channel_id
    AND h.pubkey=decode(x.requested_by,'hex') AND h.removed_at IS NULL AND h.role<>'bot')
 AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=a.community_id AND u.pubkey=decode(x.requested_by,'hex')
    AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
 AND NOT EXISTS(SELECT 1 FROM employee_office_bindings eb WHERE eb.company_id=x.company_id AND eb.public_key=decode(x.requested_by,'hex'))
 AND NOT EXISTS(SELECT 1 FROM channel_members bot WHERE bot.community_id=a.community_id AND bot.pubkey=decode(x.requested_by,'hex') AND bot.role='bot') AS human_member,
 EXISTS(SELECT 1 FROM work_assignments wa WHERE wa.company_id=x.company_id AND wa.work_item_id=x.work_item_id
    AND wa.employee_id=x.employee_id AND wa.status='active' AND wa.role IN('owner','contributor')) AS assigned,
 NOT EXISTS(SELECT 1 FROM work_dependencies d JOIN work_items dependency ON dependency.company_id=d.company_id AND dependency.id=d.depends_on_work_item_id
    WHERE d.company_id=x.company_id AND d.work_item_id=x.work_item_id AND d.released_at IS NULL AND dependency.state NOT IN('completed','cancelled')) AS dependencies_clear,
 NOT EXISTS(SELECT 1 FROM work_acceptance_criteria cr WHERE cr.company_id=x.company_id AND cr.work_item_id=x.work_item_id AND cr.status<>'pending')
 AND NOT EXISTS(SELECT 1 FROM work_approvals ap WHERE ap.company_id=x.company_id AND ap.work_item_id=x.work_item_id AND ap.status<>'pending') AS no_review,
 c.status AS company_status,e.status AS employee_status,rev.manifest,active_rev.manifest AS active_manifest,
 rb.adapter AS binding_adapter,rb.profile_ref AS binding_profile_ref,rb.model AS binding_model,
 rb.workspace_ref AS binding_workspace_ref,rb.credential_refs AS binding_credential_refs,rb.options AS binding_options,rb.validated_at AS binding_validated_at,
 mb.adapter AS memory_adapter,mb.endpoint_ref AS memory_endpoint_ref,mb.workspace AS memory_workspace,
 mb.user_peer AS memory_user_peer,mb.employee_peer AS memory_employee_peer,mb.options AS memory_options,mb.validated_at AS memory_validated_at,
 amb.adapter AS active_memory_adapter,amb.endpoint_ref AS active_memory_endpoint_ref,amb.workspace AS active_memory_workspace,
 amb.user_peer AS active_memory_user_peer,amb.employee_peer AS active_memory_employee_peer,amb.options AS active_memory_options,amb.validated_at AS active_memory_validated_at
 FROM work_executions x JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
 JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
 JOIN projects p ON p.company_id=x.company_id AND p.id=x.project_id
 JOIN project_api_bindings a ON a.company_id=x.company_id AND a.project_id=x.project_id
 JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
 JOIN companies c ON c.id=x.company_id
 JOIN employees e ON e.company_id=x.company_id AND e.id=x.employee_id
 JOIN employee_revisions rev ON rev.company_id=x.company_id AND rev.employee_id=x.employee_id AND rev.id=x.employee_revision_id
 JOIN employee_revisions active_rev ON active_rev.company_id=e.company_id AND active_rev.employee_id=e.id AND active_rev.id=e.active_revision_id
 LEFT JOIN employee_runtime_bindings rb ON rb.company_id=x.company_id AND rb.employee_id=x.employee_id AND rb.revision_id=x.employee_revision_id
 LEFT JOIN employee_memory_bindings mb ON mb.company_id=x.company_id AND mb.employee_id=x.employee_id AND mb.revision_id=x.employee_revision_id
 LEFT JOIN employee_memory_bindings amb ON amb.company_id=e.company_id AND amb.employee_id=e.id AND amb.revision_id=e.active_revision_id
 WHERE x.company_id=$1 AND x.run_id=$2";
