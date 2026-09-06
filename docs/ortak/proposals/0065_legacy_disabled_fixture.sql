-- Test-only input: fresh disposable PostgreSQL 55432, source migrations 1–64.
-- Root runs this BEFORE proposal65, using a fresh DB for each changed proposal.
-- No real resource refs, identities or credentials. Stable IDs are fixture-only.
INSERT INTO companies(id,slug,display_name) VALUES('65000000-0000-4000-8000-000000000001','lifecycle65-upgrade-fixture','Lifecycle migration fixture');
INSERT INTO employees(company_id,id,status) VALUES
 ('65000000-0000-4000-8000-000000000001','legacy-disabled','draft'),
 ('65000000-0000-4000-8000-000000000001','legacy-unactivated','disabled'),
 ('65000000-0000-4000-8000-000000000001','still-active','draft');
INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES
 ('65000000-0000-4000-8000-000000000001','65000000-0000-4000-8000-000000000002','legacy-disabled',1,'{}',decode(repeat('00',32),'hex'),'adopt'),
 ('65000000-0000-4000-8000-000000000001','65000000-0000-4000-8000-000000000003','still-active',1,'{}',decode(repeat('00',32),'hex'),'adopt');
UPDATE employees SET status='active',active_revision_id='65000000-0000-4000-8000-000000000002' WHERE company_id='65000000-0000-4000-8000-000000000001' AND id='legacy-disabled';
UPDATE employees SET status='active',active_revision_id='65000000-0000-4000-8000-000000000003' WHERE company_id='65000000-0000-4000-8000-000000000001' AND id='still-active';
INSERT INTO runs(company_id,id,employee_id,employee_revision_id,runtime_adapter,status,delivery_intent,finished_at) VALUES
 ('65000000-0000-4000-8000-000000000001','65000000-0000-4000-8000-000000000004','legacy-disabled','65000000-0000-4000-8000-000000000002','fake-runtime','completed','silent',clock_timestamp());
INSERT INTO provisioning_operations(company_id,id,employee_id,mode,dry_run,idempotency_key,manifest,manifest_fingerprint) VALUES
 ('65000000-0000-4000-8000-000000000001','65000000-0000-4000-8000-000000000005','legacy-disabled','update',false,'legacy-migration-fixture','{}',decode(repeat('00',32),'hex'));
UPDATE employees SET status='disabled' WHERE company_id='65000000-0000-4000-8000-000000000001' AND id='legacy-disabled';
