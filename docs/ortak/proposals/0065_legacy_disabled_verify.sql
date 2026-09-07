-- Run after proposal65 against the isolated fixture above; raises on failure.
DO $$
DECLARE company UUID:='65000000-0000-4000-8000-000000000001';
BEGIN
 IF (SELECT count(*) FROM employees WHERE company_id=company AND id IN('legacy-disabled','legacy-unactivated') AND status='disabled' AND lifecycle_epoch=1)<>2
 OR (SELECT lifecycle_epoch FROM employees WHERE company_id=company AND id='still-active')<>0
 OR (SELECT employee_lifecycle_epoch FROM runs WHERE company_id=company AND id='65000000-0000-4000-8000-000000000004')<>0
 OR (SELECT employee_lifecycle_epoch FROM provisioning_operations WHERE company_id=company AND id='65000000-0000-4000-8000-000000000005')<>0
 OR (SELECT count(*) FROM employee_lifecycle_events WHERE company_id=company AND action='disable' AND lifecycle_epoch=1 AND command_id IS NULL AND command_lease_token IS NULL AND command_lease_expires_at IS NULL AND previous_revision_id IS NOT DISTINCT FROM result_revision_id)<>2 THEN
   RAISE EXCEPTION 'Lifecycle migration did not preserve legacy disable barrier';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM employee_lifecycle_events WHERE company_id=company AND employee_id='legacy-disabled' AND result_revision_id='65000000-0000-4000-8000-000000000002') THEN
   RAISE EXCEPTION 'Lifecycle migration lost revision identity';
 END IF;
 BEGIN
   UPDATE employees SET status='active' WHERE company_id=company AND id='legacy-disabled';
   RAISE EXCEPTION 'Unsealed legacy reenable was accepted';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
END $$;
