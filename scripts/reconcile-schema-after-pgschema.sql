-- Reconcile schema details that pgschema does not preserve.
--
-- pgschema reconciles DDL, but it does not execute seed DML or preserve every
-- table storage parameter from schema/schema.sql. It also currently emits
-- partition children as standalone CREATE TABLE statements. Every pgschema
-- apply caller must run this idempotent script so fresh bootstraps converge on
-- the same live database contract as migration-managed databases.

-- pgschema also omits extension installation. Install this prerequisite before
-- restoring any exact function body that calls public.digest; desired SQL
-- bootstrap functions remain typed and closed until that restoration.
CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p_past'::regclass
    ) THEN
        -- pgschema may copy parent triggers onto standalone children. Drop
        -- those copies before ATTACH; PostgreSQL recreates inherited parent
        -- triggers while attaching and rejects same-named child triggers.
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p_past;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p_past;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p_past;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p_past;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p_past;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p_past;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p_past;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p_past;
        ALTER TABLE events ATTACH PARTITION events_p_past
            FOR VALUES FROM (MINVALUE) TO ('2026-01-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_01'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_01;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_01;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_01;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_01;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_01;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_01;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_01;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_01;
        ALTER TABLE events ATTACH PARTITION events_p2026_01
            FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_02'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_02;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_02;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_02;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_02;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_02;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_02;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_02;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_02;
        ALTER TABLE events ATTACH PARTITION events_p2026_02
            FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_03'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_03;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_03;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_03;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_03;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_03;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_03;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_03;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_03;
        ALTER TABLE events ATTACH PARTITION events_p2026_03
            FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_04'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_04;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_04;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_04;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_04;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_04;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_04;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_04;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_04;
        ALTER TABLE events ATTACH PARTITION events_p2026_04
            FOR VALUES FROM ('2026-04-01') TO ('2026-05-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_05'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_05;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_05;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_05;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_05;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_05;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_05;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_05;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_05;
        ALTER TABLE events ATTACH PARTITION events_p2026_05
            FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p2026_06'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p2026_06;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p2026_06;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p2026_06;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p2026_06;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p2026_06;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p2026_06;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p2026_06;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p2026_06;
        ALTER TABLE events ATTACH PARTITION events_p2026_06
            FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'events'::regclass
          AND inhrelid = 'events_p_future'::regclass
    ) THEN
        DROP TRIGGER IF EXISTS events_enqueue_push_match ON events_p_future;
        DROP TRIGGER IF EXISTS events_refresh_channel_ttl ON events_p_future;
        DROP TRIGGER IF EXISTS events_created_at_floor ON events_p_future;
        DROP TRIGGER IF EXISTS community_write_fence_events ON events_p_future;
        DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events_p_future;
        DROP TRIGGER IF EXISTS ortak_office_authority_events ON events_p_future;
        DROP TRIGGER IF EXISTS conversation_epoch_events ON events_p_future;
        DROP TRIGGER IF EXISTS employee_memory_epoch_events ON events_p_future;
        ALTER TABLE events ATTACH PARTITION events_p_future
            FOR VALUES FROM ('2026-07-01') TO (MAXVALUE);
    END IF;

    -- When pgschema creates partition children as standalone tables, it also
    -- preserves the parent's identity column on delivery_log children. PostgreSQL
    -- rejects attaching a child table that has its own identity column, so each
    -- delivery_log attach path drops that standalone identity first. Raw
    -- schema-created partitions are already attached, so these branches do not
    -- run against inherited partition columns.

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p_past'::regclass
    ) THEN
        ALTER TABLE delivery_log_p_past ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p_past;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p_past
            FOR VALUES FROM (MINVALUE) TO ('2026-03-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p2026_03'::regclass
    ) THEN
        ALTER TABLE delivery_log_p2026_03 ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p2026_03;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p2026_03
            FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p2026_04'::regclass
    ) THEN
        ALTER TABLE delivery_log_p2026_04 ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p2026_04;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p2026_04
            FOR VALUES FROM ('2026-04-01') TO ('2026-05-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p2026_05'::regclass
    ) THEN
        ALTER TABLE delivery_log_p2026_05 ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p2026_05;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p2026_05
            FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p2026_06'::regclass
    ) THEN
        ALTER TABLE delivery_log_p2026_06 ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p2026_06;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p2026_06
            FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_inherits
        WHERE inhparent = 'delivery_log'::regclass
          AND inhrelid = 'delivery_log_p_future'::regclass
    ) THEN
        ALTER TABLE delivery_log_p_future ALTER COLUMN id DROP IDENTITY IF EXISTS;
        DROP TRIGGER IF EXISTS community_write_fence_delivery_log ON delivery_log_p_future;
        ALTER TABLE delivery_log ATTACH PARTITION delivery_log_p_future
            FOR VALUES FROM ('2026-07-01') TO (MAXVALUE);
    END IF;
END $$;







-- pgschema reconciles DDL but does not apply seed DML or table storage
-- parameters from schema/schema.sql. Restore those parts of the desired-state
-- contract explicitly and fail the bootstrap if the live catalog disagrees.
ALTER TABLE replica_heartbeat SET (vacuum_truncate = false);

INSERT INTO replica_heartbeat (id) VALUES (1)
ON CONFLICT (id) DO NOTHING;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_class AS relation
        JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = current_schema()
          AND relation.relname = 'replica_heartbeat'
          AND COALESCE(
              relation.reloptions @> ARRAY['vacuum_truncate=false']::text[],
              false
          )
    ) THEN
        RAISE EXCEPTION 'replica_heartbeat must disable vacuum truncation after pgschema apply';
    END IF;

    IF (SELECT count(*) FROM replica_heartbeat WHERE id = 1) <> 1 THEN
        RAISE EXCEPTION 'replica_heartbeat must contain its singleton row after pgschema apply';
    END IF;
END $$;


-- Ortak authority statement triggers are not inherited by event partitions.
-- pgschema ignores the migration DO block; converge the live catalog here.
DO $$
DECLARE
    partition_table REGCLASS;
BEGIN
    IF to_regclass('office_authority_generations') IS NULL THEN RETURN; END IF;
    FOR partition_table IN
        SELECT relid FROM pg_partition_tree('events'::REGCLASS) WHERE isleaf
    LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid = partition_table
                       AND tgname = 'ortak_office_no_truncate' AND NOT tgisinternal) THEN
            EXECUTE format('CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON %s FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()', partition_table);
        END IF;
        IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid = partition_table
                       AND tgname = 'ortak_office_authority_events' AND tgenabled = 'O') THEN
            RAISE EXCEPTION 'Office authority row fence is missing on event partition %', partition_table;
        END IF;
    END LOOP;
END
$$;

-- Migration0051: desired-state apply omits completion-job backfill DML.
INSERT INTO runtime_office_outputs(company_id,run_id)
SELECT company_id,id FROM runs WHERE work_item_id IS NULL AND routing_decision_id IS NOT NULL AND status='completed' AND delivery_intent IN ('reply','channel')
ON CONFLICT (company_id,run_id) DO NOTHING;
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM runs r WHERE r.work_item_id IS NULL AND r.routing_decision_id IS NOT NULL AND r.status='completed'
               AND r.delivery_intent IN ('reply','channel') AND NOT EXISTS
               (SELECT 1 FROM runtime_office_outputs j WHERE j.company_id=r.company_id AND j.run_id=r.id)) THEN
        RAISE EXCEPTION 'ortak: completed run lacks durable Office output job';
    END IF;
END $$;

-- Migration0052: acknowledged publication backfill is DML, so desired-state
-- bootstrap runs it explicitly and verifies the live scheduling/fence catalog.
SELECT ortak_insert_acknowledged_memory_write(company_id,id)
FROM outbox WHERE kind='office_publish' AND state='delivered';
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='outbox'::regclass
                   AND tgname='trg_outbox_schedule_memory_write' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='employee_memory_bindings'::regclass
                   AND tgname='ortak_office_authority_memory_bindings' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='employee_memory_bindings'::regclass
                   AND tgname='ortak_office_no_truncate' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='runtime_memory_writes'::regclass
                   AND tgname='trg_memory_write_authority' AND tgenabled='O'
                   AND tgdeferrable AND tginitdeferred)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='run_context_snapshots'::regclass
                   AND tgname='trg_run_context_snapshot_immutable' AND tgenabled='O') THEN
        RAISE EXCEPTION 'ortak: durable memory scheduling or authority guard is missing';
    END IF;
    IF EXISTS (
        SELECT 1 FROM outbox o JOIN runtime_office_outputs j ON j.company_id=o.company_id AND j.outbox_id=o.id
        JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
        JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.id=r.employee_revision_id
        JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
        WHERE o.kind='office_publish' AND o.state='delivered' AND o.signed_event_id IS NOT NULL
          AND o.signed_event_bytes IS NOT NULL AND o.run_id=r.id AND j.state='enqueued'
          AND r.status='completed' AND r.delivery_intent IN ('reply','channel') AND i.channel_id IS NOT NULL
          AND jsonb_typeof(rev.manifest->'memory')='object'
          AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
          AND NOT EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)
          AND NOT EXISTS (SELECT 1 FROM runtime_memory_writes x WHERE x.company_id=r.company_id AND x.run_id=r.id)
    ) THEN
        RAISE EXCEPTION 'ortak: acknowledged Office output lacks its durable memory job';
    END IF;
END $$;

-- Migration0053: prove desired-state apply retained deferred lease enforcement.
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger t JOIN pg_proc p ON p.oid=t.tgfoid
        WHERE t.tgrelid='routing_decisions'::regclass
          AND t.tgname='ortak_routing_claim_expiry_at_commit'
          AND p.proname='ortak_check_routing_claim_expiry'
          AND t.tgenabled='O' AND t.tgdeferrable AND t.tginitdeferred
          AND t.tgtype=5 AND NOT t.tgisinternal
    ) THEN
        RAISE EXCEPTION 'ortak: waking routing claim expiry guard is missing';
    END IF;
END $$;


-- Migration0054: pgschema omits SELECT-based community fence attachment.
SELECT attach_community_write_fence('project_api_bindings');
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='project_access_grants'::regclass
        AND tgname='project_access_guard' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='project_api_bindings'::regclass
        AND tgname='project_api_binding_immutable' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='project_api_bindings'::regclass
        AND tgname='community_write_fence_project_api_bindings' AND tgenabled='O')
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='work_api_operations'::regclass
        AND tgname='work_api_receipt_at_commit' AND tgenabled='O' AND tgdeferrable AND tginitdeferred)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='work_api_operations'::regclass
        AND tgname='work_api_operation_immutable' AND tgenabled='O') THEN
        RAISE EXCEPTION 'ortak: Work API authority or receipt guard is missing';
    END IF;
END $$;

-- Migration0055: company Work evidence survives a fenced community purge.
-- pgschema must retain both the executor-only guard and its commit-time proof.
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='project_api_bindings'::regclass
        AND tgname='project_api_binding_immutable' AND tgenabled='O'
        AND tgfoid='ortak_guard_project_api_binding()'::regprocedure)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='project_api_bindings'::regclass
        AND tgname='project_api_binding_purge_at_commit' AND tgenabled='O'
        AND tgfoid='ortak_project_binding_purge_at_commit()'::regprocedure
        AND tgdeferrable AND tginitdeferred)
       OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='project_access_grants'::regclass
        AND conname='project_access_grants_company_id_project_id_fkey'
        AND contype='f' AND confrelid='projects'::regclass AND convalidated)
       OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='work_api_operations'::regclass
        AND conname='work_api_operations_company_id_project_id_fkey'
        AND contype='f' AND confrelid='projects'::regclass AND convalidated) THEN
        RAISE EXCEPTION 'ortak: project binding purge or retained evidence guard is missing';
    END IF;
END $$;

-- Migration0056: activation succeeds only with its fresh, immutable admission.
-- These are live catalog assertions; desired-schema text alone is not proof.
DO $$
DECLARE
    required RECORD;
BEGIN
    FOR required IN SELECT * FROM (VALUES
        ('provisioning_operations'::REGCLASS, 'ortak_activation_admission_at_commit',
            'ortak_check_activation_admission_at_commit()'::REGPROCEDURE, 21, true, true),
        ('provisioning_operations'::REGCLASS, 'ortak_activation_operation_immutable',
            'ortak_guard_activation_operation()'::REGPROCEDURE, 27, false, false),
        ('provisioning_operation_steps'::REGCLASS, 'ortak_activation_receipt_immutable',
            'ortak_guard_activation_receipt()'::REGPROCEDURE, 27, false, false),
        ('provisioning_operations'::REGCLASS, 'ortak_activation_operation_no_truncate',
            'ortak_reject_row_mutation()'::REGPROCEDURE, 34, false, false),
        ('provisioning_operation_steps'::REGCLASS, 'ortak_activation_receipt_no_truncate',
            'ortak_reject_row_mutation()'::REGPROCEDURE, 34, false, false)
    ) AS guards(relation_id, trigger_name, function_id, trigger_type, deferred, initially_deferred)
    LOOP
        IF NOT EXISTS (
            SELECT 1 FROM pg_trigger t
            WHERE t.tgrelid=required.relation_id AND t.tgname=required.trigger_name
              AND t.tgfoid=required.function_id AND t.tgenabled='O' AND NOT t.tgisinternal
              AND t.tgtype=required.trigger_type AND t.tgdeferrable=required.deferred
              AND t.tginitdeferred=required.initially_deferred
              AND (NOT required.deferred OR t.tgqual IS NOT NULL)
        ) THEN
            RAISE EXCEPTION 'ortak: activation admission guard % is missing or malformed',
                required.trigger_name;
        END IF;
    END LOOP;
END $$;

-- Migration0058: pgschema flattens BETWEEN into a three-term AND expression.
-- Converge to PostgreSQL's migration expression so the live catalogs, including
-- the validated constraint body, agree across install and upgrade paths.
DO $$ BEGIN
    IF (SELECT pg_get_constraintdef(oid) FROM pg_constraint
        WHERE conrelid='office_identity_profiles'::REGCLASS
          AND conname='office_identity_profiles_idempotency_key_check')
       IS DISTINCT FROM
       'CHECK ((((length(idempotency_key) >= 1) AND (length(idempotency_key) <= 256)) AND (idempotency_key ~ ''^[A-Za-z0-9:_.-]+$''::text)))' THEN
        ALTER TABLE office_identity_profiles
            DROP CONSTRAINT IF EXISTS office_identity_profiles_idempotency_key_check;
        ALTER TABLE office_identity_profiles
            ADD CONSTRAINT office_identity_profiles_idempotency_key_check
            CHECK (length(idempotency_key) BETWEEN 1 AND 256
                   AND idempotency_key ~ '^[A-Za-z0-9:_.-]+$');
    END IF;
END $$;

-- Migration0059: pgschema omits SELECT-based community fence attachment.
SELECT attach_community_write_fence('office_routing_cohorts');
SELECT attach_community_write_fence('office_routing_channels');
SELECT attach_community_write_fence('office_inbox_reconciliations');
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='office_inbox_reconciliations'::regclass
        AND tgname='ortak_inbox_reconciliation_evidence' AND tgenabled='O' AND tgtype=31)
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='office_routing_cohorts'::regclass
        AND tgname='ortak_routing_cohort_state' AND tgenabled='O' AND tgtype=23)
 THEN
        RAISE EXCEPTION 'ortak: routing reconciliation or Activity notification guards missing';
    END IF;
END $$;

-- Exact current Activity inventory: missing, extra, disabled, wrong-scope or
-- wrongly attributed notifications are all refused, not only a count mismatch.
DO $ortak_activity_inventory$
BEGIN
    IF EXISTS (
        WITH required(table_name,trigger_name,trigger_type,argument) AS (VALUES
            ('run_events','trg_activity_events',5,'run_id'),
            ('runs','trg_activity_runs',21,'id'),
            ('run_cancel_requests','trg_activity_cancel_requests',21,'run_id'),
            ('runtime_cancellations','trg_activity_cancellations',21,'run_id'),
            ('runtime_office_outputs','trg_activity_office_outputs',21,'run_id'),
            ('outbox','trg_activity_outbox',21,'run_id'),
            ('runtime_memory_writes','trg_activity_memory_writes',21,'run_id'),
            ('run_context_snapshots','trg_activity_context',5,'run_id'),
            ('office_authority_generations','trg_activity_authority',21,''),
            ('work_authority_generations','trg_activity_work_authority',21,''),
            ('runtime_work_outputs','trg_activity_work_outputs',21,'run_id'),
            ('reviewed_memory_facts','trg_activity_reviewed_fact_use',17,''),
            ('reviewed_memory_targets','trg_activity_reviewed_target_use',17,'')
        ), observed AS (
            SELECT n.nspname,c.relname,t.* FROM pg_trigger t
            JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
            WHERE t.tgfoid='ortak_activity_notify()'::regprocedure AND NOT t.tgisinternal
        )
        SELECT 1 FROM required r FULL JOIN observed t ON t.nspname='public'
            AND t.relname=r.table_name AND t.tgname=r.trigger_name
        WHERE r.trigger_name IS NULL OR t.tgname IS NULL OR t.tgenabled<>'O'
            OR t.tgtype<>r.trigger_type OR t.tgdeferrable OR t.tginitdeferred OR t.tgnargs<>1
            OR encode(t.tgargs,'hex')<>encode(convert_to(r.argument,'UTF8'),'hex')||'00'
    ) THEN
        RAISE EXCEPTION 'ortak: Activity notification inventory differs';
    END IF;
END
$ortak_activity_inventory$;

-- Migration0059: pgschema1.7.4 omits these two table CHECK expressions.
-- Recreate them explicitly, then prove their validated live catalog presence.
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='office_inbox_reconciliations'::regclass
        AND conname='office_inbox_reconciliations_check3') THEN
        ALTER TABLE office_inbox_reconciliations ADD CONSTRAINT office_inbox_reconciliations_check3
            CHECK (cursor_created_at IS NULL OR (upper_created_at IS NOT NULL AND
                   (cursor_created_at,cursor_event_id)<=(upper_created_at,upper_event_id)));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='office_inbox_reconciliations'::regclass
        AND conname='office_inbox_reconciliations_check4') THEN
        ALTER TABLE office_inbox_reconciliations ADD CONSTRAINT office_inbox_reconciliations_check4
            CHECK (upper_created_at IS NOT NULL OR completed_at IS NOT NULL);
    END IF;
    IF (SELECT count(*) FROM pg_constraint WHERE conrelid='office_inbox_reconciliations'::regclass
        AND conname IN ('office_inbox_reconciliations_check3','office_inbox_reconciliations_check4')
        AND contype='c' AND convalidated)<>2 THEN
        RAISE EXCEPTION 'ortak: reconciliation cursor bounds are missing';
    END IF;
END $$;

-- Migration0061: retained evidence stays fenced and bound to community tombstones.
SELECT attach_community_write_fence('office_identity_profiles');
DO $$ BEGIN
    IF (SELECT count(*) FROM pg_trigger WHERE tgfoid='ortak_guard_retained_office_authority()'::regprocedure
        AND tgrelid IN ('office_identity_profiles'::regclass,'office_inbox_reconciliations'::regclass)
        AND tgname='ortak_retained_office_authority' AND tgenabled='O' AND tgtype=23)<>2
       OR (SELECT count(*) FROM pg_constraint WHERE confrelid='communities'::regclass
           AND conrelid IN ('office_identity_profiles'::regclass,'office_inbox_reconciliations'::regclass)
           AND contype='f' AND convalidated AND confdeltype='a')<>2 THEN
        RAISE EXCEPTION 'ortak: retained Office evidence authority or provenance guard missing';
    END IF;
END $$;

-- Migration0062 parity includes the existing criterion review invariants.
-- pgschema1.7.4 omits these boolean-equivalence/table CHECK expressions.
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='work_acceptance_criteria'::regclass
        AND conname='work_acceptance_criteria_check') THEN
        ALTER TABLE work_acceptance_criteria ADD CONSTRAINT work_acceptance_criteria_check
            CHECK ((status = 'satisfied') = (satisfied_at IS NOT NULL));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='work_acceptance_criteria'::regclass
        AND conname='work_acceptance_criteria_check1') THEN
        ALTER TABLE work_acceptance_criteria ADD CONSTRAINT work_acceptance_criteria_check1
            CHECK ((status = 'satisfied') = (satisfied_by_type IS NOT NULL));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conrelid='work_acceptance_criteria'::regclass
        AND conname='work_acceptance_criteria_check2') THEN
        ALTER TABLE work_acceptance_criteria ADD CONSTRAINT work_acceptance_criteria_check2
            CHECK (NOT (satisfied_by_type = 'system' AND satisfied_by_id IS NOT NULL));
    END IF;
    IF (SELECT count(*) FROM pg_constraint WHERE conrelid='work_acceptance_criteria'::regclass
        AND conname IN ('work_acceptance_criteria_check','work_acceptance_criteria_check1',
                        'work_acceptance_criteria_check2') AND contype='c' AND convalidated)<>3
       OR NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='work_acceptance_criteria'::regclass
        AND tgname='trg_work_definition_criterion_history' AND tgenabled='O' AND tgtype=17
        AND tgdeferrable AND tginitdeferred
        AND tgfoid='work_definition_criterion_history_guard()'::regprocedure) THEN
        RAISE EXCEPTION 'ortak: criterion review invariants or atomic definition guard missing';
    END IF;
END $$;

-- Migration0063: pgschema omits the initial per-project Work authority seed.
INSERT INTO work_authority_generations(company_id,project_id)
SELECT company_id,id FROM projects
ON CONFLICT(company_id,project_id) DO NOTHING;
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM projects p WHERE NOT EXISTS(
        SELECT 1 FROM work_authority_generations g WHERE g.company_id=p.company_id AND g.project_id=p.id)) THEN
        RAISE EXCEPTION 'ortak: project lacks durable Work authority generation';
    END IF;
END $$;

-- Exact64 parity exposed these pgschema1.7.4 omissions, including retained Work
-- constraints first covered by the expanded catalog. Reconcile only fixed
-- reviewed definitions and verify every existing or newly created CHECK.
DO $ortak_checks$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('employee_management_commands','employee_management_commands_configuration_required','CHECK (((action = ANY (ARRAY[''compensate''::text, ''disable''::text])) OR (configuration IS NOT NULL)))'),
        ('outbox','outbox_work_dispatch_shape','CHECK (((kind <> ''work_run_dispatch''::text) OR ((run_id IS NOT NULL) AND (employee_id IS NOT NULL) AND (routing_decision_id IS NULL))))'),
        ('projects','projects_check1','CHECK (((status = ''archived''::text) = (archived_at IS NOT NULL)))'),
        ('runtime_cancellations','runtime_cancellations_check2','CHECK (((state = ''acknowledged''::text) = (acknowledged_at IS NOT NULL)))'),
        ('runtime_work_outputs','runtime_work_outputs_check3','CHECK (((state = ''materialized''::text) = (artifact_id IS NOT NULL)))'),
        ('runtime_work_outputs','runtime_work_outputs_check4','CHECK (((state <> ''failed''::text) OR (last_error_code IS NOT NULL)))'),
        ('work_approvals','work_approvals_check','CHECK (((status <> ''pending''::text) = (resolved_at IS NOT NULL)))'),
        ('work_approvals','work_approvals_check1','CHECK (((status <> ''pending''::text) = (resolved_by_type IS NOT NULL)))'),
        ('work_approvals','work_approvals_check2','CHECK ((NOT ((resolved_by_type = ''system''::text) AND (resolved_by_id IS NOT NULL))))'),
        ('work_assignments','work_assignments_check1','CHECK (((status = ''released''::text) = (released_at IS NOT NULL)))'),
        ('work_attachments','work_attachment_artifact_shape','CHECK (((kind = ''artifact''::text) = (artifact_id IS NOT NULL)))'),
        ('work_attachments','work_attachments_check','CHECK (((kind = ''office_message''::text) = (message_id IS NOT NULL)))'),
        ('work_attachments','work_attachments_check1','CHECK (((kind = ''routing_decision''::text) = (routing_decision_id IS NOT NULL)))'),
        ('work_attachments','work_attachments_check2','CHECK (((kind = ''run''::text) = (run_id IS NOT NULL)))'),
        ('work_items','work_items_check','CHECK ((NOT ((source_message_id IS NULL) AND (source_routing_decision_id IS NOT NULL))))'),
        ('work_items','work_items_check2','CHECK (((state = ''completed''::text) = (completed_at IS NOT NULL)))'),
        ('work_items','work_items_check3','CHECK (((state = ''cancelled''::text) = (cancelled_at IS NOT NULL)))')
    ) AS required(table_name,constraint_name,definition) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass
            AND conname=selected.constraint_name) THEN
            EXECUTE format('ALTER TABLE %I ADD CONSTRAINT %I %s', selected.table_name,selected.constraint_name,selected.definition);
        END IF;
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass
            AND conname=selected.constraint_name AND contype='c' AND convalidated
            AND pg_get_constraintdef(oid,false)=selected.definition) THEN
            RAISE EXCEPTION 'ortak: reviewed Work or management CHECK mismatch';
        END IF;
    END LOOP;
END
$ortak_checks$;

-- pgschema rewrites the final whitespace in these dollar-quoted bodies.
-- Reapply the exact immutable migration definitions; the catalog probe compares
-- body bytes without normalizing them or weakening any guard.
CREATE OR REPLACE FUNCTION ortak_work_generation_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (NEW.company_id,NEW.project_id) IS DISTINCT FROM (OLD.company_id,OLD.project_id)
       OR NEW.generation<>OLD.generation+1 THEN
        RAISE EXCEPTION 'ortak: Work generation only advances' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_advance_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; project UUID;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='projects' THEN project:=NEW.id;
    ELSIF TG_TABLE_NAME IN ('work_items','project_access_grants') THEN project:=NEW.project_id;
    ELSE SELECT project_id INTO project FROM work_items WHERE company_id=company AND id=NEW.work_item_id;
    END IF;
    INSERT INTO work_authority_generations(company_id,project_id) VALUES(company,project)
    ON CONFLICT(company_id,project_id) DO UPDATE SET generation=work_authority_generations.generation+1;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_work_child_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE parent_project UUID;
BEGIN
    IF TG_TABLE_NAME='work_assignments' THEN
        IF TG_OP='UPDATE' AND (NEW.company_id,NEW.work_item_id,NEW.employee_id) IS DISTINCT FROM (OLD.company_id,OLD.work_item_id,OLD.employee_id) THEN
            RAISE EXCEPTION 'ortak: Work assignment identity is immutable' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    END IF;
    SELECT project_id INTO parent_project FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id;
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=parent_project FOR SHARE NOWAIT;
    PERFORM 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'ortak: Work authority parent is missing' USING ERRCODE='foreign_key_violation'; END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_work_execution_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW)-'reconciled_at'-'result_code') IS DISTINCT FROM (to_jsonb(OLD)-'reconciled_at'-'result_code')
       OR OLD.reconciled_at IS NOT NULL OR NEW.reconciled_at IS NULL
       OR NOT EXISTS(SELECT 1 FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('completed','failed','cancelled')) THEN
        RAISE EXCEPTION 'ortak: Work execution pins its request and only closes once' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_work_output_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id,NEW.run_id,NEW.terminal_sequence,NEW.created_at) IS DISTINCT FROM
       (OLD.company_id,OLD.run_id,OLD.terminal_sequence,OLD.created_at)
       OR NEW.attempt_count<OLD.attempt_count OR OLD.state<>'pending' THEN
        RAISE EXCEPTION 'ortak: Work output attribution is immutable and terminal state is final' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_schedule_work_output() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_type IN('run.completed','run.failed','run.cancelled') AND EXISTS(
        SELECT 1 FROM work_executions WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        INSERT INTO runtime_work_outputs(company_id,run_id,terminal_sequence) VALUES(NEW.company_id,NEW.run_id,NEW.sequence)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_check_work_execution_request() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE definition JSONB;
BEGIN
    definition:=convert_from(NEW.definition_bytes,'UTF8')::jsonb;
    IF NOT EXISTS(
        SELECT 1 FROM runs r JOIN work_items w ON w.company_id=r.company_id AND w.id=r.work_item_id
        JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=NEW.execution_version
        JOIN work_api_operations o ON o.company_id=NEW.company_id AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id
        JOIN outbox ticket ON ticket.company_id=r.company_id AND ticket.run_id=r.id AND ticket.kind='work_run_dispatch'
        JOIN work_attachments attachment ON attachment.company_id=r.company_id AND attachment.work_item_id=w.id AND attachment.run_id=r.id
        WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.work_item_id=NEW.work_item_id
        AND r.employee_id=NEW.employee_id AND r.employee_revision_id=NEW.employee_revision_id
        AND r.routing_decision_id IS NULL AND r.message_id IS NULL AND r.root_message_id IS NULL
        AND ticket.employee_id=NEW.employee_id AND ticket.routing_decision_id IS NULL
        AND w.project_id=NEW.project_id AND w.version=NEW.execution_version AND w.state='in_progress'
        AND h.event_type='work.execution_requested' AND h.actor_type='human' AND h.actor_id=NEW.requested_by
        AND h.payload->>'run_id'=NEW.run_id::text AND h.payload->>'employee_id'=NEW.employee_id
        AND o.action='mutate_work_item' AND o.project_id=NEW.project_id AND o.work_item_id=NEW.work_item_id
        AND o.result_version=NEW.execution_version
        AND o.request_hash=sha256(convert_to(format('["start_execution","%s",%s,"%s"]',NEW.work_item_id,NEW.requested_version,NEW.employee_id),'UTF8'))
        AND h.xmin::text::bigint=txid_current()%4294967296 AND o.xmin::text::bigint=txid_current()%4294967296
        AND definition->>'type'='work_item' AND definition->>'work_item_id'=w.id::text
        AND definition->>'project_id'=w.project_id::text AND definition->>'title'=w.title AND definition->>'description'=w.description
        AND definition->'acceptance_criteria'=coalesce((SELECT jsonb_agg(jsonb_build_object('id',cr.id,'text',cr.text) ORDER BY cr.position)
            FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id),'[]'::jsonb)
    ) THEN
        RAISE EXCEPTION 'ortak: Work execution requires its atomic human request, definition and run provenance'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_work_run_identity_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.work_item_id IS NOT NULL OR NEW.work_item_id IS NOT NULL) AND
        (NEW.company_id,NEW.id,NEW.work_item_id,NEW.employee_id,NEW.employee_revision_id,NEW.runtime_adapter,
         NEW.routing_decision_id,NEW.message_id,NEW.root_message_id,NEW.queued_at)
        IS DISTINCT FROM
        (OLD.company_id,OLD.id,OLD.work_item_id,OLD.employee_id,OLD.employee_revision_id,OLD.runtime_adapter,
         OLD.routing_decision_id,OLD.message_id,OLD.root_message_id,OLD.queued_at) THEN
        RAISE EXCEPTION 'ortak: Work run origin and configuration pins are immutable' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_check_run_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE current_run runs%ROWTYPE;
BEGIN
    -- INSERT can precede the one final admission UPDATE in the same transaction.
    SELECT * INTO current_run FROM runs WHERE company_id=NEW.company_id AND id=NEW.id;
    IF current_run.work_item_id IS NULL THEN
        IF current_run.work_admission_generation IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: Work admission requires Work origin' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP='UPDATE' AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
        AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM work_executions x
        JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
        JOIN projects p ON p.company_id=x.company_id AND p.id=x.project_id
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants a ON a.company_id=x.company_id AND a.project_id=x.project_id AND a.actor_pubkey=x.requested_by
        JOIN work_assignments assignment ON assignment.company_id=x.company_id AND assignment.work_item_id=x.work_item_id AND assignment.employee_id=x.employee_id
        WHERE x.company_id=current_run.company_id AND x.run_id=current_run.id AND x.work_item_id=current_run.work_item_id
        AND x.employee_id=current_run.employee_id AND x.employee_revision_id=current_run.employee_revision_id
        AND g.generation=current_run.work_admission_generation AND current_run.work_admission_token IS NOT NULL
        AND p.status='active' AND w.state='in_progress' AND w.version=x.execution_version
        AND a.role IN('owner','contributor') AND a.revoked_at IS NULL
        AND assignment.status='active' AND assignment.role IN('owner','contributor')) THEN
        RAISE EXCEPTION 'ortak: Work admission changed before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_check_work_output_provenance() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; job runtime_work_outputs%ROWTYPE; final_turn JSONB; final_text TEXT; fragments BIGINT; payload_bytes BIGINT; truncated BOOLEAN;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='artifacts' THEN run:=NEW.run_id;
    ELSE run:=NEW.run_id;
    END IF;
    SELECT * INTO job FROM runtime_work_outputs WHERE company_id=company AND run_id=run;
    IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM runs r JOIN run_events ev ON ev.company_id=r.company_id AND ev.run_id=r.id
        WHERE r.company_id=company AND r.id=run AND ev.sequence=job.terminal_sequence
        AND ((r.status='completed' AND ev.event_type='run.completed') OR (r.status='failed' AND ev.event_type='run.failed')
            OR (r.status='cancelled' AND ev.event_type='run.cancelled')))
    THEN RAISE EXCEPTION 'ortak: Work output requires canonical terminal provenance' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF job.state='materialized' THEN
        SELECT payload->'turn' INTO final_turn FROM run_events WHERE company_id=company AND run_id=run
            AND sequence<job.terminal_sequence AND event_type='assistant.delta' ORDER BY sequence DESC LIMIT 1;
        SELECT count(*),coalesce(sum(octet_length(payload::text)),0),
            bool_or(NOT coalesce(
                payload->>'event_type'='assistant.delta'
                AND jsonb_typeof(payload->'turn')='number'
                AND (payload->>'turn') ~ '^(0|[1-9][0-9]{0,9})$'
                AND (payload->>'turn')::numeric<=4294967295
                AND jsonb_typeof(payload->'delta')='object'
                AND jsonb_typeof(payload->'delta'->'text')='string'
                AND (NOT (payload->'delta' ? 'truncated') OR payload->'delta'->'truncated'='false'::jsonb)
                AND (payload->'delta'->'original_bytes' IS NULL OR payload->'delta'->'original_bytes'='null'::jsonb)
                AND (payload->'delta'->'original_sha256' IS NULL OR payload->'delta'->'original_sha256'='null'::jsonb),false))
            INTO fragments,payload_bytes,truncated FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF fragments=0 OR fragments>4096 OR payload_bytes>1048576 OR truncated THEN
            RAISE EXCEPTION 'ortak: Work artifact requires a complete bounded final turn' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        SELECT string_agg(payload->'delta'->>'text','' ORDER BY sequence) INTO final_text FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF final_text IS NULL OR btrim(final_text,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')='' OR octet_length(final_text)>32768 THEN
            RAISE EXCEPTION 'ortak: Work artifact final text is empty or oversized' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        IF NOT EXISTS(SELECT 1 FROM artifacts art
            JOIN work_executions x ON x.company_id=art.company_id AND x.run_id=art.run_id
            JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
            JOIN work_items w ON w.company_id=art.company_id AND w.id=art.work_item_id
            JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=x.execution_version+1
            JOIN work_attachments attachment ON attachment.company_id=w.company_id AND attachment.work_item_id=w.id AND attachment.artifact_id=art.id
            WHERE art.company_id=company AND art.id=job.artifact_id AND art.run_id=run AND art.terminal_sequence=job.terminal_sequence
            AND art.project_id=x.project_id AND art.work_item_id=x.work_item_id
            AND art.content_bytes=convert_to(final_text,'UTF8')
            AND art.employee_id=x.employee_id AND art.employee_revision_id=x.employee_revision_id
            AND r.status='completed' AND r.delivery_intent='silent' AND w.state='review' AND w.version=x.execution_version+1
            AND h.event_type='work.execution_result_ready' AND h.actor_type='system' AND h.actor_id IS NULL
            AND h.payload->>'artifact_id'=art.id::text AND h.payload->>'run_id'=run::text
            AND h.xmin::text::bigint=txid_current()%4294967296 AND art.xmin::text::bigint=txid_current()%4294967296
            AND w.xmin::text::bigint=txid_current()%4294967296 AND attachment.xmin::text::bigint=txid_current()%4294967296
            AND x.result_code='result_ready' AND x.reconciled_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id AND cr.status<>'pending')
            AND NOT EXISTS(SELECT 1 FROM work_approvals ap WHERE ap.company_id=w.company_id AND ap.work_item_id=w.id AND ap.status<>'pending'))
        THEN RAISE EXCEPTION 'ortak: Work deliverable and review must commit atomically without human decisions' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF TG_TABLE_NAME='artifacts' THEN
        RAISE EXCEPTION 'ortak: artifacts require their materialized Work output receipt' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_management_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP IN ('DELETE','TRUNCATE') THEN RAISE EXCEPTION 'Management history is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='prepared_employee_catalog' AND (to_jsonb(NEW)-'enabled')=(to_jsonb(OLD)-'enabled') THEN RETURN NEW; END IF;
 IF TG_TABLE_NAME='employee_management_commands' AND
   (to_jsonb(NEW)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])=
   (to_jsonb(OLD)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])
   AND (OLD.operation_id IS NULL OR NEW.operation_id IS NOT DISTINCT FROM OLD.operation_id)
   AND NEW.attempts>=OLD.attempts
   AND (OLD.status IN ('pending','running') OR NEW=OLD) THEN RETURN NEW; END IF;
 RAISE EXCEPTION 'Management selection is immutable' USING ERRCODE='check_violation';
END $$;

CREATE OR REPLACE FUNCTION ortak_management_actor_allowed(target UUID, actor_key TEXT, policy_hash BYTEA, employee TEXT, channels UUID[]) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE AS $$
DECLARE p employee_management_policies%ROWTYPE; community UUID; key_bytes BYTEA;
BEGIN
 SELECT * INTO p FROM employee_management_policies WHERE company_id=target AND public_key=actor_key FOR SHARE;
 IF NOT FOUND OR NOT p.enabled OR p.fingerprint<>policy_hash OR NOT(employee=ANY(p.employee_ids)) OR NOT(channels<@p.channel_ids) THEN RETURN false; END IF;
 SELECT b.community_id INTO community FROM office_company_bindings b JOIN companies c ON c.id=b.company_id
 JOIN communities cm ON cm.id=b.community_id WHERE b.company_id=target AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL;
 IF community IS NULL THEN RETURN false; END IF;
 key_bytes:=decode(actor_key,'hex');
 IF NOT(EXISTS(SELECT 1 FROM relay_members WHERE community_id=community AND pubkey=actor_key)
     OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND removed_at IS NULL))
   OR EXISTS(SELECT 1 FROM users WHERE community_id=community AND pubkey=key_bytes AND (deactivated_at IS NOT NULL OR agent_type IS NOT NULL OR agent_owner_pubkey IS NOT NULL))
   OR EXISTS(SELECT 1 FROM employee_office_bindings WHERE company_id=target AND public_key=key_bytes)
   OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND role='bot') THEN RETURN false; END IF;
 RETURN NOT EXISTS(SELECT 1 FROM unnest(channels) selected(id) WHERE NOT EXISTS(
   SELECT 1 FROM channels c WHERE c.community_id=community AND c.id=selected.id AND c.deleted_at IS NULL
   AND c.channel_type::text='stream' AND (c.visibility::text='open' OR EXISTS(
     SELECT 1 FROM channel_members m WHERE m.community_id=community AND m.channel_id=c.id AND m.pubkey=key_bytes AND m.removed_at IS NULL))));
END $$;

CREATE OR REPLACE FUNCTION ortak_management_guard(target UUID, command UUID, token UUID, operation UUID) RETURNS VOID
LANGUAGE plpgsql VOLATILE AS $$
DECLARE c employee_management_commands%ROWTYPE; op provisioning_operations%ROWTYPE; current_revision UUID; current_status TEXT; current_epoch BIGINT;
BEGIN
 PERFORM set_config('lock_timeout','500ms',true);
 PERFORM set_config('statement_timeout','2s',true);
 PERFORM ortak_lock_office_authority(target);
 -- Read attribution before taking policy -> command locks. Immutable columns
 -- cannot change while the policy is checked.
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command;
 IF NOT FOUND OR NOT ortak_management_actor_allowed(target,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids) THEN
   RAISE EXCEPTION 'Management authority refused' USING ERRCODE='insufficient_privilege';
 END IF;
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command FOR UPDATE;
 IF c.status<>'running' OR c.lease_token IS DISTINCT FROM token OR c.lease_expires_at<=clock_timestamp() THEN
   RAISE EXCEPTION 'Management lease refused' USING ERRCODE='insufficient_privilege';
 END IF;
 IF c.operation_id IS NULL AND c.configuration IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND employee_id=c.employee_id AND idempotency_key=c.configuration->>'operation_key';
   IF FOUND THEN
     IF op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode' OR op.dry_run THEN
       RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
     END IF;
     UPDATE employee_management_commands SET operation_id=op.id WHERE company_id=target AND id=command;
     c.operation_id:=op.id;
   END IF;
 END IF;
 IF operation IS NOT NULL AND c.operation_id IS DISTINCT FROM operation THEN
   RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
 END IF;
 IF c.operation_id IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND id=c.operation_id;
   IF NOT FOUND OR op.employee_id<>c.employee_id OR (c.action<>'compensate' AND
     (op.employee_lifecycle_epoch<>c.employee_lifecycle_epoch OR op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode'
      OR op.idempotency_key IS DISTINCT FROM c.configuration->>'operation_key' OR op.dry_run)) THEN
     RAISE EXCEPTION 'Management operation scope mismatch' USING ERRCODE='check_violation';
   END IF;
 END IF;
 IF c.action<>'compensate' THEN
   SELECT active_revision_id,status,lifecycle_epoch INTO current_revision,current_status,current_epoch FROM employees WHERE company_id=target AND id=c.employee_id FOR SHARE;
   IF coalesce(current_epoch,0)<>c.employee_lifecycle_epoch OR (current_status='disabled' AND c.action NOT IN('reenable','disable')) OR (c.action='reenable' AND current_status<>'disabled' AND NOT EXISTS(SELECT 1 FROM provisioning_operations done WHERE done.company_id=target AND done.id=c.operation_id AND done.result_revision_id=current_revision AND done.status='succeeded')) OR (current_revision IS DISTINCT FROM c.expected_revision_id AND NOT EXISTS(
     SELECT 1 FROM provisioning_operations o WHERE o.company_id=target AND o.id=c.operation_id AND o.result_revision_id=current_revision AND o.status='succeeded')) THEN
     RAISE EXCEPTION 'Management revision superseded' USING ERRCODE='check_violation';
   END IF;
 END IF;
 PERFORM set_config('ortak.management_command',command::text,true);
 PERFORM set_config('ortak.management_token',token::text,true);
END $$;

CREATE OR REPLACE FUNCTION ortak_management_operation_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE target UUID; operation UUID; selected UUID; token UUID; managed BOOLEAN;
BEGIN
 target:=NEW.company_id;
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW.id; ELSE operation:=NEW.operation_id; END IF;
 SELECT EXISTS(SELECT 1 FROM employee_management_commands c JOIN provisioning_operations o ON o.company_id=c.company_id
   AND (o.id=c.operation_id OR o.idempotency_key=c.configuration->>'operation_key')
   WHERE c.company_id=target AND o.id=operation) INTO managed;
 IF NOT managed THEN RETURN NEW; END IF;
 selected:=nullif(current_setting('ortak.management_command',true),'')::uuid;
 token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
 IF selected IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Managed operation requires its executor' USING ERRCODE='insufficient_privilege'; END IF;
 PERFORM ortak_management_guard(target,selected,token,operation);
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_guard_lifecycle_event_insert() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF pg_trigger_depth()<>2 THEN
   RAISE EXCEPTION 'Lifecycle event requires employee transition' USING ERRCODE='insufficient_privilege';
 END IF;
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_pin_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE epoch BIGINT;
BEGIN
 IF TG_OP='UPDATE' THEN
   IF NEW.employee_lifecycle_epoch IS DISTINCT FROM OLD.employee_lifecycle_epoch THEN
     RAISE EXCEPTION 'Employee lifecycle pin is immutable' USING ERRCODE='check_violation';
   END IF;
   RETURN NEW;
 END IF;
 PERFORM ortak_lock_office_authority(NEW.company_id);
 IF TG_TABLE_NAME='runs' THEN
 IF NEW.routing_decision_id IS NOT NULL THEN
   SELECT employee_lifecycle_epoch INTO epoch FROM routing_recipients WHERE company_id=NEW.company_id
     AND routing_decision_id=NEW.routing_decision_id AND employee_id=NEW.employee_id;
   IF epoch IS NULL THEN RAISE EXCEPTION 'Office lifecycle recipient missing' USING ERRCODE='check_violation'; END IF;
 END IF;
 ELSE
   SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id;
 END IF;
 IF TG_TABLE_NAME='runs' THEN
 IF epoch IS NULL THEN SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id; END IF;
 END IF;
 NEW.employee_lifecycle_epoch:=coalesce(epoch,0);
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_check_run_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.office_admission_token IS NOT DISTINCT FROM OLD.office_admission_token
    AND NEW.office_admission_generation IS NOT DISTINCT FROM OLD.office_admission_generation
    AND NEW.office_admission_valid_before IS NOT DISTINCT FROM OLD.office_admission_valid_before
    AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
    AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
 IF NOT EXISTS(SELECT 1 FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id
     AND status='active' AND lifecycle_epoch=NEW.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Employee lifecycle admission changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_check_provisioning_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE operation provisioning_operations%ROWTYPE; needs_admission BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW;
   needs_admission:=NEW.status IN('running','succeeded') AND (TG_OP='INSERT' OR OLD.status<>'succeeded');
 ELSE
   SELECT * INTO operation FROM provisioning_operations WHERE company_id=NEW.company_id AND id=NEW.operation_id;
   needs_admission:=NEW.state IN('running','succeeded') AND operation.status<>'compensating';
 END IF;
 IF needs_admission AND EXISTS(SELECT 1 FROM employees WHERE company_id=operation.company_id AND id=operation.employee_id
      AND lifecycle_epoch<>operation.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Provisioning lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_guard_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE command UUID; token UUID; selected employee_management_commands%ROWTYPE;
BEGIN
 IF NEW.lifecycle_epoch<>OLD.lifecycle_epoch THEN
   RAISE EXCEPTION 'Lifecycle epoch advances only with disable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.status='disabled' AND OLD.status<>'disabled' THEN
   NEW.lifecycle_epoch:=OLD.lifecycle_epoch+1;
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   IF command IS NOT NULL THEN
     token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
     SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
     IF selected.action IS DISTINCT FROM 'disable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
        OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
        OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch THEN
       RAISE EXCEPTION 'Disable intent changed' USING ERRCODE='insufficient_privilege';
     END IF;
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'disable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 ELSIF OLD.status='disabled' AND (NEW.status<>'disabled' OR NEW.active_revision_id IS DISTINCT FROM OLD.active_revision_id) THEN
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
   IF command IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Re-enable requires sealed activation' USING ERRCODE='insufficient_privilege'; END IF;
   SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
   IF selected.action IS DISTINCT FROM 'reenable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
      OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
      OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch
      OR NEW.status<>'active' OR NEW.active_revision_id IS NULL OR NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
      OR NOT EXISTS(SELECT 1 FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id
         AND r.created_by='provisioning:'||selected.operation_id::text AND r.xmin::text::bigint=txid_current()%4294967296) THEN
     RAISE EXCEPTION 'Re-enable intent changed' USING ERRCODE='insufficient_privilege';
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'reenable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 END IF;
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_check_lifecycle_activation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.command_id IS NOT NULL AND (NEW.command_lease_expires_at<=clock_timestamp() OR NOT EXISTS(
   SELECT 1 FROM employee_management_commands c WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id
     AND c.employee_id=NEW.employee_id AND c.action=NEW.action
     AND c.expected_revision_id IS NOT DISTINCT FROM NEW.previous_revision_id
     AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch-CASE WHEN NEW.action='disable' THEN 1 ELSE 0 END
     AND ortak_management_actor_allowed(c.company_id,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids)
     AND ((NEW.action='disable' AND c.status='succeeded' AND c.lease_token IS NULL AND c.lease_expires_at IS NULL)
       OR (NEW.action='reenable' AND c.status='running' AND c.lease_token=NEW.command_lease_token
         AND c.lease_expires_at=NEW.command_lease_expires_at)))) THEN
   RAISE EXCEPTION 'Lifecycle lease must remain valid at commit' USING ERRCODE='insufficient_privilege';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id
     AND e.lifecycle_epoch=NEW.lifecycle_epoch AND e.active_revision_id IS NOT DISTINCT FROM NEW.result_revision_id
     AND e.status=CASE WHEN NEW.action='disable' THEN 'disabled' ELSE 'active' END
     AND e.xmin::text::bigint=txid_current()%4294967296) THEN
   RAISE EXCEPTION 'Lifecycle transition must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 IF NEW.action='reenable' AND NOT EXISTS(SELECT 1 FROM employee_management_commands c
    JOIN provisioning_operations o ON o.company_id=c.company_id AND o.id=c.operation_id
    JOIN employees e ON e.company_id=c.company_id AND e.id=c.employee_id
    WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id AND c.action='reenable'
    AND c.employee_id=NEW.employee_id AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch
    AND o.status='succeeded' AND NOT o.dry_run AND o.mode='update'
    AND o.employee_lifecycle_epoch=NEW.lifecycle_epoch AND o.result_revision_id=NEW.result_revision_id
    AND e.status='active' AND e.active_revision_id=NEW.result_revision_id AND e.lifecycle_epoch=NEW.lifecycle_epoch) THEN
   RAISE EXCEPTION 'Re-enable activation must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;

-- Migration0065 exact function body.
CREATE OR REPLACE FUNCTION ortak_check_output_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='runtime_work_outputs' THEN effect:=NEW.state='materialized';
 ELSIF TG_TABLE_NAME='runtime_office_outputs' THEN effect:=NEW.state='enqueued';
 ELSIF TG_TABLE_NAME='runtime_memory_writes' THEN effect:=NEW.state='pending' AND NEW.admission_token IS NOT NULL;
   IF TG_OP='UPDATE' AND NEW.admission_token IS NOT DISTINCT FROM OLD.admission_token THEN effect:=false; END IF;
 ELSE effect:=true;
 END IF;
 IF effect AND NOT EXISTS(SELECT 1 FROM runs r JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
   WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Output lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;

-- Migrations65/66 expand parity to lifecycle, Office outputs and reviewed memory. pgschema1.7.4
-- omits these boolean CHECKs and flattens the two bounded-text conjunctions.
-- Accept only the observed equivalent form before restoring exact migration SQL.
DO $ortak_lifecycle_checks$
DECLARE selected RECORD; observed TEXT;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('employee_lifecycle_events', 'employee_lifecycle_events_check', 'CHECK (((action = ''disable''::text) OR ((command_id IS NOT NULL) AND (result_revision_id IS NOT NULL))))', NULL),
        ('runtime_memory_writes', 'runtime_memory_writes_check4', 'CHECK (((state = ''acknowledged''::text) = (receipt IS NOT NULL)))', NULL),
        ('runtime_memory_writes', 'runtime_memory_writes_check5', 'CHECK (((state = ''acknowledged''::text) = (acknowledged_at IS NOT NULL)))', NULL),
        ('runtime_memory_writes', 'runtime_memory_writes_content_check', 'CHECK ((((octet_length(content) >= 1) AND (octet_length(content) <= 32768)) AND (btrim(content) <> ''''::text)))', 'CHECK (((octet_length(content) >= 1) AND (octet_length(content) <= 32768) AND (btrim(content) <> ''''::text)))'),
        ('runtime_office_outputs', 'runtime_office_outputs_check2', 'CHECK (((state = ''enqueued''::text) = (outbox_id IS NOT NULL)))', NULL),
        ('runtime_office_outputs', 'runtime_office_outputs_check3', 'CHECK (((state = ''enqueued''::text) = (enqueued_at IS NOT NULL)))', NULL),
        ('runtime_office_outputs', 'runtime_office_outputs_check4', 'CHECK ((((draft_kind IS NULL) AND (draft_tags IS NULL) AND (draft_content IS NULL) AND (draft_created_at IS NULL) AND (source_facts IS NULL) AND (office_authority_generation IS NULL) AND (office_authority_valid_before IS NULL) AND (office_authority_token IS NULL)) OR ((draft_kind IS NOT NULL) AND (draft_tags IS NOT NULL) AND (draft_content IS NOT NULL) AND (draft_created_at IS NOT NULL) AND (source_facts IS NOT NULL) AND (office_authority_generation IS NOT NULL) AND (office_authority_token IS NOT NULL))))', NULL),
        ('runtime_office_outputs', 'runtime_office_outputs_check5', 'CHECK (((state <> ''enqueued''::text) OR (draft_kind IS NOT NULL)))', NULL),
        ('runtime_office_outputs', 'runtime_office_outputs_draft_content_check', 'CHECK ((((octet_length(draft_content) >= 1) AND (octet_length(draft_content) <= 32768)) AND (btrim(draft_content) <> ''''::text)))', 'CHECK (((octet_length(draft_content) >= 1) AND (octet_length(draft_content) <= 32768) AND (btrim(draft_content) <> ''''::text)))')
,
        ('reviewed_memory_facts', 'reviewed_memory_facts_check2', 'CHECK ((((version = 1) AND (revoked_by IS NULL) AND (revoked_at IS NULL) AND (revoke_reason IS NULL) AND (revocation_operation_id IS NULL)) OR ((version = 2) AND (revoked_by IS NOT NULL) AND (revoked_at IS NOT NULL) AND (revoked_at >= approved_at) AND (revoke_reason IS NOT NULL) AND (revocation_operation_id IS NOT NULL))))', NULL),
        ('reviewed_memory_facts', 'reviewed_memory_facts_content_check', 'CHECK ((((octet_length(content) >= 1) AND (octet_length(content) <= 4096)) AND (btrim(content) <> ''''::text) AND (regexp_replace(content, ''[
	]''::text, ''''::text, ''g''::text) !~ ''[[:cntrl:]]''::text)))', 'CHECK (((octet_length(content) >= 1) AND (octet_length(content) <= 4096) AND (btrim(content) <> ''''::text) AND (regexp_replace(content, ''[
	]''::text, ''''::text, ''g''::text) !~ ''[[:cntrl:]]''::text)))'),
        ('reviewed_memory_facts', 'reviewed_memory_facts_revoke_reason_check', 'CHECK ((((octet_length(revoke_reason) >= 1) AND (octet_length(revoke_reason) <= 512)) AND (btrim(revoke_reason) <> ''''::text) AND (revoke_reason !~ ''[[:cntrl:]]''::text)))', 'CHECK (((octet_length(revoke_reason) >= 1) AND (octet_length(revoke_reason) <= 512) AND (btrim(revoke_reason) <> ''''::text) AND (revoke_reason !~ ''[[:cntrl:]]''::text)))')
    ) AS required(table_name,constraint_name,definition,pgschema_definition) LOOP
        SELECT pg_get_constraintdef(oid,false) INTO observed FROM pg_constraint
            WHERE conrelid=selected.table_name::regclass AND conname=selected.constraint_name;
        IF observed IS NOT NULL AND observed IS DISTINCT FROM selected.definition THEN
            IF selected.pgschema_definition IS NULL OR observed IS DISTINCT FROM selected.pgschema_definition THEN
                RAISE EXCEPTION 'ortak: reviewed lifecycle or output CHECK mismatch';
            END IF;
            EXECUTE format('ALTER TABLE %I DROP CONSTRAINT %I',selected.table_name,selected.constraint_name);
            observed:=NULL;
        END IF;
        IF observed IS NULL THEN
            -- BETWEEN retains the migration parse-tree shape; re-parsing the
            -- catalog's rendered conjunction would flatten it again.
            IF selected.table_name='runtime_memory_writes' AND selected.constraint_name='runtime_memory_writes_content_check' THEN
                ALTER TABLE runtime_memory_writes ADD CONSTRAINT runtime_memory_writes_content_check
                    CHECK(octet_length(content) BETWEEN 1 AND 32768 AND btrim(content)<>'');
            ELSIF selected.table_name='runtime_office_outputs' AND selected.constraint_name='runtime_office_outputs_draft_content_check' THEN
                ALTER TABLE runtime_office_outputs ADD CONSTRAINT runtime_office_outputs_draft_content_check
                    CHECK(octet_length(draft_content) BETWEEN 1 AND 32768 AND btrim(draft_content)<>'');
            ELSIF selected.table_name='reviewed_memory_facts' AND selected.constraint_name='reviewed_memory_facts_content_check' THEN
                ALTER TABLE reviewed_memory_facts ADD CONSTRAINT reviewed_memory_facts_content_check
                    CHECK(octet_length(content) BETWEEN 1 AND 4096 AND btrim(content)<>''
                        AND regexp_replace(content,E'[\n\t]','','g') !~ '[[:cntrl:]]');
            ELSIF selected.table_name='reviewed_memory_facts' AND selected.constraint_name='reviewed_memory_facts_revoke_reason_check' THEN
                ALTER TABLE reviewed_memory_facts ADD CONSTRAINT reviewed_memory_facts_revoke_reason_check
                    CHECK(octet_length(revoke_reason) BETWEEN 1 AND 512 AND btrim(revoke_reason)<>'' AND revoke_reason !~ '[[:cntrl:]]');
            ELSE
                EXECUTE format('ALTER TABLE %I ADD CONSTRAINT %I %s',selected.table_name,selected.constraint_name,selected.definition);
            END IF;
        END IF;
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass
            AND conname=selected.constraint_name AND contype='c' AND convalidated
            AND pg_get_constraintdef(oid,false)=selected.definition) THEN
            RAISE EXCEPTION 'ortak: lifecycle or output CHECK reconciliation failed';
        END IF;
    END LOOP;
END
$ortak_lifecycle_checks$;

-- Exact reviewed-memory bodies from immutable migration0066.
CREATE OR REPLACE FUNCTION ortak_reviewed_fact_source_visible(
    company UUID, project UUID, employee TEXT, message BYTEA, artifact UUID,
    community UUID, channel UUID
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT (message IS NOT NULL AND EXISTS(
        SELECT 1 FROM office_inbox i
        JOIN events e ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
            AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
        WHERE i.company_id=company AND i.event_id=message AND i.channel_id=channel
            AND i.state='decided' AND e.kind IN(9,40002) AND e.deleted_at IS NULL))
    OR (artifact IS NOT NULL AND EXISTS(
        SELECT 1 FROM artifacts a
        JOIN work_items w ON w.company_id=a.company_id AND w.id=a.work_item_id AND w.project_id=a.project_id
        WHERE a.company_id=company AND a.id=artifact AND a.project_id=project AND a.employee_id=employee
            AND (w.source_message_id IS NULL OR EXISTS(
                SELECT 1 FROM office_inbox i
                JOIN events e ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
                    AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
                WHERE i.company_id=company AND i.event_id=w.source_message_id AND i.channel_id=channel
                    AND i.state='decided' AND e.kind IN(9,40002) AND e.deleted_at IS NULL))))
$$;
CREATE OR REPLACE FUNCTION ortak_reviewed_fact_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE channel UUID;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF OLD.version<>1 OR NEW.version<>2
            OR (to_jsonb(NEW)-'version'-'revoked_by'-'revoked_at'-'revoke_reason'-'revocation_operation_id') IS DISTINCT FROM
               (to_jsonb(OLD)-'version'-'revoked_by'-'revoked_at'-'revoke_reason'-'revocation_operation_id') THEN
            RAISE EXCEPTION 'ortak: reviewed fact only permits one retained revocation' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id FOR SHARE;
    SELECT b.channel_id INTO channel FROM project_api_bindings b
        JOIN projects p ON p.company_id=b.company_id AND p.id=b.project_id
        JOIN employees e ON e.company_id=b.company_id AND e.id=NEW.employee_id
        WHERE b.company_id=NEW.company_id AND b.project_id=NEW.project_id AND b.community_id=NEW.community_id
            AND p.status='active' AND e.status='active';
    IF channel IS NULL OR NEW.version<>1 OR NEW.approved_at>clock_timestamp() OR NEW.expires_at<=clock_timestamp()
        OR NOT ortak_reviewed_fact_source_visible(NEW.company_id,NEW.project_id,NEW.employee_id,
            NEW.source_message_id,NEW.source_artifact_id,NEW.community_id,channel) THEN
        RAISE EXCEPTION 'ortak: reviewed fact requires current scoped evidence and approval' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- At most 1024 retained approvals per exact audience; revoked evidence counts.
    PERFORM pg_advisory_xact_lock(hashtextextended(format('ortak-reviewed-memory-scope:%s:%s:%s',
        NEW.company_id,NEW.project_id,NEW.employee_id),0));
    IF (SELECT count(*) FROM reviewed_memory_facts WHERE company_id=NEW.company_id AND project_id=NEW.project_id
        AND employee_id=NEW.employee_id)>=1024 THEN
        RAISE EXCEPTION 'ortak: reviewed memory scope is full' USING ERRCODE='program_limit_exceeded';
    END IF;
    RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION ortak_reviewed_fact_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE actor TEXT; operation UUID; expected_action TEXT;
BEGIN
    actor:=CASE WHEN TG_OP='INSERT' THEN NEW.approved_by ELSE NEW.revoked_by END;
    operation:=CASE WHEN TG_OP='INSERT' THEN NEW.promotion_operation_id ELSE NEW.revocation_operation_id END;
    expected_action:=CASE WHEN TG_OP='INSERT' THEN 'promote' ELSE 'revoke' END;
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_operations o
        WHERE o.company_id=NEW.company_id AND o.community_id=NEW.community_id AND o.actor_pubkey=actor
            AND o.operation_id=operation AND o.action=expected_action AND o.fact_id=NEW.id
            AND o.project_id=NEW.project_id AND o.result_version=NEW.version
            AND o.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed fact transition requires an atomic receipt' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION ortak_reviewed_memory_operation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND clock_timestamp()>=NEW.valid_before THEN
        RAISE EXCEPTION 'ortak: reviewed memory authority expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f WHERE f.company_id=NEW.company_id
        AND f.community_id=NEW.community_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id
        AND f.xmin::text::bigint=txid_current()%4294967296
        AND ((NEW.action='promote' AND f.approved_by=NEW.actor_pubkey AND f.promotion_operation_id=NEW.operation_id)
            OR (NEW.action='revoke' AND f.revoked_by=NEW.actor_pubkey AND f.revocation_operation_id=NEW.operation_id))) THEN
        RAISE EXCEPTION 'ortak: reviewed memory receipt requires its atomic fact transition' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;


-- E4 retained dependency edits use the exact migration67 function body.
CREATE OR REPLACE FUNCTION ortak_work_dependency_edit_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE source_state TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'released_at') IS DISTINCT FROM (to_jsonb(OLD)-'released_at')
            OR (OLD.released_at IS NULL)=(NEW.released_at IS NULL)
            OR NEW.released_at>clock_timestamp() THEN
            RAISE EXCEPTION 'ortak: dependency only permits retained release or reactivation'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF NEW.released_at IS NOT NULL THEN
        RAISE EXCEPTION 'ortak: dependency must be created active'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- Direct writers must not invert the graph's project-before-item lock order.
    -- Ordinary commands already hold EXCLUSIVE before reading current authority.
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        AND status='active' FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: dependency project is unavailable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT state INTO source_state FROM work_items
        WHERE company_id=NEW.company_id AND project_id=NEW.project_id AND id=NEW.work_item_id
        FOR UPDATE NOWAIT;
    IF source_state IS NULL OR source_state IN('completed','cancelled') THEN
        RAISE EXCEPTION 'ortak: dependency source is immutable or missing'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;



-- Keep the exact migration68 function body after pgschema normalization.
-- pgschema1.7.4 omits this second state/NULL equivalence while retaining the
-- running/contained_at equivalence. Converge it and assert its real catalog.
DO $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_constraint
        WHERE conrelid='provisioning_runtime_probes'::regclass
          AND conname='provisioning_runtime_probes_check3') THEN
        ALTER TABLE provisioning_runtime_probes ADD CONSTRAINT provisioning_runtime_probes_check3
            CHECK((state='failed')=(error_code IS NOT NULL));
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint
        WHERE conrelid='provisioning_runtime_probes'::regclass
          AND conname='provisioning_runtime_probes_check3' AND contype='c'
          AND convalidated AND NOT condeferrable AND NOT condeferred
          AND pg_get_constraintdef(oid,false)='CHECK (((state = ''failed''::text) = (error_code IS NOT NULL)))') THEN
        RAISE EXCEPTION 'Runtime probe terminal failure constraint differs from migration68';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION ortak_provisioning_runtime_probe_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE op provisioning_operations%ROWTYPE; prior INTEGER; epoch BIGINT; employee_status TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-ARRAY['state','contained_at','error_code']) IS DISTINCT FROM
           (to_jsonb(OLD)-ARRAY['state','contained_at','error_code'])
           OR OLD.state<>'running' OR NEW.state='running' OR NEW.contained_at>clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe only permits one contained terminal receipt'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Revoked/expired operations can still retain successful cleanup. They
        -- cannot turn that cleanup into a current readiness witness.
        IF NEW.state='failed' THEN RETURN NEW; END IF;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NOT EXISTS(SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id WHERE c.id=NEW.company_id AND c.status='active'
        AND cm.deletion_state='active' AND cm.deleted_at IS NULL) THEN
        RAISE EXCEPTION 'Runtime probe Office authority unavailable' USING ERRCODE='insufficient_privilege';
    END IF;
    SELECT * INTO op FROM provisioning_operations
        WHERE company_id=NEW.company_id AND id=NEW.operation_id FOR UPDATE NOWAIT;
    IF NOT FOUND OR op.employee_id<>NEW.employee_id OR op.dry_run
       OR op.status NOT IN('pending','running','failed')
       OR op.manifest->>'provisioning' IS DISTINCT FROM 'adopt'
       OR op.manifest#>>'{employee,runtime,adapter}' IS DISTINCT FROM 'hermes' THEN
        RAISE EXCEPTION 'Runtime probe operation unavailable' USING ERRCODE='check_violation';
    END IF;
    SELECT lifecycle_epoch,status INTO epoch,employee_status FROM employees
        WHERE company_id=NEW.company_id AND id=NEW.employee_id FOR SHARE;
    IF op.employee_lifecycle_epoch<>coalesce(epoch,0) OR (employee_status='disabled' AND NOT EXISTS(
        SELECT 1 FROM employee_management_commands c JOIN employees e
          ON e.company_id=c.company_id AND e.id=c.employee_id
        WHERE c.company_id=NEW.company_id AND c.id=nullif(current_setting('ortak.management_command',true),'')::uuid
          AND c.operation_id=op.id AND c.action='reenable' AND c.employee_lifecycle_epoch=e.lifecycle_epoch
          AND c.expected_revision_id IS NOT DISTINCT FROM e.active_revision_id)) THEN
        RAISE EXCEPTION 'Runtime probe lifecycle changed' USING ERRCODE='serialization_failure';
    END IF;
    IF TG_OP='INSERT' AND TG_WHEN='BEFORE' THEN
        SELECT coalesce(max(generation),0) INTO prior FROM provisioning_runtime_probes
            WHERE company_id=NEW.company_id AND operation_id=NEW.operation_id;
        IF NEW.generation<>prior+1 OR NEW.state<>'running' OR NEW.contained_at IS NOT NULL
           OR NEW.error_code IS NOT NULL OR NEW.created_at>clock_timestamp() OR NEW.deadline<=clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe admission is not the next bounded attempt' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.deadline<=clock_timestamp() THEN
        RAISE EXCEPTION 'Runtime probe readiness expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;


-- D2b migration69 exact function bodies and dynamic retained-table triggers.
-- pgschema1.7.4 drops this Boolean/NULL equivalence, as it does for probe68.
DO $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_constraint
        WHERE conrelid='reviewed_memory_export_receipts'::regclass
          AND conname='reviewed_memory_export_receipts_check') THEN
        ALTER TABLE reviewed_memory_export_receipts ADD CONSTRAINT reviewed_memory_export_receipts_check
            CHECK(erased_from_reviewed_store=(tombstone_at IS NOT NULL));
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint
        WHERE conrelid='reviewed_memory_export_receipts'::regclass
          AND conname='reviewed_memory_export_receipts_check' AND contype='c'
          AND convalidated AND NOT condeferrable AND NOT condeferred
          AND pg_get_constraintdef(oid,false)='CHECK ((erased_from_reviewed_store = (tombstone_at IS NOT NULL)))') THEN
        RAISE EXCEPTION 'Reviewed removal evidence constraint differs from migration69';
    END IF;
END $$;



CREATE OR REPLACE FUNCTION ortak_reviewed_export_eligible(company UUID, fact UUID, target UUID) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
        JOIN companies c ON c.id=f.company_id
        JOIN communities cm ON cm.id=f.community_id
        JOIN office_company_bindings ob ON ob.company_id=f.company_id AND ob.community_id=f.community_id
        JOIN project_api_bindings b ON b.company_id=f.company_id AND b.project_id=f.project_id AND b.community_id=f.community_id
        JOIN projects p ON p.company_id=f.company_id AND p.id=f.project_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=e.active_revision_id
        JOIN employee_office_bindings eb ON eb.company_id=e.company_id AND eb.employee_id=e.id
        JOIN channel_members m ON m.community_id=f.community_id AND m.channel_id=b.channel_id AND m.pubkey=eb.public_key AND m.removed_at IS NULL
        WHERE f.company_id=company AND f.id=fact AND f.audience_kind='project' AND t.id=target AND f.version=1 AND f.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND p.status='active' AND e.status='active'
          AND t.enabled AND t.valid_until>clock_timestamp() AND t.community_id=f.community_id
          AND t.employee_revision_id=e.active_revision_id AND t.employee_lifecycle_epoch=e.lifecycle_epoch
          AND t.binding=r.manifest->'memory' AND mb.validated_at IS NOT NULL
          AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
          AND eb.verified_at IS NOT NULL AND eb.valid_from<=clock_timestamp() AND (eb.valid_until IS NULL OR eb.valid_until>clock_timestamp())
          AND encode(eb.public_key,'hex')=r.manifest#>>'{office,public_key}' AND eb.signer_ref=r.manifest#>>'{office,signer_ref}'
          AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=f.community_id AND u.pubkey=eb.public_key AND u.deactivated_at IS NOT NULL)
          AND ortak_reviewed_fact_source_visible(f.company_id,f.project_id,f.employee_id,f.source_message_id,f.source_artifact_id,f.community_id,b.channel_id))
$$;

CREATE OR REPLACE FUNCTION ortak_reviewed_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selecting BOOLEAN;
BEGIN
    IF TG_OP='UPDATE' AND
        (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch')
        IS DISTINCT FROM
        (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch') THEN
        RAISE EXCEPTION 'ortak: reviewed target identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.consumption_epoch<>0 OR NEW.conversation_consumption_epoch<>0 THEN
            RAISE EXCEPTION 'ortak: invalid initial consumption epoch' USING ERRCODE='check_violation';
        END IF;
        selecting=NEW.conversation_channel_id IS NOT NULL;
    ELSE
        IF NEW.consumption_epoch<>OLD.consumption_epoch
            OR NEW.conversation_consumption_epoch<>OLD.conversation_consumption_epoch THEN
            RAISE EXCEPTION 'ortak: consumption epochs are server derived' USING ERRCODE='check_violation';
        END IF;
        IF OLD.conversation_channel_id IS NOT NULL
            AND NEW.conversation_channel_id IS DISTINCT FROM OLD.conversation_channel_id THEN
            RAISE EXCEPTION 'ortak: conversation target channel is immutable' USING ERRCODE='check_violation';
        END IF;
        IF OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled THEN
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
        IF OLD.conversation_consumption_enabled AND NOT NEW.conversation_consumption_enabled THEN
            NEW.conversation_consumption_epoch=OLD.conversation_consumption_epoch+1;
        END IF;
        selecting=OLD.conversation_channel_id IS NULL AND NEW.conversation_channel_id IS NOT NULL;
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp() OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds') THEN
        RAISE EXCEPTION 'ortak: reviewed target witness must be short and live' USING ERRCODE='check_violation';
    END IF;
    -- A disable-only advertisement must still work after source/identity loss.
    -- In particular the existing advertise transaction briefly sets enabled=false
    -- before refreshing its selected rows. That is not conversation opt-out and
    -- must not advance its separate epoch or fail a stale-scope current check.
    IF selecting OR (NEW.conversation_consumption_enabled AND NEW.enabled) THEN
        PERFORM ortak_lock_office_authority(NEW.company_id);
        PERFORM 1 FROM projects p WHERE p.company_id=NEW.company_id AND p.id=NEW.project_id FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities authority
            WHERE authority.company_id=NEW.company_id AND authority.community_id=NEW.community_id
                AND authority.project_id=NEW.project_id AND authority.channel_id=NEW.conversation_channel_id FOR SHARE;
        IF NOT FOUND OR NOT ortak_conversation_scope_current(
                NEW.company_id,NEW.community_id,NEW.project_id,NEW.conversation_channel_id)
            OR NOT EXISTS (SELECT 1 FROM employees e
                JOIN employee_revisions rev ON rev.company_id=e.company_id AND rev.employee_id=e.id AND rev.id=e.active_revision_id
                JOIN employee_memory_bindings memory ON memory.company_id=e.company_id AND memory.employee_id=e.id AND memory.revision_id=e.active_revision_id
                WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id AND e.status='active'
                    AND NEW.employee_revision_id=e.active_revision_id AND NEW.employee_lifecycle_epoch=e.lifecycle_epoch
                    AND NEW.binding=rev.manifest->'memory' AND memory.validated_at IS NOT NULL
                    AND NEW.binding=jsonb_build_object('adapter',memory.adapter,'endpoint_ref',memory.endpoint_ref,
                        'workspace',memory.workspace,'user_peer',memory.user_peer,'employee_peer',memory.employee_peer,'options',memory.options))
            OR (NEW.conversation_consumption_enabled AND (NOT NEW.enabled OR NEW.valid_until<=clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: conversation target requires current selected scope and binding'
                USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;



CREATE OR REPLACE FUNCTION ortak_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE allowed BOOLEAN:=false;
BEGIN
    IF (NEW.company_id,NEW.community_id,NEW.fact_id,NEW.action,NEW.idempotency_key,NEW.request_hash)
        IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.fact_id,OLD.action,OLD.idempotency_key,OLD.request_hash)
        OR OLD.state='acknowledged' OR NEW.total_attempts<OLD.total_attempts OR NEW.total_attempts>OLD.total_attempts+1
        OR NEW.retry_version<OLD.retry_version OR NEW.retry_version>OLD.retry_version+1 THEN
        RAISE EXCEPTION 'ortak: reviewed job identity and progress are retained' USING ERRCODE='check_violation';
    END IF;
    IF NEW.retry_version=OLD.retry_version+1 THEN
        allowed:=OLD.state='failed' AND OLD.lease_token IS NULL AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=OLD.total_attempts AND NEW.lease_token IS NULL AND NEW.last_error_code IS NULL
            AND NEW.next_attempt_at<=clock_timestamp();
    ELSIF NEW.attempt_count=OLD.attempt_count+1 AND NEW.total_attempts=OLD.total_attempts+1 THEN
        allowed:=OLD.state='pending' AND NEW.state='pending' AND OLD.next_attempt_at<=clock_timestamp()
            AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
            AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
            AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=clock_timestamp()+INTERVAL '60 seconds'
            AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code;
    ELSIF NEW.attempt_count=OLD.attempt_count AND NEW.total_attempts=OLD.total_attempts AND OLD.state='pending' THEN
        IF NEW.state='acknowledged' THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.lease_token=OLD.lease_token AND NEW.lease_expires_at=OLD.lease_expires_at
                AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NULL;
        ELSIF NEW.state='failed' AND NEW.last_error_code='lease_exhausted' THEN
            allowed:=OLD.attempt_count=20 AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
                AND NEW.lease_token IS NULL AND NEW.next_attempt_at=OLD.next_attempt_at;
        ELSIF NEW.state='pending' AND NEW.action='withdraw' AND NEW.next_attempt_at<=OLD.next_attempt_at THEN
            allowed:=(NEW.lease_token,NEW.lease_expires_at,NEW.last_error_code)
                IS NOT DISTINCT FROM (OLD.lease_token,OLD.lease_expires_at,OLD.last_error_code)
                AND EXISTS(SELECT 1 FROM reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
        ELSIF NEW.lease_token IS NULL AND NEW.last_error_code IS NOT NULL THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.next_attempt_at>clock_timestamp() AND NEW.next_attempt_at<=clock_timestamp()+INTERVAL '301 seconds'
                AND (NEW.state='failed' OR NEW.state='pending' AND OLD.attempt_count<20);
        END IF;
    END IF;
    IF NOT coalesce(allowed,false) THEN
        RAISE EXCEPTION 'ortak: reviewed job transition lacks a due claim, live lease, stop or audited retry' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='reviewed:'||NEW.action||':'||NEW.fact_id::text
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND NEW.valid_before<=clock_timestamp() THEN
        RAISE EXCEPTION 'ortak: reviewed command authority expired' USING ERRCODE='serialization_failure';
    END IF;
    IF (NEW.action='publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
        AND x.operation_id=NEW.operation_id AND x.requested_by=NEW.actor_pubkey AND NEW.result_version=0 AND x.xmin::text::bigint=txid_current()%4294967296))
        OR (NEW.action<>'publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
            AND 'retry_'||j.action=NEW.action AND j.retry_version=NEW.result_version AND j.xmin::text::bigint=txid_current()%4294967296)) THEN
        RAISE EXCEPTION 'ortak: reviewed command requires its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j
        JOIN reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;



DO $$ DECLARE relation TEXT; BEGIN
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_targets','reviewed_memory_exports','reviewed_memory_export_jobs','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=relation::regclass AND tgname='reviewed_export_no_delete') THEN
            EXECUTE format('CREATE TRIGGER reviewed_export_no_delete BEFORE DELETE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        END IF;
        IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=relation::regclass AND tgname='reviewed_export_no_truncate') THEN
            EXECUTE format('CREATE TRIGGER reviewed_export_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()',relation);
        END IF;
        PERFORM attach_community_write_fence(relation);
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_exports','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=relation::regclass AND tgname='reviewed_export_immutable') THEN
            EXECUTE format('CREATE TRIGGER reviewed_export_immutable BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        END IF;
    END LOOP;
END $$;


-- Migrations70/71: exact function bodies after pgschema dependency ordering and
-- dollar-quote normalization. No source or credential data is seeded here.
CREATE OR REPLACE FUNCTION ortak_work_decomposition_reserve() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE parent work_items%ROWTYPE; parent_depth SMALLINT; children INTEGER;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        AND status='active' FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: decomposition project is unavailable' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT * INTO parent FROM work_items WHERE company_id=NEW.company_id
        AND project_id=NEW.project_id AND id=NEW.parent_id FOR UPDATE NOWAIT;
    IF NOT FOUND OR parent.state IN('completed','cancelled') OR parent.version+1<>NEW.parent_version
        OR EXISTS(SELECT 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.child_id) THEN
        RAISE EXCEPTION 'ortak: decomposition requires a mutable parent and a fresh child'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT coalesce((SELECT depth FROM work_decomposition
        WHERE company_id=NEW.company_id AND child_id=NEW.parent_id),0) INTO parent_depth;
    SELECT count(*) INTO children FROM work_decomposition
        WHERE company_id=NEW.company_id AND parent_id=NEW.parent_id;
    IF NEW.depth<>parent_depth+1 OR children>=32 OR NEW.created_at<>now() THEN
        RAISE EXCEPTION 'ortak: decomposition bound or provenance differs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_work_decomposition_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM work_items parent JOIN work_items child
          ON child.company_id=parent.company_id AND child.project_id=parent.project_id
        JOIN work_item_history ph ON ph.company_id=parent.company_id AND ph.work_item_id=parent.id
          AND ph.version=NEW.parent_version AND ph.sequence=NEW.parent_version-1
        JOIN work_item_history ch ON ch.company_id=child.company_id AND ch.work_item_id=child.id
          AND ch.version=1 AND ch.sequence=0
        JOIN work_api_operations receipt ON receipt.company_id=NEW.company_id
          AND receipt.actor_pubkey=NEW.actor_pubkey AND receipt.operation_id=NEW.operation_id
        WHERE parent.company_id=NEW.company_id AND parent.project_id=NEW.project_id
          AND parent.id=NEW.parent_id AND parent.version=NEW.parent_version
          AND parent.state NOT IN('completed','cancelled')
          AND child.id=NEW.child_id AND child.version=1 AND child.state='proposed'
          AND child.source_message_id IS NULL AND child.source_routing_decision_id IS NULL
          AND child.created_by_type='human' AND child.created_by_id=NEW.actor_pubkey
          AND child.created_at=NEW.created_at
          AND ph.event_type='work.child_created' AND ph.actor_type='human' AND ph.actor_id=NEW.actor_pubkey
          AND ph.payload=jsonb_build_object('event','child_created','child_id',NEW.child_id)
          AND ch.event_type='work.created' AND ch.actor_type='human' AND ch.actor_id=NEW.actor_pubkey
          AND receipt.action='create_work_item' AND receipt.project_id=NEW.project_id
          AND receipt.work_item_id=NEW.child_id AND receipt.result_version=1
          AND (receipt.valid_before IS NULL OR receipt.valid_before>clock_timestamp())
    ) OR EXISTS(SELECT 1 FROM work_assignments WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_dependencies WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_attachments WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_acceptance_criteria WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id AND status<>'pending')
      OR EXISTS(SELECT 1 FROM work_approvals WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id AND status<>'pending') THEN
        RAISE EXCEPTION 'ortak: decomposition must commit independent creation and parent history atomically'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_runtime_eligible(company UUID, fact UUID, target UUID, epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        JOIN reviewed_memory_export_receipts ack ON ack.company_id=x.company_id AND ack.fact_id=x.fact_id AND ack.action='publish'
        JOIN companies c ON c.id=f.company_id JOIN communities cm ON cm.id=f.community_id
        JOIN office_company_bindings ob ON ob.company_id=f.company_id AND ob.community_id=f.community_id
        JOIN projects p ON p.company_id=f.company_id AND p.id=f.project_id
        JOIN project_api_bindings b ON b.company_id=f.company_id AND b.project_id=f.project_id AND b.community_id=f.community_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=e.active_revision_id
        JOIN employee_office_bindings eb ON eb.company_id=e.company_id AND eb.employee_id=e.id
        JOIN channel_members m ON m.community_id=f.community_id AND m.channel_id=b.channel_id AND m.pubkey=eb.public_key AND m.removed_at IS NULL
        WHERE f.company_id=company AND f.id=fact AND f.audience_kind='project' AND t.id=target AND t.consumption_epoch=epoch
          AND f.version=1 AND f.revoked_at IS NULL AND f.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND p.status='active' AND e.status='active'
          AND t.enabled AND t.runtime_consumption_enabled AND t.valid_until>clock_timestamp()
          AND t.company_id=f.company_id AND t.community_id=f.community_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
          AND t.binding=r.manifest->'memory' AND mb.validated_at IS NOT NULL
          AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
          AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store AND ack.binding_hash=t.binding_hash
          AND ack.content_hash=x.content_hash AND x.content_hash=sha256(convert_to(f.content,'UTF8'))
          AND x.source_hash=ortak_reviewed_export_source_hash(f)
          AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw')
          AND eb.verified_at IS NOT NULL AND eb.valid_from<=clock_timestamp() AND (eb.valid_until IS NULL OR eb.valid_until>clock_timestamp())
          AND encode(eb.public_key,'hex')=r.manifest#>>'{office,public_key}' AND eb.signer_ref=r.manifest#>>'{office,signer_ref}'
          AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=f.community_id AND u.pubkey=eb.public_key AND u.deactivated_at IS NOT NULL)
          AND ortak_reviewed_fact_source_visible(f.company_id,f.project_id,f.employee_id,f.source_message_id,f.source_artifact_id,f.community_id,b.channel_id))
$$;





CREATE OR REPLACE FUNCTION ortak_reviewed_use_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'ortak: reviewed run uses are retained and immutable' USING ERRCODE='check_violation'; END $$;

CREATE OR REPLACE FUNCTION ortak_snapshot_scratch_jsonb(value JSON) RETURNS JSONB
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $function$
    SELECT regexp_replace(
        regexp_replace(value::text,
            $pattern$(?<!\\)((?:\\\\)*)\\u0001$pattern$,
            $replacement$\1\\u0001\\u0001$replacement$,'g'),
        $pattern$(?<!\\)((?:\\\\)*)\\u0000$pattern$,
        $replacement$\1\\u0001\\u0002$replacement$,'g')::jsonb
$function$;





DO $ortak_70_71_triggers$
BEGIN
    PERFORM attach_community_write_fence('run_reviewed_memory_uses');
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='work_decomposition'::regclass AND tgname='work_decomposition_reserve') THEN
        EXECUTE $ddl$CREATE TRIGGER work_decomposition_reserve BEFORE INSERT ON work_decomposition
    FOR EACH ROW EXECUTE FUNCTION ortak_work_decomposition_reserve();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='work_decomposition'::regclass AND tgname='work_decomposition_immutable') THEN
        EXECUTE $ddl$CREATE TRIGGER work_decomposition_immutable BEFORE UPDATE OR DELETE ON work_decomposition
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='work_decomposition'::regclass AND tgname='work_decomposition_no_truncate') THEN
        EXECUTE $ddl$CREATE TRIGGER work_decomposition_no_truncate BEFORE TRUNCATE ON work_decomposition
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='work_decomposition'::regclass AND tgname='work_decomposition_at_commit') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER work_decomposition_at_commit AFTER INSERT ON work_decomposition
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_work_decomposition_commit();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='run_reviewed_memory_uses'::regclass AND tgname='ortak_reviewed_use_immutable') THEN
        EXECUTE $ddl$CREATE TRIGGER ortak_reviewed_use_immutable BEFORE UPDATE OR DELETE ON run_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_use_immutable();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='run_reviewed_memory_uses'::regclass AND tgname='ortak_reviewed_use_no_truncate') THEN
        EXECUTE $ddl$CREATE TRIGGER ortak_reviewed_use_no_truncate BEFORE TRUNCATE ON run_reviewed_memory_uses
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reviewed_use_immutable();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='run_context_snapshots'::regclass AND tgname='ortak_reviewed_snapshot_consistent') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER ortak_reviewed_snapshot_consistent AFTER INSERT ON run_context_snapshots
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='run_reviewed_memory_uses'::regclass AND tgname='ortak_reviewed_use_consistent') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER ortak_reviewed_use_consistent AFTER INSERT ON run_reviewed_memory_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='runs'::regclass AND tgname='ortak_reviewed_run_admission') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER ortak_reviewed_run_admission AFTER UPDATE ON runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='artifacts'::regclass AND tgname='ortak_reviewed_artifact_admission') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER ortak_reviewed_artifact_admission AFTER INSERT ON artifacts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_facts'::regclass AND tgname='trg_activity_reviewed_fact_use') THEN
        EXECUTE $ddl$CREATE TRIGGER trg_activity_reviewed_fact_use AFTER UPDATE OF version ON reviewed_memory_facts
    FOR EACH ROW WHEN(NEW.version IS DISTINCT FROM OLD.version) EXECUTE FUNCTION ortak_activity_notify('');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_targets'::regclass AND tgname='trg_activity_reviewed_target_use') THEN
        EXECUTE $ddl$CREATE TRIGGER trg_activity_reviewed_target_use AFTER UPDATE ON reviewed_memory_targets
    FOR EACH ROW WHEN(NEW.consumption_epoch IS DISTINCT FROM OLD.consumption_epoch) EXECUTE FUNCTION ortak_activity_notify('');$ddl$;
    END IF;
END
$ortak_70_71_triggers$;

-- Every70/71 bound must be present and validated on the real catalog.
DO $ortak_70_71_checks$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('work_decomposition','work_decomposition_child_id_check','CHECK ((child_id <> ''00000000-0000-0000-0000-000000000000''::uuid))'),
        ('work_decomposition','work_decomposition_parent_version_check','CHECK ((parent_version > 1))'),
        ('work_decomposition','work_decomposition_depth_check','CHECK (((depth >= 1) AND (depth <= 8)))'),
        ('work_decomposition','work_decomposition_actor_pubkey_check','CHECK ((actor_pubkey ~ ''^[0-9a-f]{64}$''::text))'),
        ('work_decomposition','work_decomposition_check','CHECK ((parent_id <> child_id))'),
        ('reviewed_memory_targets','reviewed_memory_targets_consumption_epoch_check','CHECK ((consumption_epoch >= 0))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_ordinal_check','CHECK (((ordinal >= 0) AND (ordinal <= 7)))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_fact_version_check','CHECK ((fact_version = 1))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_consumption_epoch_check','CHECK ((consumption_epoch >= 0))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_content_hash_check','CHECK ((octet_length(content_hash) = 32))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_source_hash_check','CHECK ((octet_length(source_hash) = 32))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_binding_hash_check','CHECK ((octet_length(binding_hash) = 32))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_approved_by_check','CHECK ((approved_by ~ ''^[0-9a-f]{64}$''::text))')
    ) AS required(table_name,constraint_name,definition) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass AND conname=selected.constraint_name) THEN
            EXECUTE format('ALTER TABLE %I ADD CONSTRAINT %I %s',selected.table_name,selected.constraint_name,selected.definition);
        END IF;
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass AND conname=selected.constraint_name
            AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred AND pg_get_constraintdef(oid,false)=selected.definition) THEN
            RAISE EXCEPTION 'ortak: decomposition or reviewed use CHECK mismatch';
        END IF;
    END LOOP;
END
$ortak_70_71_checks$;

-- A pre-existing trigger name alone is not proof of enforcement.
DO $ortak_70_71_guards$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('work_decomposition','work_decomposition_reserve','ortak_work_decomposition_reserve',7,false),
        ('work_decomposition','work_decomposition_immutable','ortak_reject_row_mutation',27,false),
        ('work_decomposition','work_decomposition_no_truncate','ortak_reject_office_truncate',34,false),
        ('work_decomposition','work_decomposition_at_commit','ortak_work_decomposition_commit',5,true),
        ('run_reviewed_memory_uses','ortak_reviewed_use_immutable','ortak_reviewed_use_immutable',27,false),
        ('run_reviewed_memory_uses','ortak_reviewed_use_no_truncate','ortak_reviewed_use_immutable',34,false),
        ('run_context_snapshots','ortak_reviewed_snapshot_consistent','ortak_reviewed_snapshot_consistent',5,true),
        ('run_reviewed_memory_uses','ortak_reviewed_use_consistent','ortak_reviewed_snapshot_consistent',5,true),
        ('runs','ortak_reviewed_run_admission','ortak_reviewed_run_admission',17,true),
        ('artifacts','ortak_reviewed_artifact_admission','ortak_reviewed_run_admission',5,true),
        ('reviewed_memory_facts','trg_activity_reviewed_fact_use','ortak_activity_notify',17,false),
        ('reviewed_memory_targets','trg_activity_reviewed_target_use','ortak_activity_notify',17,false)
    ) AS required(table_name,trigger_name,function_name,trigger_type,is_deferred) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=selected.table_name::regclass
            AND t.tgname=selected.trigger_name AND t.tgenabled='O' AND NOT t.tgisinternal
            AND t.tgtype=selected.trigger_type AND t.tgdeferrable=selected.is_deferred
            AND t.tginitdeferred=selected.is_deferred
            AND t.tgfoid=(selected.function_name||'()')::regprocedure) THEN
            RAISE EXCEPTION 'ortak: decomposition or reviewed use trigger mismatch';
        END IF;
    END LOOP;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='run_reviewed_memory_uses'::regclass
        AND t.tgfoid='enforce_community_write_fence()'::regprocedure AND t.tgenabled='O'
        AND t.tgtype=31 AND NOT t.tgdeferrable AND NOT t.tginitdeferred AND NOT t.tgisinternal) THEN
        RAISE EXCEPTION 'ortak: reviewed use community fence missing';
    END IF;
END
$ortak_70_71_guards$;


-- Migration73: retained DM identity and pair/expiry authority transitions.
CREATE OR REPLACE FUNCTION ortak_private_dm_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.channel_type = 'dm' AND OLD.participant_hash IS NOT NULL
       AND (NEW.channel_type IS DISTINCT FROM OLD.channel_type
            OR NEW.visibility IS DISTINCT FROM OLD.visibility
            OR NEW.participant_hash IS DISTINCT FROM OLD.participant_hash) THEN
        RAISE EXCEPTION 'A retained DM participant identity cannot be replaced'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END
$$;

-- Converge only the known predecessor or a missing trigger. Unknown function,
-- mode, event or argument substitutions remain an explicit catalog refusal.
DO $ortak_73_triggers$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass AND tgname='ortak_private_dm_identity') THEN
        CREATE TRIGGER ortak_private_dm_identity BEFORE UPDATE ON channels
        FOR EACH ROW EXECUTE FUNCTION ortak_private_dm_identity();
    END IF;
    IF EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass
        AND tgname='ortak_office_authority_channels' AND tgenabled='O' AND tgtype=31
        AND NOT tgisinternal AND NOT tgdeferrable AND NOT tginitdeferred
        AND tgnargs=7 AND tgattr::text='' AND tgqual IS NULL
        AND tgfoid='ortak_fence_office_mutation()'::regprocedure
        AND tgargs=decode('636f6d6d756e69747900636f6d6d756e6974795f6964006964006368616e6e656c5f74797065007669736962696c6974790061726368697665645f61740064656c657465645f617400','hex')) THEN
        DROP TRIGGER ortak_office_authority_channels ON channels;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass AND tgname='ortak_office_authority_channels') THEN
        CREATE TRIGGER ortak_office_authority_channels BEFORE INSERT OR UPDATE OR DELETE ON channels
        FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation(
            'community', 'community_id', 'id', 'channel_type', 'visibility',
            'archived_at', 'deleted_at', 'participant_hash', 'ttl_seconds', 'ttl_deadline');
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass
        AND tgname='ortak_private_dm_identity' AND tgenabled='O' AND tgtype=19
        AND NOT tgisinternal AND NOT tgdeferrable AND NOT tginitdeferred
        AND tgattr::text='' AND tgqual IS NULL
        AND tgfoid='ortak_private_dm_identity()'::regprocedure AND tgnargs=0 AND tgargs=decode('','hex'))
       OR NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass
        AND tgname='ortak_office_authority_channels' AND tgenabled='O' AND tgtype=31
        AND NOT tgisinternal AND NOT tgdeferrable AND NOT tginitdeferred
        AND tgattr::text='' AND tgqual IS NULL
        AND tgfoid='ortak_fence_office_mutation()'::regprocedure AND tgnargs=10
        AND tgargs=decode('636f6d6d756e69747900636f6d6d756e6974795f6964006964006368616e6e656c5f74797065007669736962696c6974790061726368697665645f61740064656c657465645f6174007061727469636970616e745f686173680074746c5f7365636f6e64730074746c5f646561646c696e6500','hex')) THEN
        RAISE EXCEPTION 'ortak: private DM identity or authority trigger mismatch';
    END IF;
END
$ortak_73_triggers$;

DO $ortak_72_73_functions$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_language l ON l.oid=p.prolang
        WHERE p.oid='ortak_snapshot_scratch_jsonb(json)'::regprocedure AND l.lanname='sql'
        AND p.provolatile='i' AND p.proisstrict AND NOT p.prosecdef AND NOT p.proleakproof
        AND p.proparallel='s' AND p.proconfig IS NULL AND p.prorettype='jsonb'::regtype)
       OR EXISTS(SELECT 1 FROM pg_proc p JOIN pg_language l ON l.oid=p.prolang
        WHERE p.oid=ANY(ARRAY['ortak_reviewed_snapshot_consistent()'::regprocedure,
                             'ortak_private_dm_identity()'::regprocedure,
                             'ortak_fence_office_mutation()'::regprocedure])
          AND (l.lanname<>'plpgsql' OR p.provolatile<>'v' OR p.proisstrict OR p.prosecdef
            OR p.proleakproof OR p.proparallel<>'u' OR p.proconfig IS NOT NULL OR p.prorettype<>'trigger'::regtype)) THEN
        RAISE EXCEPTION 'ortak: snapshot comparison or DM function contract mismatch';
    END IF;
END
$ortak_72_73_functions$;

-- Migration74: dynamic community write fences omitted by desired-state DDL.
SELECT attach_community_write_fence('workspace_bindings');
SELECT attach_community_write_fence('workspace_files');
SELECT attach_community_write_fence('run_workspace_uses');
SELECT attach_community_write_fence('workspace_tool_actions');
SELECT attach_community_write_fence('workspace_tool_receipts');
SELECT attach_community_write_fence('workspace_reader_executions');

DO $ortak_74_fences$
DECLARE relation TEXT;
BEGIN
    FOREACH relation IN ARRAY ARRAY['workspace_bindings','workspace_files','run_workspace_uses','workspace_tool_actions','workspace_tool_receipts','workspace_reader_executions'] LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=relation::regclass
            AND t.tgname='community_write_fence_'||relation
            AND t.tgfoid='enforce_community_write_fence()'::regprocedure
            AND NOT t.tgisinternal AND t.tgenabled='O' AND t.tgtype=31
            AND NOT t.tgdeferrable AND NOT t.tginitdeferred AND t.tgnargs=0
            AND t.tgargs=decode('','hex') AND t.tgattr::text='' AND t.tgqual IS NULL) THEN
            RAISE EXCEPTION 'ortak: workspace community write fence mismatch';
        END IF;
    END LOOP;
END
$ortak_74_fences$;

-- Migration74: finish the fail-closed desired-state workspace admission stub.
CREATE OR REPLACE FUNCTION ortak_run_workspace_current(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN u.run_id IS NULL THEN
        NOT require_use OR NOT (r.runtime_adapter='hermes' AND rev.manifest#>'{permissions,allowed_tools}'='["files"]'::jsonb)
    ELSE EXISTS(SELECT 1 FROM workspace_bindings b
        JOIN employees e ON e.company_id=b.company_id AND e.id=b.employee_id
        JOIN employee_revisions active ON active.company_id=e.company_id AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN employee_runtime_bindings runtime ON runtime.company_id=e.company_id AND runtime.employee_id=e.id AND runtime.revision_id=e.active_revision_id
        JOIN companies c ON c.id=b.company_id JOIN communities cm ON cm.id=b.community_id
        JOIN office_company_bindings ob ON ob.company_id=b.company_id AND ob.community_id=b.community_id
        JOIN project_api_bindings pb ON pb.company_id=b.company_id AND pb.project_id=b.project_id AND pb.community_id=b.community_id
        JOIN work_executions wx ON wx.company_id=r.company_id AND wx.run_id=r.id AND wx.project_id=b.project_id
        WHERE b.company_id=u.company_id AND b.id=u.workspace_id AND b.community_id=u.community_id
          AND b.employee_id=r.employee_id AND b.manifest_hash=u.manifest_hash AND b.revoked_at IS NULL AND b.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND e.status='active'
          AND r.employee_revision_id=u.employee_revision_id AND r.employee_lifecycle_epoch=u.employee_lifecycle_epoch
          AND e.lifecycle_epoch=u.employee_lifecycle_epoch AND r.work_item_id=wx.work_item_id
          AND rev.manifest->'permissions'=jsonb_build_object('allowed_tools',jsonb_build_array('files'),
              'allowed_workspaces',jsonb_build_array(b.workspace_ref),'allowed_networks','[]'::jsonb,'approval_required','[]'::jsonb)
          AND active.manifest->'permissions'=rev.manifest->'permissions'
          AND active.manifest#>>'{runtime,workspace_ref}'=b.workspace_ref AND rev.manifest#>>'{runtime,workspace_ref}'=b.workspace_ref
          AND runtime.workspace_ref=b.workspace_ref AND runtime.validated_at IS NOT NULL)
    END FROM runs r JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
    LEFT JOIN run_workspace_uses u ON u.company_id=r.company_id AND u.run_id=r.id
    WHERE r.company_id=company AND r.id=run
$$;

-- Migration74: pgschema1.7.4 flattens nested BETWEEN checks and omits the
-- stopped-reader equivalence. Converge only the observed predecessor or a
-- missing constraint; every other installed definition remains a refusal.
DO $ortak_74_checks$
BEGIN
    IF EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_files'::regclass AND conname='workspace_files_logical_name_check'
        AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
        AND pg_get_constraintdef(oid,false)='CHECK (((octet_length(logical_name) >= 1) AND (octet_length(logical_name) <= 256) AND (logical_name ~ ''^[A-Za-z0-9][A-Za-z0-9._/-]*$''::text) AND (logical_name !~ ''(^|/)(\.|\.\.|)(/|$)''::text)))') THEN
        ALTER TABLE workspace_files DROP CONSTRAINT workspace_files_logical_name_check;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_files'::regclass AND conname='workspace_files_logical_name_check') THEN
        ALTER TABLE workspace_files ADD CONSTRAINT workspace_files_logical_name_check CHECK(octet_length(logical_name) BETWEEN 1 AND 256 AND logical_name ~ '^[A-Za-z0-9][A-Za-z0-9._/-]*$' AND logical_name !~ '(^|/)(\.|\.\.|)(/|$)');
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_files'::regclass AND conname='workspace_files_logical_name_check'
        AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
        AND pg_get_constraintdef(oid,false)='CHECK ((((octet_length(logical_name) >= 1) AND (octet_length(logical_name) <= 256)) AND (logical_name ~ ''^[A-Za-z0-9][A-Za-z0-9._/-]*$''::text) AND (logical_name !~ ''(^|/)(\.|\.\.|)(/|$)''::text)))') THEN
        RAISE EXCEPTION 'ortak: workspace desired-state check mismatch';
    END IF;
    IF EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_reader_executions'::regclass AND conname='workspace_reader_executions_check1'
        AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
        AND pg_get_constraintdef(oid,false)='CHECK (((executable IS NULL) OR ((octet_length(executable) >= 1) AND (octet_length(executable) <= 4096) AND ("left"(executable, 1) = ''/''::text) AND (octet_length(executable_hash) = 32) AND (operating_uid >= 0) AND (operating_uid <= ''4294967295''::bigint))))') THEN
        ALTER TABLE workspace_reader_executions DROP CONSTRAINT workspace_reader_executions_check1;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_reader_executions'::regclass AND conname='workspace_reader_executions_check1') THEN
        ALTER TABLE workspace_reader_executions ADD CONSTRAINT workspace_reader_executions_check1 CHECK(executable IS NULL OR (octet_length(executable) BETWEEN 1 AND 4096 AND left(executable,1)='/' AND octet_length(executable_hash)=32 AND operating_uid BETWEEN 0 AND 4294967295));
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_reader_executions'::regclass AND conname='workspace_reader_executions_check1'
        AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
        AND pg_get_constraintdef(oid,false)='CHECK (((executable IS NULL) OR (((octet_length(executable) >= 1) AND (octet_length(executable) <= 4096)) AND ("left"(executable, 1) = ''/''::text) AND (octet_length(executable_hash) = 32) AND ((operating_uid >= 0) AND (operating_uid <= ''4294967295''::bigint)))))') THEN
        RAISE EXCEPTION 'ortak: workspace desired-state check mismatch';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_reader_executions'::regclass AND conname='workspace_reader_executions_check2') THEN
        ALTER TABLE workspace_reader_executions ADD CONSTRAINT workspace_reader_executions_check2 CHECK((state='stopped')=(stopped_at IS NOT NULL) AND (state='stopped')=(stop_proof IS NOT NULL));
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid='workspace_reader_executions'::regclass AND conname='workspace_reader_executions_check2'
        AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
        AND pg_get_constraintdef(oid,false)='CHECK ((((state = ''stopped''::text) = (stopped_at IS NOT NULL)) AND ((state = ''stopped''::text) = (stop_proof IS NOT NULL))))') THEN
        RAISE EXCEPTION 'ortak: workspace desired-state check mismatch';
    END IF;
END
$ortak_74_checks$;

-- Migration74: restore exact immutable PL/pgSQL source after pgschema
-- rewrites the final END whitespace. Catalog equality does not normalize bodies.
CREATE OR REPLACE FUNCTION ortak_workspace_binding_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'revoked_at') IS DISTINCT FROM (to_jsonb(OLD)-'revoked_at')
            OR OLD.revoked_at IS NOT NULL OR NEW.revoked_at IS NULL THEN
            RAISE EXCEPTION 'ortak: workspace revision is immutable except one withdrawal' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.revoked_at IS NOT NULL OR NEW.verified_at>clock_timestamp()
        OR NEW.verified_at<clock_timestamp()-INTERVAL '30 seconds'
        OR NEW.expires_at<=clock_timestamp() OR NEW.expires_at>clock_timestamp()+INTERVAL '30 days' THEN
        RAISE EXCEPTION 'ortak: workspace verification or retention is invalid' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_manifest_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE b workspace_bindings; wire JSONB; files JSONB; file_count INTEGER; total INTEGER;
BEGIN
    IF TG_TABLE_NAME='workspace_bindings' THEN b=NEW;
    ELSE SELECT * INTO b FROM workspace_bindings WHERE company_id=NEW.company_id AND id=NEW.workspace_id; END IF;
    wire=convert_from(b.grant_bytes,'UTF8')::jsonb;
    SELECT count(*),coalesce(sum(byte_count),0),jsonb_agg(jsonb_build_object('file_id',id,'name',logical_name,
        'media_type',media_type,'bytes',byte_count,'sha256',encode(content_hash,'hex')) ORDER BY id)
        INTO file_count,total,files FROM workspace_files WHERE company_id=b.company_id AND workspace_id=b.id AND community_id=b.community_id;
    IF file_count NOT BETWEEN 1 AND 8 OR total>65536
        OR EXISTS(SELECT 1 FROM workspace_files f WHERE f.company_id=b.company_id AND f.workspace_id=b.id AND
            (f.community_id<>b.community_id OR f.ordinal<>(SELECT count(*) FROM workspace_files p
                WHERE p.company_id=f.company_id AND p.workspace_id=f.workspace_id AND p.id<f.id)))
        OR wire IS DISTINCT FROM jsonb_build_object('format','ortak-workspace-read/v1','company_id',b.company_id,
            'project_id',b.project_id,'employee_id',b.employee_id,'workspace_ref',b.workspace_ref,'revision',b.id,
            'manifest_hash',encode(b.manifest_hash,'hex'),'files',files)
        OR b.grant_bytes<>convert_to(ortak_workspace_canonical(wire),'UTF8')
        OR b.manifest_hash<>sha256(convert_to(ortak_workspace_canonical(wire-'manifest_hash'),'UTF8')) THEN
        RAISE EXCEPTION 'ortak: workspace selected manifest differs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_activation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE manifest JSONB;
BEGIN
    IF NEW.status<>'active' OR (TG_OP='UPDATE' AND NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
        AND NEW.status IS NOT DISTINCT FROM OLD.status) THEN RETURN NEW; END IF;
    SELECT r.manifest INTO manifest FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id;
    IF manifest#>>'{runtime,adapter}'='hermes' AND manifest#>'{permissions,allowed_tools}'='["files"]'::jsonb
        AND NOT ortak_workspace_profile_available(NEW.company_id,NEW.id,manifest#>>'{runtime,workspace_ref}') THEN
        RAISE EXCEPTION 'ortak: Files profile requires a current selected workspace at activation' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_lock_run_workspace(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM b.id FROM workspace_bindings b JOIN run_workspace_uses u ON u.company_id=b.company_id AND u.workspace_id=b.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY b.id FOR SHARE OF b;
    RETURN coalesce(ortak_run_workspace_current(company,run,require_use),false);
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_use_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM outbox o JOIN runs r ON r.company_id=o.company_id AND r.id=o.run_id
            WHERE o.company_id=NEW.company_id AND o.id=NEW.outbox_id AND o.run_id=NEW.run_id
              AND o.kind='work_run_dispatch' AND o.state='pending' AND o.lease_token=NEW.admission_lease
              AND o.lease_expires_at>clock_timestamp() AND r.status='queued' AND r.runtime_run_ref IS NULL)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.workspace_id=NEW.workspace_id AND reader.request_key='prepare' AND reader.owner_lease=NEW.admission_lease AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace use lacks current dispatch authority' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_action_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'pending' OR NEW.attempt_count<>0 OR NEW.lease_token IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: invalid initial workspace action' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            IS DISTINCT FROM (to_jsonb(OLD)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            OR OLD.state IN('delivered','interrupted') OR NEW.attempt_count<OLD.attempt_count
            OR NEW.attempt_count>OLD.attempt_count+1 OR NEW.updated_at<OLD.updated_at
            OR (NEW.state='pending' AND OLD.state<>'pending') THEN
            RAISE EXCEPTION 'ortak: invalid workspace action transition' USING ERRCODE='check_violation';
        END IF;
        IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
            IF OLD.lease_expires_at>clock_timestamp() OR NEW.attempt_count<>OLD.attempt_count+1
                OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+INTERVAL '30 seconds' THEN
                RAISE EXCEPTION 'ortak: workspace action lease is not claimable' USING ERRCODE='check_violation';
            END IF;
        ELSIF NEW.attempt_count<>OLD.attempt_count OR NEW.lease_expires_at IS DISTINCT FROM OLD.lease_expires_at THEN
            IF NOT (NEW.lease_token IS NULL AND NEW.lease_expires_at IS NULL AND NEW.attempt_count=OLD.attempt_count) THEN
                RAISE EXCEPTION 'ortak: workspace action attempt is not a fresh claim' USING ERRCODE='check_violation';
            END IF;
        END IF;
        IF NEW.state IN('result_ready','delivered') AND NOT EXISTS(SELECT 1 FROM workspace_tool_receipts x
            WHERE x.company_id=NEW.company_id AND x.run_id=NEW.run_id AND x.call_id=NEW.call_id) THEN
            RAISE EXCEPTION 'ortak: workspace action needs its exact result receipt' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_action_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND NEW.state='interrupted' THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM run_workspace_uses u JOIN workspace_files f ON f.company_id=u.company_id AND f.workspace_id=u.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND f.id=NEW.file_id AND u.community_id=NEW.community_id)
        OR (NEW.state='pending' AND NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)) THEN
        RAISE EXCEPTION 'ortak: workspace action input is not currently selected' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE a workspace_tool_actions; f workspace_files; wire JSONB;
BEGIN
    SELECT * INTO a FROM workspace_tool_actions WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND call_id=NEW.call_id;
    SELECT file.* INTO f FROM workspace_files file JOIN run_workspace_uses u ON u.company_id=file.company_id AND u.workspace_id=file.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND file.id=a.file_id;
    wire=convert_from(NEW.result_bytes,'UTF8')::jsonb;
    IF a.call_id IS NULL OR f.id IS NULL OR a.community_id<>NEW.community_id OR a.arguments_hash<>NEW.arguments_hash
        OR a.state<>'result_ready' OR a.lease_token IS DISTINCT FROM NEW.lease_token OR a.attempt_count<>NEW.attempt_count
        OR a.lease_expires_at<=clock_timestamp() OR NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.request_key='read:'||NEW.call_id AND reader.owner_lease=NEW.lease_token AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status IN('running','waiting'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace result has no exact live authority/lease' USING ERRCODE='check_violation';
    END IF;
    IF wire->>'status'='completed' THEN
        IF wire IS DISTINCT FROM jsonb_build_object('status','completed','content',wire->>'content','sha256',encode(f.content_hash,'hex'),
            'bytes',f.byte_count,'name',f.logical_name) OR octet_length(wire->>'content') IS DISTINCT FROM f.byte_count
            OR sha256(convert_to(wire->>'content','UTF8')) IS DISTINCT FROM f.content_hash THEN
            RAISE EXCEPTION 'ortak: workspace result bytes differ from selected input' USING ERRCODE='check_violation';
        END IF;
    ELSIF wire IS DISTINCT FROM jsonb_build_object('status','failed','code',wire->>'code')
        OR wire->>'code' IS NULL OR wire->>'code' NOT IN('authority_changed','workspace_unavailable','file_unavailable','input_changed','deadline_exceeded','cancelled') THEN
        RAISE EXCEPTION 'ortak: invalid workspace failure result' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE run UUID; required BOOLEAN=true;
BEGIN
    IF TG_TABLE_NAME='runs' THEN
        IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
            AND NEW.runtime_run_ref IS NOT DISTINCT FROM OLD.runtime_run_ref THEN RETURN NEW; END IF;
        -- A confirmed stop can discover the reference of an accepted start
        -- whose response was lost. Restore only that metadata under the live
        -- cancellation lease (or its ACK), never renew execution authority.
        IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
            AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
            AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                AND (c.state='acknowledged' OR (c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp())))
            AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN
            RETURN NEW;
        END IF;
        run=NEW.id; required=NEW.runtime_run_ref IS NOT NULL;
    ELSE run=NEW.run_id;
    END IF;
    IF run IS NULL THEN RETURN NEW; END IF;
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,run,required),false) THEN
        RAISE EXCEPTION 'ortak: selected workspace permission changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_reader_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'planned' OR NEW.pid IS NOT NULL OR NEW.owner_deadline<=clock_timestamp()
            OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR NOT EXISTS(SELECT 1 FROM workspace_bindings b WHERE b.company_id=NEW.company_id AND b.id=NEW.workspace_id AND b.community_id=NEW.community_id)
            OR NOT (EXISTS(SELECT 1 FROM outbox o WHERE o.company_id=NEW.company_id AND o.run_id=NEW.run_id AND o.kind='work_run_dispatch'
                AND o.state='pending' AND o.lease_token=NEW.owner_lease AND o.lease_expires_at=NEW.owner_deadline AND o.lease_expires_at>clock_timestamp() AND NEW.request_key='prepare')
                OR EXISTS(SELECT 1 FROM workspace_tool_actions a WHERE a.company_id=NEW.company_id AND a.run_id=NEW.run_id
                    AND NEW.request_key='read:'||a.call_id AND a.state='pending' AND a.lease_token=NEW.owner_lease
                    AND a.lease_expires_at=NEW.owner_deadline AND a.lease_expires_at>clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: reader execution needs its exact live owner lease' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'pid'-'state'-'stop_proof'-'stopped_at') IS DISTINCT FROM (to_jsonb(OLD)-'pid'-'state'-'stop_proof'-'stopped_at')
            OR OLD.state='stopped' OR NEW.state='planned' OR (OLD.pid IS NOT NULL AND NEW.pid IS DISTINCT FROM OLD.pid)
            OR (NEW.state='running' AND (OLD.state<>'planned' OR NEW.owner_deadline<=clock_timestamp()
                OR (NEW.executable IS NOT NULL AND NEW.pid IS NULL)
                OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
                OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)))
            OR (NEW.state='stopped' AND NEW.stop_proof='confirmed_absence' AND NEW.owner_deadline>clock_timestamp()) THEN
            RAISE EXCEPTION 'ortak: reader identity or stop proof changed' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_workspace_reader_cancel_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NEW.state='acknowledged' AND EXISTS(SELECT 1 FROM workspace_reader_executions r
        WHERE r.company_id=NEW.company_id AND r.run_id=NEW.run_id AND r.state<>'stopped') THEN
        RAISE EXCEPTION 'ortak: unresolved workspace reader prevents cancellation acknowledgement' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;


-- Migration75: exact conversation function bodies after all desired tables.
-- Preserve source proconfig/defaults/security/volatility and body bytes; do not
-- leave a pgschema-formatted body or a fail-closed bootstrap stub installed.
CREATE OR REPLACE FUNCTION ortak_conversation_json75(value JSONB, nesting INTEGER DEFAULT 0)
RETURNS TEXT LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    member RECORD;
    encoded TEXT;
    result TEXT;
    separator TEXT := '';
BEGIN
    IF nesting < 0 OR nesting > 4 OR octet_length(value::text) > 524288 THEN
        RETURN NULL;
    END IF;
    CASE jsonb_typeof(value)
    WHEN 'object' THEN
        result := '{';
        FOR member IN SELECT e.key, e.val FROM jsonb_each(value) AS e(key,val)
                      ORDER BY e.key COLLATE "C" LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || to_json(member.key)::text || ':' || encoded;
            separator := ',';
        END LOOP;
        RETURN result || '}';
    WHEN 'array' THEN
        result := '[';
        FOR member IN SELECT e.val FROM jsonb_array_elements(value)
                      WITH ORDINALITY AS e(val,ordinal) ORDER BY e.ordinal LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || encoded;
            separator := ',';
        END LOOP;
        RETURN result || ']';
    WHEN 'string' THEN RETURN to_json(value #>> '{}')::text;
    WHEN 'number' THEN
        -- These wires contain only the canonical event's int32 kind, never
        -- arbitrary floating-point values whose encoders could disagree.
        IF value::text !~ '^-?(0|[1-9][0-9]*)$' THEN RETURN NULL; END IF;
        RETURN value::text;
    WHEN 'boolean' THEN RETURN value::text;
    WHEN 'null' THEN RETURN 'null';
    ELSE RETURN NULL;
    END CASE;
END
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_source_observation(
    company UUID, project UUID, employee TEXT, human BYTEA,
    source_id BYTEA, audience_kind TEXT
) RETURNS TABLE(
    community_id UUID,
    channel_id UUID,
    source_event_created_at TIMESTAMPTZ,
    thread_root_event_id BYTEA,
    thread_root_event_created_at TIMESTAMPTZ,
    audience_bytes BYTEA,
    audience_hash BYTEA,
    source_evidence_hash BYTEA,
    source_hash BYTEA,
    provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ,
    valid_before TIMESTAMPTZ
) LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    node RECORD;
    first_node RECORD;
    count_nodes INTEGER := 0;
    seen BYTEA[] := ARRAY[]::bytea[];
    expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ;
    expected_depth INTEGER;
    expected_root BYTEA;
    expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA;
    resolved_root_at TIMESTAMPTZ;
    tag JSONB;
    part JSONB;
    marker TEXT;
    reference_id BYTEA;
    claimed_root BYTEA;
    claimed_parent BYTEA;
    effective_depth INTEGER;
    source_stamp TEXT;
    root_stamp TEXT;
    audience_wire JSONB;
    encoded TEXT;
BEGIN
    IF company IS NULL OR project IS NULL OR employee IS NULL OR human IS NULL
       OR source_id IS NULL OR audience_kind IS NULL
       OR company = '00000000-0000-0000-0000-000000000000'::uuid
       OR project = '00000000-0000-0000-0000-000000000000'::uuid
       OR octet_length(employee) NOT BETWEEN 1 AND 64
       OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
       OR octet_length(human) <> 32 OR octet_length(source_id) <> 32
       OR audience_kind NOT IN ('channel','thread') THEN
        RETURN;
    END IF;

    FOR node IN
      WITH RECURSIVE visible AS MATERIALIZED (
        SELECT office.community_id, a.channel_id, statement_timestamp() AS observed_at,
               least(ch.ttl_deadline,b.valid_until) AS valid_before
        FROM public.companies co
        JOIN public.office_company_bindings office ON office.company_id=co.id
        JOIN public.communities cm ON cm.id=office.community_id
          AND cm.deleted_at IS NULL AND cm.deletion_state='active'
        JOIN public.projects p ON p.company_id=co.id AND p.id=$2 AND p.status='active'
        JOIN public.project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=cm.id
        JOIN public.project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id
          AND g.actor_pubkey=encode($4,'hex') AND g.revoked_at IS NULL
        JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=a.channel_id
          AND ch.channel_type='stream' AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
          AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
        JOIN public.channel_members human_member ON human_member.community_id=cm.id AND human_member.channel_id=ch.id
          AND human_member.pubkey=$4 AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.employees emp ON emp.company_id=co.id AND emp.id=$3 AND emp.status='active'
        JOIN public.employee_revisions rev ON rev.company_id=emp.company_id AND rev.employee_id=emp.id AND rev.id=emp.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=emp.company_id AND b.employee_id=emp.id
          AND encode(b.public_key,'hex')=rev.manifest #>> '{office,public_key}'
          AND b.signer_ref=rev.manifest #>> '{office,signer_ref}'
          AND b.verified_at IS NOT NULL AND b.valid_from<=statement_timestamp()
          AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.channel_members employee_member ON employee_member.community_id=cm.id AND employee_member.channel_id=ch.id
          AND employee_member.pubkey=b.public_key AND employee_member.removed_at IS NULL
        WHERE co.id=$1 AND co.status='active'
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$4
            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
          AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$4)
          AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$4 AND bot.role='bot')
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.office_inbox i ON i.company_id=$1 AND i.event_id=$5 AND i.state='decided'
          AND i.channel_id=v.channel_id
        JOIN public.events e ON e.community_id=v.community_id AND e.id=i.event_id AND e.created_at=i.event_created_at
          AND e.pubkey=i.author_pubkey AND e.kind=i.event_kind AND e.channel_id=i.channel_id
        WHERE e.kind IN(9,40002) AND e.deleted_at IS NULL AND octet_length(e.content)<=65536
          AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
      ), ancestry AS (
        SELECT 0 AS hop,e.id,e.created_at,
          CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END AS tags,
          t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
          t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
        FROM source s JOIN public.events e ON e.community_id=s.community_id AND e.id=s.id AND e.created_at=s.created_at
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        UNION ALL
        SELECT a.hop+1,e.id,e.created_at,
          CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END,
          t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
          t.root_event_id,t.root_event_created_at,t.depth
        FROM ancestry a JOIN public.events e ON e.community_id=(SELECT s.community_id FROM source s)
          AND e.id=a.parent_event_id AND e.created_at=a.parent_event_created_at
          AND e.channel_id=(SELECT s.channel_id FROM source s) AND e.deleted_at IS NULL AND e.kind IN(9,40002)
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        WHERE a.hop<32
      )
      SELECT a.*,s.community_id,s.channel_id,s.observed_at,s.valid_before,
        CASE WHEN a.hop=0 THEN s.content END AS source_content,
        CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
        CASE WHEN a.hop=0 THEN s.sig END AS source_signature,s.kind AS source_kind
      FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
    LOOP
        IF node.hop <> count_nodes OR octet_length(node.id) <> 32
           OR node.id = ANY(seen)
           OR NOT isfinite(node.created_at)
           OR node.created_at < '1970-01-01 00:00:00+00'::timestamptz
           OR node.created_at >= '10000-01-01 00:00:00+00'::timestamptz
           OR node.tags IS NULL OR jsonb_typeof(node.tags) <> 'array' THEN RETURN; END IF;
        seen := array_append(seen,node.id);
        IF count_nodes=0 THEN
            first_node := node;
            IF node.community_id = '00000000-0000-0000-0000-000000000000'::uuid
               OR node.channel_id = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN; END IF;
        ELSE
            IF expected_parent IS DISTINCT FROM node.id
               OR expected_parent_at IS DISTINCT FROM node.created_at THEN RETURN; END IF;
        END IF;

        -- Vec<Vec<String>> parity: even non-e tags must be arrays of strings.
        claimed_root := NULL; claimed_parent := NULL;
        FOR tag IN SELECT t.value FROM jsonb_array_elements(node.tags) AS t(value) LOOP
            IF jsonb_typeof(tag) <> 'array' THEN RETURN; END IF;
            FOR part IN SELECT t.value FROM jsonb_array_elements(tag) AS t(value) LOOP
                IF jsonb_typeof(part) <> 'string' THEN RETURN; END IF;
            END LOOP;
            IF tag->>0 IS DISTINCT FROM 'e' THEN CONTINUE; END IF;
            IF jsonb_array_length(tag)<4 OR octet_length(tag->>1)<>64
               OR (tag->>1) COLLATE "C" !~ '^[0-9a-fA-F]{64}$' THEN RETURN; END IF;
            reference_id := decode(tag->>1,'hex');
            marker := tag->>3;
            CASE marker
            WHEN 'root' THEN
                IF claimed_root IS NOT NULL THEN RETURN; END IF;
                claimed_root := reference_id;
            WHEN 'reply' THEN
                IF claimed_parent IS NOT NULL THEN RETURN; END IF;
                claimed_parent := reference_id;
            WHEN 'mention' THEN CONTINUE;
            ELSE RETURN;
            END CASE;
        END LOOP;
        IF claimed_root IS NOT NULL AND claimed_parent IS NULL THEN RETURN; END IF;
        claimed_root := coalesce(claimed_root,claimed_parent);

        -- Both locator halves are required, including exact UTC partition time.
        IF (node.parent_event_id IS NULL) <> (node.parent_event_created_at IS NULL)
           OR (node.root_event_id IS NULL) <> (node.root_event_created_at IS NULL) THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL AND (octet_length(node.parent_event_id)<>32
           OR NOT isfinite(node.parent_event_created_at)
           OR node.parent_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.parent_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;
        IF node.root_event_id IS NOT NULL AND (octet_length(node.root_event_id)<>32
           OR NOT isfinite(node.root_event_created_at)
           OR node.root_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.root_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;

        effective_depth := coalesce(node.depth,0);
        IF node.metadata_present THEN
            IF node.metadata_channel IS DISTINCT FROM first_node.channel_id THEN RETURN; END IF;
            IF node.parent_event_id IS NULL AND node.depth=0 AND claimed_parent IS NULL THEN
                IF node.root_event_id IS NOT NULL AND
                   (node.root_event_id IS DISTINCT FROM node.id OR node.root_event_created_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            ELSIF node.parent_event_id IS NOT NULL AND node.root_event_id IS NOT NULL
                  AND node.depth BETWEEN 1 AND 32
                  AND claimed_parent=node.parent_event_id AND claimed_root=node.root_event_id THEN
                NULL;
            ELSE RETURN;
            END IF;
        ELSIF node.parent_event_id IS NOT NULL OR node.root_event_id IS NOT NULL
              OR node.depth IS NOT NULL OR claimed_parent IS NOT NULL THEN RETURN;
        END IF;
        IF count_nodes>0 AND expected_depth IS DISTINCT FROM effective_depth THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL THEN
            IF count_nodes=0 THEN
                expected_root := node.root_event_id;
                expected_root_at := node.root_event_created_at;
            ELSIF node.root_event_id IS DISTINCT FROM expected_root
                  OR node.root_event_created_at IS DISTINCT FROM expected_root_at THEN RETURN;
            END IF;
        ELSE
            IF expected_root IS NOT NULL AND (expected_root IS DISTINCT FROM node.id
               OR expected_root_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            resolved_root := node.id; resolved_root_at := node.created_at;
        END IF;
        expected_parent := node.parent_event_id;
        expected_parent_at := node.parent_event_created_at;
        expected_depth := effective_depth-1;
        count_nodes := count_nodes+1;
    END LOOP;
    -- A missing/deleted/cross-channel parent, cycle or 33rd edge cannot become
    -- a top-level fallback. Every nonterminal depth decreases to an actual root.
    IF count_nodes=0 OR expected_parent IS NOT NULL OR resolved_root IS NULL THEN RETURN; END IF;

    community_id := first_node.community_id;
    channel_id := first_node.channel_id;
    source_event_created_at := first_node.created_at;
    observed_at := first_node.observed_at;
    valid_before := first_node.valid_before;
    source_stamp := to_char(source_event_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    IF audience_kind='thread' THEN
        thread_root_event_id := resolved_root;
        thread_root_event_created_at := resolved_root_at;
        root_stamp := to_char(resolved_root_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    END IF;

    audience_wire := jsonb_build_object(
        'channel_id',channel_id,'community_id',community_id,'company_id',company,
        'employee_id',employee,'format','ortak-reviewed-conversation-audience/1',
        'kind',audience_kind,'project_id',project,'thread_root_event_created_at',root_stamp,
        'thread_root_event_id',encode(thread_root_event_id,'hex'));
    encoded := public.ortak_conversation_json75(audience_wire);
    IF encoded IS NULL THEN RETURN; END IF;
    audience_bytes := convert_to(encoded,'UTF8');
    IF octet_length(audience_bytes)>2048 THEN RETURN; END IF;
    audience_hash := public.digest(audience_bytes,'sha256');

    -- Exact SourceEvidence declaration order in Rust is lexical; tags and
    -- source content retain their original strings and array order. No body
    -- is returned, copied into provenance or replaced with a message:<id> hash.
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'author_pubkey',encode(first_node.source_author,'hex'),'channel_id',channel_id,
        'community_id',community_id,'company_id',company,'content',first_node.source_content,
        'event_created_at',source_stamp,'event_id',encode(source_id,'hex'),
        'format','ortak-reviewed-conversation-evidence/1','kind',first_node.source_kind,
        'sig',encode(first_node.source_signature,'hex'),'tags',first_node.tags));
    IF encoded IS NULL THEN RETURN; END IF;
    source_evidence_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience_hash',encode(audience_hash,'hex'),'format','ortak-reviewed-conversation-source/1',
        'source_evidence_hash',encode(source_evidence_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    source_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience',audience_wire,'audience_hash',encode(audience_hash,'hex'),
        'format','ortak-reviewed-conversation-provenance/1','source_event_created_at',source_stamp,
        'source_event_id',encode(source_id,'hex'),'source_evidence_hash',encode(source_evidence_hash,'hex'),
        'source_hash',encode(source_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    provenance_bytes := convert_to(encoded,'UTF8');
    IF octet_length(provenance_bytes)>4096 THEN RETURN; END IF;
    RETURN NEXT;
END
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_scope_current(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM companies co
        JOIN office_company_bindings office ON office.company_id=co.id
            AND office.community_id=community
        JOIN communities cm ON cm.id=office.community_id
        JOIN projects p ON p.company_id=co.id AND p.id=project
        JOIN project_api_bindings b ON b.company_id=p.company_id AND b.project_id=p.id
            AND b.community_id=cm.id AND b.channel_id=channel
        JOIN channels ch ON ch.community_id=cm.id AND ch.id=b.channel_id
        WHERE co.id=company AND co.status='active' AND p.status='active'
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL
            AND ch.channel_type='stream' AND ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
    )
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'ortak: conversation authorities are retained'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF TG_OP='UPDATE' THEN
        IF (NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id,NEW.created_at)
            IS DISTINCT FROM
            (OLD.company_id,OLD.community_id,OLD.project_id,OLD.channel_id,OLD.created_at)
            OR OLD.epoch=9223372036854775807 OR NEW.epoch<>OLD.epoch+1
            OR NEW.last_change_reason='registered' THEN
            RAISE EXCEPTION 'ortak: conversation authority only advances'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Mutation hooks own the Office/project fence and update sorted scope
        -- rows. Do not acquire Office exclusive here: project-grant writers use
        -- the existing project NOWAIT fence under signed shared-Office auth.
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        FOR SHARE NOWAIT;
    IF NOT FOUND OR NEW.epoch<>0 OR NEW.last_change_reason<>'registered'
        OR NOT ortak_conversation_scope_current(
            NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- Both ceilings include retained/removed scopes. Community then company
    -- registration locks are always nonblocking and acquired in that order.
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||NEW.community_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE company_id=NEW.company_id)>=128 THEN
        RAISE EXCEPTION 'ortak: retained conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'ortak: retained community conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp();
    NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_register_conversation_authority(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE selected BIGINT;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=project FOR SHARE NOWAIT;
    IF NOT FOUND OR NOT ortak_conversation_scope_current(company,community,project,channel) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||community::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    SELECT a.epoch INTO selected FROM conversation_memory_authorities a
        WHERE a.company_id=company AND a.community_id=community
            AND a.project_id=project AND a.channel_id=channel FOR SHARE;
    IF FOUND THEN RETURN selected; END IF;
    -- A conflicting retained identity is an error, never a rebind/upsert.
    INSERT INTO conversation_memory_authorities(company_id,community_id,project_id,channel_id)
        VALUES(company,community,project,channel) RETURNING epoch INTO selected;
    RETURN selected;
END $$;

CREATE OR REPLACE FUNCTION ortak_conversation_fact_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; fact UUID; f reviewed_memory_facts;
    a reviewed_memory_conversation_audiences; observed RECORD;
BEGIN
    company=NEW.company_id;
    IF TG_TABLE_NAME='reviewed_memory_facts' THEN
        fact=NEW.id;
    ELSE
        fact=NEW.fact_id;
    END IF;
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=fact;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation audience parent fact is missing' USING ERRCODE='check_violation';
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=fact;
    IF f.audience_kind='project' THEN
        IF FOUND THEN
            RAISE EXCEPTION 'ortak: project facts cannot acquire a conversation audience' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NOT FOUND OR f.audience_kind<>'conversation' OR f.source_artifact_id IS NOT NULL
        OR (a.company_id,a.community_id,a.project_id,a.employee_id,a.source_event_id)
            IS DISTINCT FROM (f.company_id,f.community_id,f.project_id,f.employee_id,f.source_message_id)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_facts born
            WHERE born.company_id=company AND born.id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_conversation_audiences born
            WHERE born.company_id=company AND born.fact_id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_operations receipt
            WHERE receipt.company_id=f.company_id AND receipt.community_id=f.community_id
                AND receipt.fact_id=f.id AND receipt.project_id=f.project_id
                AND receipt.actor_pubkey=f.approved_by AND receipt.operation_id=f.promotion_operation_id
                AND receipt.action='promote' AND receipt.result_version=1
                AND receipt.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires one atomic audience and promotion receipt'
            USING ERRCODE='check_violation';
    END IF;
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=f.project_id FOR SHARE NOWAIT;
    PERFORM 1 FROM conversation_memory_authorities authority
        WHERE authority.company_id=a.company_id AND authority.community_id=a.community_id
            AND authority.project_id=a.project_id AND authority.channel_id=a.channel_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation authority identity is missing' USING ERRCODE='check_violation';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM project_access_grants grant_row
        WHERE grant_row.company_id=f.company_id AND grant_row.project_id=f.project_id
            AND grant_row.actor_pubkey=f.approved_by AND grant_row.revoked_at IS NULL
            AND grant_row.role IN ('owner','reviewer')) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires current project review authority'
            USING ERRCODE='check_violation';
    END IF;
    BEGIN
        SELECT * INTO STRICT observed FROM ortak_conversation_source_observation(
            f.company_id,f.project_id,f.employee_id,decode(f.approved_by,'hex'),
            f.source_message_id,a.kind);
    EXCEPTION WHEN NO_DATA_FOUND OR TOO_MANY_ROWS THEN
        RAISE EXCEPTION 'ortak: conversation approval source is no longer current'
            USING ERRCODE='check_violation';
    END;
    IF (a.community_id,a.channel_id,a.source_event_created_at,
        a.thread_root_event_id,a.thread_root_event_created_at,a.audience_bytes,
        a.audience_hash,a.source_evidence_hash,a.source_hash,a.provenance_bytes)
        IS DISTINCT FROM
        (observed.community_id,observed.channel_id,observed.source_event_created_at,
        observed.thread_root_event_id,observed.thread_root_event_created_at,observed.audience_bytes,
        observed.audience_hash,observed.source_evidence_hash,observed.source_hash,observed.provenance_bytes)
        OR f.expires_at<=clock_timestamp()
        OR (observed.valid_before IS NOT NULL AND
            (clock_timestamp()>=observed.valid_before OR f.expires_at>observed.valid_before)) THEN
        RAISE EXCEPTION 'ortak: conversation approval bytes or current deadline differ'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_conversation_use_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
BEGIN
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=NEW.company_id AND x.id=NEW.fact_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: reviewed use fact is missing' USING ERRCODE='check_violation';
    END IF;
    IF f.audience_kind='project' THEN
        IF NEW.conversation_audience_hash IS NOT NULL OR NEW.conversation_authority_epoch IS NOT NULL
            OR NEW.conversation_consumption_epoch IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: project use cannot carry conversation pins' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id;
    IF NOT FOUND OR NEW.consumption_epoch<>0 OR NEW.conversation_audience_hash IS DISTINCT FROM a.audience_hash
        OR NEW.conversation_authority_epoch IS NULL OR NEW.conversation_consumption_epoch IS NULL
        OR NEW.community_id<>a.community_id OR NEW.source_hash<>a.source_hash
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_targets target
            WHERE target.company_id=NEW.company_id AND target.id=NEW.target_id
                AND target.community_id=a.community_id AND target.project_id=a.project_id
                AND target.employee_id=a.employee_id AND target.conversation_channel_id=a.channel_id) THEN
        RAISE EXCEPTION 'ortak: reviewed conversation use storage pins differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

-- Migration75: exact storage checks and triggers. Expected CHECK text was
-- observed from the source-fragment database, not inferred from pgschema output.
-- Add only missing constraints; unknown installed expressions fail closed.
DO $ortak_75_checks$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('conversation_memory_authorities','conversation_memory_authorities_channel_id_check','CHECK ((channel_id <> ''00000000-0000-0000-0000-000000000000''::uuid))'),
        ('conversation_memory_authorities','conversation_memory_authorities_check','CHECK (((company_id <> ''00000000-0000-0000-0000-000000000000''::uuid) AND (community_id <> ''00000000-0000-0000-0000-000000000000''::uuid) AND (project_id <> ''00000000-0000-0000-0000-000000000000''::uuid)))'),
        ('conversation_memory_authorities','conversation_memory_authorities_check1','CHECK ((changed_at >= created_at))'),
        ('conversation_memory_authorities','conversation_memory_authorities_epoch_check','CHECK ((epoch >= 0))'),
        ('conversation_memory_authorities','conversation_memory_authorities_last_change_reason_check','CHECK ((last_change_reason = ANY (ARRAY[''registered''::text, ''channel_changed''::text, ''membership_changed''::text, ''project_changed''::text, ''project_grant_changed''::text, ''event_changed''::text, ''thread_changed''::text, ''identity_changed''::text, ''scope_closed''::text])))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audi_source_event_created_at_check','CHECK (((source_event_created_at >= ''1970-01-01 00:00:00+00''::timestamp with time zone) AND (source_event_created_at < ''10000-01-01 00:00:00+00''::timestamp with time zone)))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audienc_source_evidence_hash_check','CHECK ((octet_length(source_evidence_hash) = 32))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audienc_thread_root_event_id_check','CHECK ((octet_length(thread_root_event_id) = 32))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_audience_bytes_check','CHECK (((octet_length(audience_bytes) >= 1) AND (octet_length(audience_bytes) <= 2048)))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_audience_hash_check','CHECK ((octet_length(audience_hash) = 32))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_check','CHECK ((((kind = ''channel''::text) AND (thread_root_event_id IS NULL) AND (thread_root_event_created_at IS NULL)) OR ((kind = ''thread''::text) AND (thread_root_event_id IS NOT NULL) AND (thread_root_event_created_at IS NOT NULL))))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_check1','CHECK (((source_event_id IS DISTINCT FROM thread_root_event_id) OR (source_event_created_at = thread_root_event_created_at)))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_check2','CHECK ((sha256(audience_bytes) = audience_hash))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_check3','CHECK ((source_hash = sha256(convert_to(((((''{"audience_hash":"''::text || encode(audience_hash, ''hex''::text)) || ''","format":"ortak-reviewed-conversation-source/1","source_evidence_hash":"''::text) || encode(source_evidence_hash, ''hex''::text)) || ''"}''::text), ''UTF8''::name))))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_kind_check','CHECK ((kind = ANY (ARRAY[''channel''::text, ''thread''::text])))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_provenance_bytes_check','CHECK (((octet_length(provenance_bytes) >= 1) AND (octet_length(provenance_bytes) <= 4096)))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_source_event_id_check','CHECK ((octet_length(source_event_id) = 32))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_audiences_source_hash_check','CHECK ((octet_length(source_hash) = 32))'),
        ('reviewed_memory_conversation_audiences','reviewed_memory_conversation_thread_root_event_created_at_check','CHECK (((thread_root_event_created_at IS NULL) OR ((thread_root_event_created_at >= ''1970-01-01 00:00:00+00''::timestamp with time zone) AND (thread_root_event_created_at < ''10000-01-01 00:00:00+00''::timestamp with time zone))))'),
        ('reviewed_memory_facts','reviewed_memory_facts_audience_kind_check','CHECK ((audience_kind = ANY (ARRAY[''project''::text, ''conversation''::text])))'),
        ('reviewed_memory_targets','conversation_target_selection_shape','CHECK ((((NOT conversation_consumption_enabled) OR (conversation_channel_id IS NOT NULL)) AND ((conversation_channel_id IS NOT NULL) OR (conversation_consumption_epoch = 0))))'),
        ('reviewed_memory_targets','reviewed_memory_targets_conversation_channel_id_check','CHECK (((conversation_channel_id IS NULL) OR (conversation_channel_id <> ''00000000-0000-0000-0000-000000000000''::uuid)))'),
        ('reviewed_memory_targets','reviewed_memory_targets_conversation_consumption_epoch_check','CHECK ((conversation_consumption_epoch >= 0))'),
        ('run_reviewed_memory_uses','conversation_use_pin_shape','CHECK ((((conversation_audience_hash IS NULL) AND (conversation_authority_epoch IS NULL) AND (conversation_consumption_epoch IS NULL)) OR ((conversation_audience_hash IS NOT NULL) AND (conversation_authority_epoch IS NOT NULL) AND (conversation_consumption_epoch IS NOT NULL) AND (consumption_epoch = 0))))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_conversation_audience_hash_check','CHECK ((octet_length(conversation_audience_hash) = 32))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_conversation_authority_epoch_check','CHECK ((conversation_authority_epoch >= 0))'),
        ('run_reviewed_memory_uses','run_reviewed_memory_uses_conversation_consumption_epoch_check','CHECK ((conversation_consumption_epoch >= 0))')
    ) AS required(table_name,constraint_name,definition) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass AND conname=selected.constraint_name) THEN
            EXECUTE format('ALTER TABLE %I ADD CONSTRAINT %I %s',selected.table_name,selected.constraint_name,selected.definition);
        END IF;
        IF NOT EXISTS(SELECT 1 FROM pg_constraint WHERE conrelid=selected.table_name::regclass AND conname=selected.constraint_name
            AND contype='c' AND convalidated AND NOT condeferrable AND NOT condeferred
            AND pg_get_constraintdef(oid,false)=selected.definition) THEN
            RAISE EXCEPTION 'ortak: conversation storage CHECK mismatch';
        END IF;
    END LOOP;
END
$ortak_75_checks$;

DO $ortak_75_triggers$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='conversation_memory_authorities'::regclass AND tgname='conversation_authority_guard') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_authority_guard
    BEFORE INSERT OR UPDATE OR DELETE ON conversation_memory_authorities
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_authority_guard();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='conversation_memory_authorities'::regclass AND tgname='conversation_authority_no_truncate') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_authority_no_truncate
    BEFORE TRUNCATE ON conversation_memory_authorities
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_conversation_audiences'::regclass AND tgname='conversation_audience_immutable') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_audience_immutable
    BEFORE UPDATE OR DELETE ON reviewed_memory_conversation_audiences
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_conversation_audiences'::regclass AND tgname='conversation_audience_no_truncate') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_audience_no_truncate
    BEFORE TRUNCATE ON reviewed_memory_conversation_audiences
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_facts'::regclass AND tgname='conversation_fact_storage_at_commit') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER conversation_fact_storage_at_commit
    AFTER INSERT ON reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='reviewed_memory_conversation_audiences'::regclass AND tgname='conversation_audience_storage_at_commit') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER conversation_audience_storage_at_commit
    AFTER INSERT ON reviewed_memory_conversation_audiences DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='run_reviewed_memory_uses'::regclass AND tgname='conversation_use_storage_at_commit') THEN
        EXECUTE $ddl$CREATE CONSTRAINT TRIGGER conversation_use_storage_at_commit
    AFTER INSERT ON run_reviewed_memory_uses DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_use_storage_at_commit();$ddl$;
    END IF;
END
$ortak_75_triggers$;

SELECT attach_community_write_fence('conversation_memory_authorities');
SELECT attach_community_write_fence('reviewed_memory_conversation_audiences');

DO $ortak_75_guards$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('conversation_memory_authorities','conversation_authority_guard','ortak_conversation_authority_guard',31,false),
        ('conversation_memory_authorities','conversation_authority_no_truncate','ortak_reject_office_truncate',34,false),
        ('reviewed_memory_conversation_audiences','conversation_audience_immutable','ortak_reject_row_mutation',27,false),
        ('reviewed_memory_conversation_audiences','conversation_audience_no_truncate','ortak_reject_office_truncate',34,false),
        ('reviewed_memory_facts','conversation_fact_storage_at_commit','ortak_conversation_fact_storage_at_commit',5,true),
        ('reviewed_memory_conversation_audiences','conversation_audience_storage_at_commit','ortak_conversation_fact_storage_at_commit',5,true),
        ('run_reviewed_memory_uses','conversation_use_storage_at_commit','ortak_conversation_use_storage_at_commit',5,true),
        ('conversation_memory_authorities','community_write_fence_conversation_memory_authorities','enforce_community_write_fence',31,false),
        ('reviewed_memory_conversation_audiences','community_write_fence_reviewed_memory_conversation_audiences','enforce_community_write_fence',31,false)
    ) AS required(table_name,trigger_name,function_name,trigger_type,is_deferred) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=selected.table_name::regclass
            AND t.tgname=selected.trigger_name AND t.tgfoid=(selected.function_name||'()')::regprocedure
            AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgtype=selected.trigger_type
            AND t.tgdeferrable=selected.is_deferred AND t.tginitdeferred=selected.is_deferred
            AND t.tgnargs=0 AND t.tgargs=decode('','hex') AND t.tgattr::text='' AND t.tgqual IS NULL) THEN
            RAISE EXCEPTION 'ortak: conversation storage trigger mismatch';
        END IF;
    END LOOP;
END
$ortak_75_guards$;


-- Migration75: exact scoped epoch functions and trigger metadata.
-- The neutral-thread desired-only stub is replaced before serving any writes.
CREATE OR REPLACE FUNCTION ortak_conversation_thread_insert_neutral75(proposed JSONB)
RETURNS BOOLEAN LANGUAGE sql STABLE STRICT AS $$
    SELECT proposed->>'parent_event_id' IS NULL
      AND proposed->>'parent_event_created_at' IS NULL
      AND (proposed->>'depth')::integer=0
      AND ((proposed->>'root_event_id' IS NULL AND proposed->>'root_event_created_at' IS NULL)
        OR (proposed->>'root_event_id'=proposed->>'event_id'
          AND (proposed->>'root_event_created_at')::timestamptz=(proposed->>'event_created_at')::timestamptz))
      AND EXISTS(SELECT 1 FROM events e
        WHERE e.community_id=(proposed->>'community_id')::uuid
          AND e.id=(proposed->>'event_id')::bytea
          AND e.created_at=(proposed->>'event_created_at')::timestamptz
          AND e.channel_id=(proposed->>'channel_id')::uuid
          AND e.kind IN(9,40002) AND e.deleted_at IS NULL
          AND jsonb_typeof(e.tags)='array'
          AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(
            CASE WHEN jsonb_typeof(e.tags)='array' THEN e.tags ELSE '[]'::jsonb END) t(tag)
            WHERE jsonb_typeof(t.tag)<>'array'
              OR EXISTS(SELECT 1 FROM jsonb_array_elements(
                CASE WHEN jsonb_typeof(t.tag)='array' THEN t.tag ELSE '[]'::jsonb END) p(part)
                WHERE jsonb_typeof(p.part)<>'string')
              OR (t.tag->>0='e' AND (t.tag->>3 IS DISTINCT FROM 'mention'
                OR coalesce(t.tag->>1,'') COLLATE "C" !~ '^[0-9a-fA-F]{64}$'))))
$$;

CREATE OR REPLACE FUNCTION ortak_fence_office_mutation() RETURNS TRIGGER
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP <> 'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP <> 'DELETE' THEN to_jsonb(NEW) END;
    target UUID;
    target_company UUID;
    field TEXT;
    changed BOOLEAN := TG_OP <> 'UPDATE';
BEGIN
    IF TG_OP = 'UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[1:TG_NARGS - 1] LOOP
            IF previous -> field IS DISTINCT FROM proposed -> field THEN
                changed := true;
                EXIT;
            END IF;
        END LOOP;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF TG_TABLE_NAME LIKE 'events%' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'thread_metadata' AND TG_OP = 'INSERT'
       AND ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'runs' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'outbox'
       AND NOT (COALESCE(previous ->> 'kind' = 'office_publish'
                         AND previous ->> 'signed_event_id' IS NOT NULL, false)
                OR COALESCE(proposed ->> 'kind' = 'office_publish'
                            AND proposed ->> 'signed_event_id' IS NOT NULL, false)) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF TG_ARGV[0] IN ('community', 'binding', 'community_root') THEN
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Office authority community mutation fence is busy'
                    USING ERRCODE = 'serialization_failure';
            END IF;
            SELECT company_id INTO target_company FROM office_company_bindings
             WHERE community_id = target;
            IF target_company IS NOT NULL THEN
                PERFORM ortak_advance_office_authority(target_company, TG_TABLE_NAME);
            END IF;
        END LOOP;
    END IF;
    IF TG_ARGV[0] IN ('company', 'binding', 'company_root') THEN
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            PERFORM ortak_advance_office_authority(target, TG_TABLE_NAME);
        END LOOP;
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;

CREATE OR REPLACE FUNCTION ortak_advance_conversation_scopes75(
    companies UUID[], communities UUID[], channels UUID[], projects UUID[],
    employees TEXT[], public_keys BYTEA[], selection TEXT, reason TEXT,
    office_fence BOOLEAN
) RETURNS VOID LANGUAGE plpgsql VOLATILE AS $$
DECLARE target UUID; project_key RECORD; keys JSONB; selected JSONB; key_hex TEXT[];
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Conversation authority requires READ COMMITTED isolation'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF companies IS NULL OR communities IS NULL OR channels IS NULL OR projects IS NULL
       OR employees IS NULL OR public_keys IS NULL OR selection IS NULL OR reason IS NULL OR office_fence IS NULL
       OR selection NOT IN('scope','channel','membership','project','identity','employee')
       OR reason NOT IN('channel_changed','membership_changed','project_changed','project_grant_changed',
            'event_changed','thread_changed','identity_changed','scope_closed')
       OR cardinality(companies)>2 OR cardinality(communities)>2 OR cardinality(channels)>2
       OR cardinality(projects)>2 OR cardinality(employees)>2 OR cardinality(public_keys)>2 THEN
        RAISE EXCEPTION 'Conversation mutation selection is invalid' USING ERRCODE='check_violation';
    END IF;
    IF office_fence THEN
        -- Acquire discovery's absent-row fence BEFORE selecting retained keys.
        -- Registration holds the matching Office shared locks. Try-locks retain
        -- 48's reverse-order refusal when the mutating tuple is already locked.
        FOR target IN SELECT DISTINCT v FROM unnest(communities) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation community mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
        FOR target IN SELECT DISTINCT v FROM unnest(companies) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    ELSE
        -- Project archive/binding/grant writers can run under signed shared
        -- Office authentication. NEVER upgrade that Office lock. Project row
        -- locking blocks both grant races and newly registered scope phantoms.
        IF selection<>'project' OR cardinality(companies)=0 OR cardinality(projects)=0 THEN
            RAISE EXCEPTION 'Conversation project mutation selection is invalid' USING ERRCODE='check_violation';
        END IF;
        FOR project_key IN SELECT DISTINCT c AS company_id,p AS project_id
            FROM unnest(companies) c CROSS JOIN unnest(projects) p ORDER BY c,p LOOP
            PERFORM 1 FROM public.projects p WHERE p.company_id=project_key.company_id
                AND p.id=project_key.project_id FOR UPDATE NOWAIT;
        END LOOP;
    END IF;
    SELECT coalesce(array_agg(encode(v,'hex')),ARRAY[]::text[]) INTO key_hex FROM unnest(public_keys) v;
    SELECT coalesce(jsonb_agg(to_jsonb(candidate) ORDER BY candidate.company_id,candidate.project_id,candidate.channel_id),'[]'::jsonb)
      INTO keys FROM (
        SELECT a.company_id,a.project_id,a.channel_id
        FROM conversation_memory_authorities a
        JOIN public.communities cm ON cm.id=a.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        WHERE (CASE WHEN cardinality(companies)>0 THEN a.company_id=ANY(companies)
                    ELSE a.community_id=ANY(communities) END)
          AND (selection='scope'
            OR (selection='project' AND a.project_id=ANY(projects))
            OR (selection IN('channel','membership') AND a.channel_id=ANY(channels))
            OR (selection IN('identity','employee','membership') AND (
                EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=a.community_id AND m.channel_id=a.channel_id
                    AND m.pubkey=ANY(public_keys))
                OR EXISTS(SELECT 1 FROM project_access_grants g WHERE g.company_id=a.company_id
                    AND g.project_id=a.project_id AND g.actor_pubkey=ANY(key_hex))
                OR EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences f
                    WHERE f.company_id=a.company_id AND f.project_id=a.project_id AND f.channel_id=a.channel_id
                      AND (f.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=f.company_id AND b.employee_id=f.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM reviewed_memory_targets t
                    WHERE t.company_id=a.company_id AND t.project_id=a.project_id AND t.conversation_channel_id=a.channel_id
                      AND (t.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=t.company_id AND b.employee_id=t.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM employee_office_bindings b JOIN channel_members m
                    ON m.community_id=a.community_id AND m.channel_id=a.channel_id AND m.pubkey=b.public_key
                    WHERE b.company_id=a.company_id AND b.employee_id=ANY(employees)))))
        ORDER BY a.company_id,a.project_id,a.channel_id LIMIT 513
      ) candidate;
    IF jsonb_array_length(keys)>512 THEN
        RAISE EXCEPTION 'Conversation mutation exceeds retained scope bound' USING ERRCODE='program_limit_exceeded';
    END IF;
    -- Retained mappings, not only current office_company_bindings, determine
    -- company identity. Closed communities were retired BEFORE their first
    -- close; later mutations leave that epoch/reason intact. They must not
    -- demand an old deletion lease or bypass the universal community fence.
    IF office_fence THEN
        FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation retained company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    END IF;
    FOR selected IN SELECT v FROM jsonb_array_elements(keys) v LOOP
        PERFORM 1 FROM public.projects p WHERE p.company_id=(selected->>'company_id')::uuid
            AND p.id=(selected->>'project_id')::uuid FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid FOR UPDATE NOWAIT;
        UPDATE conversation_memory_authorities a SET epoch=a.epoch+1,last_change_reason=reason
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
END
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_epoch_mutation75() RETURNS TRIGGER LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP<>'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP<>'DELETE' THEN to_jsonb(NEW) END;
    fields TEXT[]; field TEXT; changed BOOLEAN := TG_OP<>'UPDATE';
    companies UUID[]; communities UUID[]; channels UUID[]; projects UUID[];
    employees TEXT[]; public_keys BYTEA[];
    kind TEXT := TG_ARGV[0]; selection TEXT; reason TEXT; office_fence BOOLEAN := true;
    old_manifest JSONB; new_manifest JSONB;
BEGIN
    CASE kind
    WHEN 'channel' THEN fields:=ARRAY['community_id','id','channel_type','visibility','archived_at','deleted_at','participant_hash','ttl_seconds','ttl_deadline']; selection:='channel'; reason:='channel_changed';
    WHEN 'membership' THEN fields:=ARRAY['community_id','channel_id','pubkey','role','removed_at']; selection:='membership'; reason:='membership_changed';
    WHEN 'event' THEN fields:=ARRAY['community_id','id','created_at','pubkey','kind','tags','content','sig','channel_id','deleted_at']; selection:='channel'; reason:='event_changed';
    WHEN 'thread' THEN fields:=ARRAY['community_id','event_id','event_created_at','channel_id','parent_event_id','parent_event_created_at','root_event_id','root_event_created_at','depth']; selection:='channel'; reason:='thread_changed';
    WHEN 'inbox' THEN fields:=ARRAY['company_id','event_id','event_created_at','event_kind','author_pubkey','channel_id','state']; selection:='channel'; reason:='event_changed';
    WHEN 'project' THEN fields:=ARRAY['company_id','id','status','archived_at']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'project_binding' THEN fields:=ARRAY['company_id','project_id','community_id','channel_id']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'grant' THEN fields:=ARRAY['company_id','project_id','actor_pubkey','role','revoked_at']; selection:='project'; reason:='project_grant_changed'; office_fence:=false;
    WHEN 'user' THEN fields:=ARRAY['community_id','pubkey','agent_type','agent_owner_pubkey','deactivated_at']; selection:='identity'; reason:='identity_changed';
    WHEN 'employee' THEN fields:=ARRAY['company_id','id','status','active_revision_id']; selection:='employee'; reason:='identity_changed';
    WHEN 'office_identity' THEN fields:=ARRAY['company_id','employee_id','public_key','signer_ref','valid_from','valid_until']; selection:='employee'; reason:='identity_changed';
    WHEN 'memory_identity' THEN fields:=ARRAY['company_id','employee_id','revision_id','adapter','endpoint_ref','workspace','user_peer','employee_peer','options']; selection:='employee'; reason:='identity_changed';
    WHEN 'company' THEN fields:=ARRAY['id','status']; selection:='scope'; reason:='scope_closed';
    WHEN 'community' THEN fields:=ARRAY['id','deletion_state','deletion_fence_generation','deleted_at']; selection:='scope'; reason:='scope_closed';
    WHEN 'company_binding' THEN fields:=ARRAY['company_id','community_id']; selection:='scope'; reason:='scope_closed';
    ELSE RAISE EXCEPTION 'Conversation mutation kind is invalid' USING ERRCODE='check_violation';
    END CASE;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY fields LOOP
            IF previous->field IS DISTINCT FROM proposed->field THEN changed:=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN changed:=changed OR ((previous->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL)); END IF;
        IF kind='memory_identity' THEN changed:=changed OR ((previous->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL)); END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(previous->>'deletion_state','')<>'active' THEN
        -- The first transition out of active retired every scope under the
        -- Office exclusive fence. Later closure stages and return to active
        -- never lower that epoch, so no old use can revive. Run this hook
        -- BEFORE the first close while the universal write fence allows it.
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- Exclude the just-inserted metadata row from its own reference proof.
        -- Child/root indexes cover a channel fact whose canonical root is not
        -- retained in its channel-wide audience. New unrelated replies have
        -- neither retained anchors nor existing descendants and do not bump.
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
            WHERE a.community_id=(proposed->>'community_id')::uuid AND
              ((a.source_event_id=(proposed->>'event_id')::bytea AND a.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
                OR (a.thread_root_event_id=(proposed->>'event_id')::bytea AND a.thread_root_event_created_at=(proposed->>'event_created_at')::timestamptz)))
           AND NOT EXISTS(SELECT 1 FROM thread_metadata t WHERE t.community_id=(proposed->>'community_id')::uuid
             AND (t.event_id,t.event_created_at) IS DISTINCT FROM ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
             AND ((t.parent_event_id=(proposed->>'event_id')::bytea AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
               OR (t.root_event_id=(proposed->>'event_id')::bytea AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN RETURN NEW; END IF;
    END IF;
    IF kind='inbox' AND coalesce(previous->>'state','')<>'decided'
       AND NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
         WHERE (a.company_id,a.source_event_id,a.source_event_created_at) IN (
           ((previous->>'company_id')::uuid,(previous->>'event_id')::bytea,(previous->>'event_created_at')::timestamptz),
           ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE' AND previous->>'company_id'=proposed->>'company_id'
       AND previous->>'id'=proposed->>'id' AND previous->>'status'=proposed->>'status' THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_manifest
            FROM employee_revisions r WHERE r.company_id=(previous->>'company_id')::uuid AND r.employee_id=previous->>'id'
                AND r.id=(previous->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_manifest
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid AND r.employee_id=proposed->>'id'
                AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_manifest IS NOT NULL AND old_manifest IS NOT DISTINCT FROM new_manifest THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM public.employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN (
          ((previous->>'company_id')::uuid,previous->>'employee_id',(previous->>'revision_id')::uuid),
          ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
       AND coalesce(proposed,previous)->>'agent_type' IS NULL
       AND coalesce(proposed,previous)->>'agent_owner_pubkey' IS NULL
       AND coalesce(proposed,previous)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    -- Ordinary first joins cannot invalidate a previously authorized reader;
    -- bot insertion is different: the resolver treats that key as automated
    -- across its entire community, including other retained channels/projects.
    IF kind='membership' AND TG_OP='INSERT' AND proposed->>'role'<>'bot' THEN RETURN NEW; END IF;

    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO companies FROM (VALUES
        (previous->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO communities FROM (VALUES
        (previous->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (previous->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO projects FROM (VALUES
        (previous->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END),
        (proposed->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employees FROM (VALUES
        (previous->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::bytea),ARRAY[]::bytea[]) INTO public_keys FROM (VALUES
        (previous->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END),
        (proposed->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END)) t(v) WHERE v IS NOT NULL;
    IF kind='membership' AND coalesce(previous->>'role','')<>'bot' AND coalesce(proposed->>'role','')<>'bot' THEN
        public_keys:=ARRAY[]::bytea[];
    END IF;
    PERFORM ortak_advance_conversation_scopes75(companies,communities,channels,projects,employees,public_keys,selection,reason,office_fence);
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END
$$;

-- Only the exact migration48 thread trigger is a recognized predecessor.
-- Unknown existing functions, flags, columns or arguments remain a refusal.
DO $ortak_75_epoch_triggers$
BEGIN
    IF EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='thread_metadata'::regclass
        AND tgname='ortak_office_authority_thread_metadata' AND tgenabled='O' AND tgtype=31
        AND NOT tgisinternal AND NOT tgdeferrable AND NOT tginitdeferred
        AND tgnargs=6 AND tgattr::text='' AND tgqual IS NULL
        AND tgfoid='ortak_fence_office_mutation()'::regprocedure
        AND tgargs=decode('636f6d6d756e69747900636f6d6d756e6974795f6964006576656e745f6964006576656e745f637265617465645f617400706172656e745f6576656e745f696400706172656e745f6576656e745f637265617465645f617400','hex')) THEN
        DROP TRIGGER ortak_office_authority_thread_metadata ON thread_metadata;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='thread_metadata'::regclass AND tgname='ortak_office_authority_thread_metadata') THEN
        EXECUTE $ddl$CREATE TRIGGER ortak_office_authority_thread_metadata BEFORE INSERT OR UPDATE OR DELETE ON thread_metadata
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community','community_id','event_id',
    'event_created_at','channel_id','parent_event_id','parent_event_created_at',
    'root_event_id','root_event_created_at','depth');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channels'::regclass AND tgname='conversation_epoch_channels') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_channels AFTER INSERT OR UPDATE OR DELETE ON channels FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('channel');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='channel_members'::regclass AND tgname='conversation_epoch_members') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_members AFTER INSERT OR UPDATE OR DELETE ON channel_members FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('membership');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='events'::regclass AND tgname='conversation_epoch_events') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_events AFTER UPDATE OR DELETE ON events FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('event');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='thread_metadata'::regclass AND tgname='conversation_epoch_threads') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_threads AFTER INSERT OR UPDATE OR DELETE ON thread_metadata FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('thread');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='office_inbox'::regclass AND tgname='conversation_epoch_inbox') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_inbox AFTER UPDATE OR DELETE ON office_inbox FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('inbox');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='projects'::regclass AND tgname='conversation_epoch_projects') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_projects AFTER UPDATE OR DELETE ON projects FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='project_api_bindings'::regclass AND tgname='conversation_epoch_project_bindings') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_project_bindings AFTER INSERT OR UPDATE OR DELETE ON project_api_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project_binding');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='project_access_grants'::regclass AND tgname='conversation_epoch_grants') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_grants AFTER INSERT OR UPDATE OR DELETE ON project_access_grants FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('grant');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='users'::regclass AND tgname='conversation_epoch_users') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_users AFTER INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('user');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='employees'::regclass AND tgname='conversation_epoch_employees') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_employees AFTER INSERT OR UPDATE OR DELETE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('employee');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='employee_office_bindings'::regclass AND tgname='conversation_epoch_office_identities') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_office_identities AFTER INSERT OR UPDATE OR DELETE ON employee_office_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('office_identity');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='employee_memory_bindings'::regclass AND tgname='conversation_epoch_memory_identities') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_memory_identities AFTER INSERT OR UPDATE OR DELETE ON employee_memory_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('memory_identity');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='companies'::regclass AND tgname='conversation_epoch_companies') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_companies AFTER UPDATE OR DELETE ON companies FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='communities'::regclass AND tgname='ortak_z_conversation_epoch_communities') THEN
        EXECUTE $ddl$CREATE TRIGGER ortak_z_conversation_epoch_communities BEFORE UPDATE OR DELETE ON communities FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('community');$ddl$;
    END IF;
    IF NOT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='office_company_bindings'::regclass AND tgname='conversation_epoch_company_bindings') THEN
        EXECUTE $ddl$CREATE TRIGGER conversation_epoch_company_bindings AFTER INSERT OR UPDATE OR DELETE ON office_company_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company_binding');$ddl$;
    END IF;
END
$ortak_75_epoch_triggers$;

DO $ortak_75_epoch_guards$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('thread_metadata','ortak_office_authority_thread_metadata','ortak_fence_office_mutation',31,10,'636f6d6d756e69747900636f6d6d756e6974795f6964006576656e745f6964006576656e745f637265617465645f6174006368616e6e656c5f696400706172656e745f6576656e745f696400706172656e745f6576656e745f637265617465645f617400726f6f745f6576656e745f696400726f6f745f6576656e745f637265617465645f617400646570746800'),
        ('channels','conversation_epoch_channels','ortak_conversation_epoch_mutation75',29,1,'6368616e6e656c00'),
        ('channel_members','conversation_epoch_members','ortak_conversation_epoch_mutation75',29,1,'6d656d6265727368697000'),
        ('events','conversation_epoch_events','ortak_conversation_epoch_mutation75',25,1,'6576656e7400'),
        ('thread_metadata','conversation_epoch_threads','ortak_conversation_epoch_mutation75',29,1,'74687265616400'),
        ('office_inbox','conversation_epoch_inbox','ortak_conversation_epoch_mutation75',25,1,'696e626f7800'),
        ('projects','conversation_epoch_projects','ortak_conversation_epoch_mutation75',25,1,'70726f6a65637400'),
        ('project_api_bindings','conversation_epoch_project_bindings','ortak_conversation_epoch_mutation75',29,1,'70726f6a6563745f62696e64696e6700'),
        ('project_access_grants','conversation_epoch_grants','ortak_conversation_epoch_mutation75',29,1,'6772616e7400'),
        ('users','conversation_epoch_users','ortak_conversation_epoch_mutation75',29,1,'7573657200'),
        ('employees','conversation_epoch_employees','ortak_conversation_epoch_mutation75',29,1,'656d706c6f79656500'),
        ('employee_office_bindings','conversation_epoch_office_identities','ortak_conversation_epoch_mutation75',29,1,'6f66666963655f6964656e7469747900'),
        ('employee_memory_bindings','conversation_epoch_memory_identities','ortak_conversation_epoch_mutation75',29,1,'6d656d6f72795f6964656e7469747900'),
        ('companies','conversation_epoch_companies','ortak_conversation_epoch_mutation75',25,1,'636f6d70616e7900'),
        ('communities','ortak_z_conversation_epoch_communities','ortak_conversation_epoch_mutation75',27,1,'636f6d6d756e69747900'),
        ('office_company_bindings','conversation_epoch_company_bindings','ortak_conversation_epoch_mutation75',29,1,'636f6d70616e795f62696e64696e6700')
    ) AS required(table_name,trigger_name,function_name,trigger_type,arg_count,arg_hex) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=selected.table_name::regclass
            AND t.tgname=selected.trigger_name AND t.tgfoid=(selected.function_name||'()')::regprocedure
            AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgtype=selected.trigger_type
            AND NOT t.tgdeferrable AND NOT t.tginitdeferred
            AND t.tgnargs=selected.arg_count AND t.tgargs=decode(selected.arg_hex,'hex')
            AND t.tgattr::text='' AND t.tgqual IS NULL) THEN
            RAISE EXCEPTION 'ortak: conversation epoch trigger mismatch';
        END IF;
    END LOOP;
END
$ortak_75_epoch_guards$;

-- These are ordinary nonunique btree indexes; no existing index is replaced.
CREATE INDEX IF NOT EXISTS idx_conversation_thread_parent_exact
    ON thread_metadata(community_id,parent_event_id,parent_event_created_at)
    WHERE parent_event_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conversation_thread_root_exact
    ON thread_metadata(community_id,root_event_id,root_event_created_at)
    WHERE root_event_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conversation_office_employee_keys
    ON employee_office_bindings(company_id,employee_id,public_key);
DO $ortak_75_epoch_indexes$
DECLARE selected RECORD;
BEGIN
    FOR selected IN SELECT * FROM (VALUES
        ('idx_conversation_thread_parent_exact','thread_metadata',ARRAY['community_id','parent_event_id','parent_event_created_at'],'(parent_event_id IS NOT NULL)'),
        ('idx_conversation_thread_root_exact','thread_metadata',ARRAY['community_id','root_event_id','root_event_created_at'],'(root_event_id IS NOT NULL)'),
        ('idx_conversation_office_employee_keys','employee_office_bindings',ARRAY['company_id','employee_id','public_key'],NULL)
    ) AS required(index_name,table_name,columns,predicate) LOOP
        IF NOT EXISTS(SELECT 1 FROM pg_index i JOIN pg_class idx ON idx.oid=i.indexrelid
            JOIN pg_am am ON am.oid=idx.relam
            WHERE i.indexrelid=selected.index_name::regclass AND i.indrelid=selected.table_name::regclass
              AND idx.relnamespace='public'::regnamespace AND am.amname='btree'
              AND NOT i.indisunique AND NOT i.indisprimary AND NOT i.indisexclusion
              AND i.indisvalid AND i.indisready AND i.indislive AND i.indimmediate
              AND i.indnatts=3 AND i.indnkeyatts=3 AND i.indexprs IS NULL
              AND i.indoption::text='0 0 0' AND idx.reloptions IS NULL
              AND (SELECT array_agg(a.attname::text ORDER BY k.ordinality)
                   FROM unnest(i.indkey::smallint[]) WITH ORDINALITY k(attnum,ordinality)
                   JOIN pg_attribute a ON a.attrelid=i.indrelid AND a.attnum=k.attnum)=selected.columns
              AND pg_get_expr(i.indpred,i.indrelid,false) IS NOT DISTINCT FROM selected.predicate) THEN
            RAISE EXCEPTION 'ortak: conversation epoch index mismatch';
        END IF;
    END LOOP;
END
$ortak_75_epoch_indexes$;

-- Exact immutable76 function bodies, ordered before their dependent SQL callers.































-- Exact76 functions in reviewed dependency order, including named-dollar legacy definitions.
CREATE OR REPLACE FUNCTION ortak_conversation_run_origin(company UUID, run UUID, project UUID)
RETURNS TABLE(requester_public_key BYTEA, provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    WITH base AS MATERIALIZED (
        SELECT r.*, b.community_id,b.channel_id AS project_channel_id,active.manifest AS active_manifest,
            pinned.manifest AS pinned_manifest
        FROM runs r
        JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
            AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_revisions pinned ON pinned.company_id=r.company_id
            AND pinned.employee_id=r.employee_id AND pinned.id=r.employee_revision_id
        JOIN employee_revisions active ON active.company_id=e.company_id
            AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN project_api_bindings b ON b.company_id=r.company_id AND b.project_id=project
        JOIN office_routing_cohorts cohort ON cohort.company_id=r.company_id
            AND cohort.community_id=b.community_id AND cohort.state='enabled'
        JOIN office_routing_channels ch ON ch.company_id=cohort.company_id
            AND ch.community_id=cohort.community_id AND ch.channel_id=b.channel_id
        JOIN office_routing_employees selected ON selected.company_id=r.company_id
            AND selected.employee_id=r.employee_id
        WHERE r.company_id=company AND r.id=run
            AND pinned.manifest->'office'=active.manifest->'office'
            AND pinned.manifest->'memory'=active.manifest->'memory'
            AND NOT EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
            AND NOT EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=r.company_id AND c.run_id=r.id)
    ), origins AS (
        SELECT i.author_pubkey AS human,r.message_id AS source,r.employee_id
        FROM base r
        JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
            AND d.message_id=r.message_id AND d.root_message_id=r.root_message_id
            AND d.origin_type='human' AND d.office_authority_generation IS NOT NULL
            AND d.office_input_hash IS NOT NULL
        JOIN routing_recipients recipient ON recipient.company_id=r.company_id
            AND recipient.routing_decision_id=d.id AND recipient.employee_id=r.employee_id
            AND recipient.action='wake' AND recipient.employee_revision_id=r.employee_revision_id
            AND recipient.employee_lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN delivery_chain_visits visit ON visit.company_id=r.company_id
            AND visit.root_message_id=d.root_message_id AND visit.employee_id=r.employee_id
            AND visit.routing_decision_id=d.id
        JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
            AND i.channel_id=r.project_channel_id AND i.state='decided'
            AND d.origin_id=encode(i.author_pubkey,'hex')
        WHERE r.work_item_id IS NULL AND r.status IN('queued','running','waiting','completed')
          -- The runtime configured exactly one project for this employee's
          -- channel. Multiple live advertisements must not make a caller's
          -- arbitrary project parameter a choice of conversation namespace.
          AND (SELECT count(DISTINCT t.project_id) FROM reviewed_memory_targets t
            WHERE t.company_id=r.company_id AND t.community_id=r.community_id
              AND t.employee_id=r.employee_id AND t.conversation_channel_id=r.project_channel_id
              AND t.enabled AND t.conversation_consumption_enabled
              AND t.valid_until>clock_timestamp())=1
          AND EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=r.company_id
            AND t.project_id=project AND t.employee_id=r.employee_id AND t.conversation_channel_id=r.project_channel_id
            AND t.enabled AND t.conversation_consumption_enabled AND t.valid_until>clock_timestamp())
        UNION ALL
        SELECT decode(x.requested_by,'hex'), w.source_message_id, r.employee_id
        FROM base r JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
            AND x.project_id=project AND x.work_item_id=r.work_item_id
            AND x.employee_id=r.employee_id AND x.employee_revision_id=r.employee_revision_id
        JOIN work_items w ON w.company_id=x.company_id AND w.project_id=x.project_id AND w.id=x.work_item_id
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants acl ON acl.company_id=x.company_id AND acl.project_id=x.project_id
            AND acl.actor_pubkey=x.requested_by AND acl.role IN('owner','contributor') AND acl.revoked_at IS NULL
        WHERE w.source_message_id IS NOT NULL AND r.routing_decision_id IS NULL
            AND r.message_id IS NULL AND r.root_message_id IS NULL
            AND EXISTS(SELECT 1 FROM work_assignments a WHERE a.company_id=x.company_id
                AND a.work_item_id=x.work_item_id AND a.employee_id=x.employee_id
                AND a.status='active' AND a.role IN('owner','contributor'))
            AND ((w.state='in_progress' AND w.version=x.execution_version
                AND x.reconciled_at IS NULL AND r.status IN('queued','running','waiting','completed')
                AND (r.work_admission_generation=g.generation OR r.status='queued' AND r.work_admission_generation IS NULL)
                AND NOT EXISTS(SELECT 1 FROM work_dependencies d JOIN work_items dependency
                    ON dependency.company_id=d.company_id AND dependency.id=d.depends_on_work_item_id
                    WHERE d.company_id=x.company_id AND d.work_item_id=x.work_item_id AND d.released_at IS NULL
                        AND dependency.state NOT IN('completed','cancelled'))
                AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria c WHERE c.company_id=x.company_id
                    AND c.work_item_id=x.work_item_id AND c.status<>'pending')
                AND NOT EXISTS(SELECT 1 FROM work_approvals a WHERE a.company_id=x.company_id
                    AND a.work_item_id=x.work_item_id AND a.status<>'pending'))
              -- A materialized result remains inspectable after human review.
              -- This branch cannot create a new run or first artifact: both
              -- exact retained artifact and materialized output must exist.
              OR (r.status='completed' AND w.state IN('review','completed') AND x.result_code='result_ready'
                AND x.reconciled_at IS NOT NULL AND EXISTS(SELECT 1 FROM runtime_work_outputs output
                    JOIN artifacts artifact ON artifact.company_id=output.company_id AND artifact.id=output.artifact_id
                        AND artifact.run_id=output.run_id AND artifact.project_id=x.project_id AND artifact.work_item_id=x.work_item_id
                    WHERE output.company_id=r.company_id AND output.run_id=r.id AND output.state='materialized')))
    ), unique_origin AS (
        SELECT * FROM origins WHERE (SELECT count(*) FROM origins)=1
    )
    SELECT o.human, s.provenance_bytes, s.observed_at, s.valid_before
    FROM unique_origin o CROSS JOIN LATERAL
        ortak_conversation_source_observation(company,project,o.employee_id,o.human,o.source,'thread') s
    WHERE s.valid_before IS NULL OR s.valid_before>clock_timestamp()
$$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_source_hash(f reviewed_memory_facts)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN f.audience_kind='conversation' THEN
        (SELECT a.source_hash FROM reviewed_memory_conversation_audiences a
            WHERE a.company_id=f.company_id AND a.fact_id=f.id)
    WHEN f.source_message_id IS NOT NULL
        THEN sha256(convert_to('message:'||encode(f.source_message_id,'hex'),'UTF8'))
    ELSE (SELECT sha256(convert_to('artifact:'||a.id::text||':'||encode(a.content_hash,'hex'),'UTF8'))
        FROM artifacts a WHERE a.company_id=f.company_id AND a.id=f.source_artifact_id) END
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_target_eligible76(company UUID, fact UUID, target UUID, publication BOOLEAN)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=target
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id AND e.status='active'
        JOIN employee_revisions revision ON revision.company_id=e.company_id
            AND revision.employee_id=e.id AND revision.id=e.active_revision_id
        JOIN employee_memory_bindings memory ON memory.company_id=e.company_id
            AND memory.employee_id=e.id AND memory.revision_id=e.active_revision_id
        CROSS JOIN LATERAL ortak_conversation_source_observation(f.company_id,f.project_id,f.employee_id,
            decode(f.approved_by,'hex'),f.source_message_id,a.kind) source
        WHERE publication IS NOT NULL AND f.company_id=company AND f.id=fact AND f.audience_kind='conversation'
            AND f.version=1 AND f.revoked_at IS NULL AND f.expires_at>clock_timestamp()
            AND a.community_id=f.community_id AND a.project_id=f.project_id AND a.employee_id=f.employee_id
            AND source.provenance_bytes=a.provenance_bytes AND source.source_hash=a.source_hash
            AND source.audience_hash=a.audience_hash
            AND (source.valid_before IS NULL OR source.valid_before>clock_timestamp())
            AND t.enabled AND t.valid_until>clock_timestamp()
            AND t.community_id=f.community_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
            AND t.conversation_channel_id=a.channel_id
            AND (NOT publication OR t.employee_revision_id=e.active_revision_id)
            AND t.employee_lifecycle_epoch=e.lifecycle_epoch AND memory.validated_at IS NOT NULL
            AND t.binding=revision.manifest->'memory'
            AND t.binding=jsonb_build_object('adapter',memory.adapter,'endpoint_ref',memory.endpoint_ref,
                'workspace',memory.workspace,'user_peer',memory.user_peer,'employee_peer',memory.employee_peer,'options',memory.options)
            AND t.creation_receipt->'binding'=t.binding
            AND t.creation_receipt->>'company_id'=company::text
            AND t.creation_receipt->>'employee_id'=f.employee_id
            AND t.creation_receipt->>'deployment_id'=t.deployment_id::text)
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_conversation_target_eligible76(company,fact,target,true)
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    authority_epoch BIGINT, consumption_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
        JOIN conversation_memory_authorities authority ON authority.company_id=a.company_id
            AND authority.community_id=a.community_id AND authority.project_id=a.project_id AND authority.channel_id=a.channel_id
        JOIN reviewed_memory_exports export ON export.company_id=f.company_id AND export.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=export.company_id AND t.id=export.target_id
        JOIN reviewed_memory_export_receipts ack ON ack.company_id=f.company_id AND ack.fact_id=f.id AND ack.action='publish'
        JOIN runs r ON r.company_id=f.company_id AND r.id=run AND r.employee_id=f.employee_id
        CROSS JOIN LATERAL ortak_conversation_run_origin(company,run,f.project_id) origin
        CROSS JOIN LATERAL (SELECT convert_from(origin.provenance_bytes,'UTF8')::jsonb AS value) op
        WHERE f.company_id=company AND f.id=fact AND t.id=target
          AND ortak_conversation_target_eligible76(company,fact,target,false)
          AND authority.epoch=$5 AND t.conversation_consumption_epoch=$6
          AND t.conversation_consumption_enabled AND t.conversation_channel_id=a.channel_id
          AND op.value#>>'{audience,company_id}'=company::text
          AND op.value#>>'{audience,community_id}'=a.community_id::text
          AND op.value#>>'{audience,project_id}'=a.project_id::text
          AND op.value#>>'{audience,employee_id}'=a.employee_id
          AND op.value#>>'{audience,channel_id}'=a.channel_id::text
          AND (a.kind='channel' OR
            (op.value#>>'{audience,thread_root_event_id}'=encode(a.thread_root_event_id,'hex')
              AND (op.value#>>'{audience,thread_root_event_created_at}')::timestamptz=a.thread_root_event_created_at))
          AND export.community_id=f.community_id AND export.project_id=f.project_id AND export.employee_id=f.employee_id
          AND export.content_hash=sha256(convert_to(f.content,'UTF8')) AND export.source_hash=a.source_hash
          AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store
          AND ack.binding_hash=t.binding_hash AND ack.content_hash=export.content_hash
          AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop
            WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw'))
$$;

CREATE OR REPLACE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_run_employee_memory_current(company,run) AND NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
        LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
        LEFT JOIN work_executions wx ON wx.company_id=r.company_id AND wx.run_id=r.id
        LEFT JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        LEFT JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        LEFT JOIN run_context_snapshots snapshot ON snapshot.company_id=u.company_id AND snapshot.run_id=u.run_id
        WHERE u.company_id=company AND u.run_id=run AND (
            r.id IS NULL OR f.id IS NULL OR t.id IS NULL OR snapshot.run_id IS NULL
            OR f.employee_id IS DISTINCT FROM r.employee_id OR f.community_id IS DISTINCT FROM u.community_id
            OR f.version IS DISTINCT FROM u.fact_version OR f.promotion_operation_id IS DISTINCT FROM u.approval_id
            OR f.approved_by IS DISTINCT FROM u.approved_by OR f.expires_at IS DISTINCT FROM u.expires_at
            OR sha256(convert_to(f.content,'UTF8')) IS DISTINCT FROM u.content_hash
            OR ortak_reviewed_export_source_hash(f) IS DISTINCT FROM u.source_hash OR t.binding_hash IS DISTINCT FROM u.binding_hash
            OR CASE WHEN f.audience_kind='project' THEN
                wx.run_id IS NULL OR f.project_id IS DISTINCT FROM wx.project_id
                OR NOT ortak_reviewed_runtime_eligible(company,u.fact_id,u.target_id,u.consumption_epoch)
              WHEN f.audience_kind='conversation' THEN
                u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM
                    (SELECT a.audience_hash FROM reviewed_memory_conversation_audiences a WHERE a.company_id=company AND a.fact_id=u.fact_id)
                OR NOT coalesce(ortak_conversation_runtime_eligible(company,run,u.fact_id,u.target_id,
                    u.conversation_authority_epoch,u.conversation_consumption_epoch),false)
                OR NOT EXISTS(SELECT 1 FROM ortak_conversation_run_origin(company,run,f.project_id) origin
                    WHERE (CASE WHEN ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)->'version'='5'::jsonb
                        THEN ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)#>'{employee,conversation_origin}'
                        ELSE ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)#>'{conversation,origin}' END)
                        =ortak_snapshot_scratch_jsonb(jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
                            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json))
              ELSE true END))
$$;

CREATE OR REPLACE FUNCTION ortak_lock_run_reviewed_memory(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM p.id FROM projects p WHERE p.company_id=company AND p.id IN
        (SELECT f.project_id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u
            ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run)
        ORDER BY p.id FOR SHARE OF p NOWAIT;
    PERFORM w.id FROM work_items w JOIN work_executions x ON x.company_id=w.company_id AND x.work_item_id=w.id
        WHERE x.company_id=company AND x.run_id=run ORDER BY w.id FOR SHARE OF w NOWAIT;
    PERFORM a.channel_id FROM conversation_memory_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_reviewed_memory_uses u JOIN reviewed_memory_conversation_audiences f
            ON f.company_id=u.company_id AND f.fact_id=u.fact_id WHERE u.company_id=company AND u.run_id=run
                AND f.project_id=a.project_id AND f.channel_id=a.channel_id)
        ORDER BY a.company_id,a.project_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM a.channel_id FROM employee_memory_channel_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
            JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
            CROSS JOIN LATERAL ortak_employee_memory_run_origin(company,run,f.destination_channel_id) origin
            WHERE u.company_id=company AND u.run_id=run AND f.employee_id=a.employee_id
                AND f.community_id=a.community_id AND (a.channel_id IN(f.source_channel_id,f.destination_channel_id)
                    OR a.channel_id=(convert_from(origin.origin_bytes,'UTF8')::jsonb#>>'{source,channel_id}')::uuid))
        ORDER BY a.employee_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM f.id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u ON u.company_id=f.company_id AND u.fact_id=f.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    PERFORM f.id FROM employee_reviewed_memory_facts f JOIN run_employee_reviewed_memory_uses u
        ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run
        ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM employee_reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    RETURN ortak_run_reviewed_memory_current(company,run);
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selected_run UUID; conversation BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='runs' THEN selected_run=NEW.id; ELSE selected_run=NEW.run_id; END IF;
    SELECT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=selected_run AND u.conversation_audience_hash IS NOT NULL) OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        WHERE u.company_id=NEW.company_id AND u.run_id=selected_run) INTO conversation;
    IF TG_TABLE_NAME='runs' THEN
        IF NOT conversation THEN
            -- Preserve the reviewed-project admission trigger's legacy effect.
            IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token THEN RETURN NEW; END IF;
        ELSE
            IF (NEW.office_admission_token,NEW.office_admission_generation,NEW.office_admission_valid_before,
                NEW.work_admission_token,NEW.work_admission_generation,NEW.runtime_run_ref)
              IS NOT DISTINCT FROM
               (OLD.office_admission_token,OLD.office_admission_generation,OLD.office_admission_valid_before,
                OLD.work_admission_token,OLD.work_admission_generation,OLD.runtime_run_ref) THEN RETURN NEW; END IF;
            -- Exact74 lost-start ACK correlation is accounting after confirmed
            -- stop; no new token, output, bytes or active status can ride along.
            IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
                AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
                AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                    AND (c.state='acknowledged' OR c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp()))
                AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader
                    WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN RETURN NEW; END IF;
            IF NEW.status NOT IN('queued','running','waiting') THEN
                RAISE EXCEPTION 'ortak: terminal conversation run cannot gain fresh admission' USING ERRCODE='check_violation';
            END IF;
        END IF;
    END IF;
    IF NOT ortak_run_reviewed_memory_current(NEW.company_id,selected_run) THEN
        RAISE EXCEPTION 'ortak: reviewed memory use no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_conversation_effect_admission76() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN=false; previous JSONB; proposed JSONB;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=NEW.run_id AND u.conversation_audience_hash IS NOT NULL)
        AND NOT EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
            AND u.run_id=NEW.run_id) THEN RETURN NEW; END IF;
    previous=CASE WHEN TG_OP='UPDATE' THEN to_jsonb(OLD) END; proposed=to_jsonb(NEW);
    CASE TG_TABLE_NAME
    WHEN 'runtime_work_outputs' THEN effect=NEW.state='materialized';
    WHEN 'runtime_office_outputs' THEN effect=NEW.state='enqueued' OR
        (NEW.office_authority_token IS NOT NULL AND (TG_OP='INSERT'
            OR (proposed->'office_authority_token',proposed->'office_authority_generation',proposed->'office_authority_valid_before')
              IS DISTINCT FROM (previous->'office_authority_token',previous->'office_authority_generation',previous->'office_authority_valid_before')));
    WHEN 'runtime_memory_writes' THEN effect=NEW.state='pending' AND NEW.admission_token IS NOT NULL
        AND (TG_OP='INSERT' OR (proposed->'admission_token',proposed->'admission_generation',proposed->'admission_valid_before')
            IS DISTINCT FROM (previous->'admission_token',previous->'admission_generation',previous->'admission_valid_before'));
    WHEN 'outbox' THEN effect=NEW.kind='office_publish' AND NEW.state='pending'
        AND (TG_OP='INSERT' OR (proposed->'signed_event_id',proposed->'signed_event_bytes')
            IS DISTINCT FROM (previous->'signed_event_id',previous->'signed_event_bytes'));
    ELSE RAISE EXCEPTION 'ortak: unknown conversation effect' USING ERRCODE='check_violation';
    END CASE;
    IF effect AND NOT ortak_run_reviewed_memory_current(NEW.company_id,NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: conversation output authority changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_view(company UUID,fact UUID) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object('fact_id',x.fact_id,'runtime_consumption_enabled',
        CASE WHEN f.audience_kind='conversation' THEN
            t.conversation_consumption_enabled AND ortak_conversation_target_eligible76(company,fact,t.id,false)
            AND EXISTS(SELECT 1 FROM reviewed_memory_export_receipts ack WHERE ack.company_id=x.company_id
                AND ack.fact_id=x.fact_id AND ack.action='publish' AND ack.remote_status='active'
                AND NOT ack.erased_from_reviewed_store AND ack.binding_hash=t.binding_hash
                AND ack.content_hash=x.content_hash AND x.content_hash=sha256(convert_to(f.content,'UTF8'))
                AND x.source_hash=ortak_reviewed_export_source_hash(f))
            AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop WHERE stop.company_id=x.company_id
                AND stop.fact_id=x.fact_id AND stop.action='withdraw')
        ELSE ortak_reviewed_runtime_eligible(company,fact,t.id,t.consumption_epoch) END,
        'publication',jsonb_build_object('state',p.state,'retry_version',p.retry_version,'attempt_count',p.attempt_count,
            'next_attempt_at',p.next_attempt_at,'error_code',p.last_error_code),
        'cleanup',jsonb_build_object('state',w.state,'retry_version',w.retry_version,'attempt_count',w.attempt_count,
            'next_attempt_at',w.next_attempt_at,'error_code',w.last_error_code),
        'erased_from_reviewed_store',coalesce(r.erased_from_reviewed_store,false))
    FROM reviewed_memory_exports x
    JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
    JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
    JOIN reviewed_memory_export_jobs p ON p.company_id=x.company_id AND p.fact_id=x.fact_id AND p.action='publish'
    JOIN reviewed_memory_export_jobs w ON w.company_id=x.company_id AND w.fact_id=x.fact_id AND w.action='withdraw'
    LEFT JOIN reviewed_memory_export_receipts r ON r.company_id=x.company_id AND r.fact_id=x.fact_id AND r.action='withdraw'
    WHERE x.company_id=company AND x.fact_id=fact
$$;

CREATE OR REPLACE FUNCTION ortak_conversation_snapshot76(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    r runs; revision employee_revisions; work work_executions;
    selected_project UUID; origin RECORD; context JSONB; record JSONB; pin JSONB;
    wrapped JSONB; rendered JSONB; expected_pin JSONB; expected_record JSONB;
    u run_reviewed_memory_uses; f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
    used_count INTEGER; scratch_count INTEGER; i INTEGER=0; conversations INTEGER=0;
    reviewed_bytes INTEGER=0; total_bytes INTEGER=0; content TEXT; seen UUID[]=ARRAY[]::uuid[];
BEGIN
    SELECT * INTO r FROM runs x WHERE x.company_id=company AND x.id=run;
    SELECT * INTO revision FROM employee_revisions x WHERE x.company_id=company
        AND x.employee_id=r.employee_id AND x.id=r.employee_revision_id;
    context=wire->'conversation';
    IF r.id IS NULL OR revision.id IS NULL OR r.status NOT IN('queued','running','waiting')
        OR wire->'version' IS DISTINCT FROM '4'::jsonb
        OR wire ? 'reviewed' OR jsonb_typeof(context) IS DISTINCT FROM 'object'
        OR (context-'origin'-'records'-'truncated')<>'{}'::jsonb
        OR jsonb_typeof(context->'truncated') IS DISTINCT FROM 'boolean'
        OR jsonb_typeof(context->'records') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array'
        OR wire->>'company_id' IS DISTINCT FROM company::text
        OR wire#>>'{spec,run_id}' IS DISTINCT FROM run::text
        OR wire#>>'{spec,employee_id}' IS DISTINCT FROM r.employee_id
        OR wire#>>'{spec,revision_id}' IS DISTINCT FROM r.employee_revision_id::text
        OR wire#>>'{spec,idempotency_key}' IS DISTINCT FROM 'ortak-run:'||company::text||':'||run::text
        OR wire#>'{spec,binding}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'runtime')::json)
        OR wire#>'{spec,permissions}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'permissions')::json)
        OR wire->'memory_binding' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'memory')::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot shape or run identity differs' USING ERRCODE='check_violation';
    END IF;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF used_count NOT BETWEEN 1 AND 8 OR jsonb_array_length(context->'records')<>used_count
        OR scratch_count+used_count>8
        OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: conversation snapshot count differs' USING ERRCODE='check_violation';
    END IF;
    -- Select the project from immutable use/fact rows, never from the caller's
    -- JSON provenance. Every reviewed record below must have this same project.
    SELECT min(fact.project_id::text)::uuid INTO selected_project
        FROM run_reviewed_memory_uses used JOIN reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.project_id)=1;
    SELECT * INTO origin FROM ortak_conversation_run_origin(company,run,selected_project);
    IF NOT FOUND OR context->'origin' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(
        jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot origin differs' USING ERRCODE='check_violation';
    END IF;
    IF r.work_item_id IS NULL THEN
        IF wire ? 'work_origin' OR wire->>'message_id' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire->>'root_message_id' IS DISTINCT FROM encode(r.root_message_id,'hex')
            OR wire->>'routing_decision_id' IS DISTINCT FROM r.routing_decision_id::text
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire#>>'{spec,context,reply_to_message_id}' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire#>'{spec,context,work_item_id}' IS DISTINCT FROM 'null'::jsonb
            OR NOT EXISTS(SELECT 1 FROM office_inbox inbox
                JOIN office_company_bindings office ON office.company_id=inbox.company_id
                JOIN events event ON event.community_id=office.community_id AND event.id=inbox.event_id
                    AND event.created_at=inbox.event_created_at AND event.kind=inbox.event_kind
                    AND event.channel_id=inbox.channel_id AND event.pubkey=inbox.author_pubkey
                CROSS JOIN LATERAL (SELECT regexp_replace(event.content,
                    U&'[\0001-\0008\000B\000C\000E-\001F\007F-\009F]','','g') AS cleaned) input
                WHERE inbox.company_id=company AND inbox.event_id=r.message_id
                AND wire->'event_kind'=to_jsonb(inbox.event_kind)
                AND wire#>>'{spec,context,conversation_ref}'=inbox.channel_id::text
                -- Source75 already caps the original text at65536 bytes;
                -- control removal cannot require UTF-8 truncation afterwards.
                AND event.deleted_at IS NULL AND octet_length(event.content)<=65536
                AND btrim(input.cleaned,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
                AND wire#>'{spec,input}'=ortak_snapshot_scratch_jsonb(to_json(input.cleaned))) THEN
            RAISE EXCEPTION 'ortak: conversation Office origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        SELECT * INTO work FROM work_executions x WHERE x.company_id=company AND x.run_id=run;
        IF work.run_id IS NULL OR work.project_id<>selected_project
            OR wire ? 'message_id' OR wire ? 'root_message_id' OR wire ? 'routing_decision_id'
            OR wire->'event_kind' IS DISTINCT FROM '0'::jsonb
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire->'work_origin' IS DISTINCT FROM jsonb_build_object('run_id',work.run_id,
                'work_item_id',work.work_item_id,'project_id',work.project_id,'execution_version',work.execution_version,
                'definition_hash',encode(work.definition_hash,'hex'))
            OR wire#>'{spec,input}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(convert_from(work.definition_bytes,'UTF8')))
            OR wire#>>'{spec,context,work_item_id}' IS DISTINCT FROM r.work_item_id::text
            OR wire#>'{spec,context,reply_to_message_id}' IS DISTINCT FROM 'null'::jsonb
            OR wire#>'{spec,context,conversation_ref}' IS DISTINCT FROM 'null'::jsonb THEN
            RAISE EXCEPTION 'ortak: conversation Work origin differs' USING ERRCODE='check_violation';
        END IF;
    END IF;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',i::text])>8192
            OR jsonb_typeof(record->'content') IS DISTINCT FROM 'string' THEN
            RAISE EXCEPTION 'ortak: conversation scratch rendering differs' USING ERRCODE='check_violation';
        END IF;
        content=record->>'content';
        total_bytes=total_bytes+octet_length(content)
            -(octet_length(content)-octet_length(regexp_replace(content,E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR wrapped IN SELECT value FROM jsonb_array_elements(context->'records') LOOP
        record=wrapped->'record'; pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR f.project_id<>selected_project OR u.fact_id=ANY(seen)
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets target WHERE target.company_id=company
                AND target.id=u.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
            RAISE EXCEPTION 'ortak: conversation retained record identity differs' USING ERRCODE='check_violation';
        END IF;
        seen=array_append(seen,u.fact_id);
        expected_pin=jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
            'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
            'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
            'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at');
        IF wrapped->>'scope'='conversation' AND f.audience_kind='conversation' THEN
            SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=f.id;
            IF NOT FOUND OR u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM a.audience_hash THEN
                RAISE EXCEPTION 'ortak: conversation audience pin differs' USING ERRCODE='check_violation';
            END IF;
            expected_pin=expected_pin||jsonb_build_object('conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
                'conversation_authority_epoch',u.conversation_authority_epoch,
                'conversation_consumption_epoch',u.conversation_consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content,'provenance',convert_from(a.provenance_bytes,'UTF8'));
            conversations=conversations+1;
        ELSIF wrapped->>'scope'='project' AND f.audience_kind='project' AND r.work_item_id IS NOT NULL THEN
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content);
        ELSE RAISE EXCEPTION 'ortak: conversation record scope differs' USING ERRCODE='check_violation';
        END IF;
        IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
            OR wrapped IS DISTINCT FROM jsonb_build_object('scope',wrapped->>'scope','record',record)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: conversation record bytes differ from retained use' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type',CASE WHEN wrapped->>'scope'='project'
                THEN 'reviewed_project_memory' ELSE 'reviewed_conversation_memory' END,'trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])>8192 THEN
            RAISE EXCEPTION 'ortak: conversation rendered bytes differ' USING ERRCODE='check_violation';
        END IF;
        reviewed_bytes=reviewed_bytes+octet_length(f.content); i=i+1;
    END LOOP;
    IF conversations=0 OR reviewed_bytes>8192 OR total_bytes+reviewed_bytes>16384
        OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: conversation budget or current authority differs' USING ERRCODE='check_violation';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    -- Even PostgreSQL json field access may unescape unrelated NUL values.
    -- Encode the whole comparison document before performing any field access.
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
    IF wire->'version'='5'::jsonb THEN
        PERFORM ortak_employee_snapshot_v5(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'employee' OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses employee_use
        WHERE employee_use.company_id=company AND employee_use.run_id=run) THEN
        RAISE EXCEPTION 'legacy snapshot cannot carry employee context' USING ERRCODE='check_violation';
    END IF;
    IF wire->'version'='4'::jsonb THEN
        PERFORM ortak_conversation_snapshot76(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'conversation' THEN
        RAISE EXCEPTION 'ortak: legacy snapshot cannot carry conversation context' USING ERRCODE='check_violation';
    END IF;
    IF wire->'version' IS DISTINCT FROM '3'::jsonb THEN
        IF used_count<>0 OR wire ? 'reviewed' THEN RAISE EXCEPTION 'ortak: legacy snapshot cannot contain reviewed context' USING ERRCODE='check_violation'; END IF;
        RETURN NEW;
    END IF;
    IF jsonb_typeof(wire#>'{reviewed,records}') IS DISTINCT FROM 'array'
        OR jsonb_array_length(wire#>'{reviewed,records}')<>used_count OR used_count>8
        OR NOT EXISTS(SELECT 1 FROM work_executions wx JOIN runs r ON r.company_id=wx.company_id AND r.id=wx.run_id
            WHERE wx.company_id=company AND wx.run_id=run AND wire#>>'{work_origin,project_id}'=wx.project_id::text
              AND wire#>>'{spec,employee_id}'=r.employee_id) THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot scope or count differs' USING ERRCODE='check_violation';
    END IF;
    IF jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array' THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot context arrays missing' USING ERRCODE='check_violation';
    END IF;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF scratch_count+used_count>8 OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot total record budget differs' USING ERRCODE='check_violation';
    END IF;
    -- Outer records are already encoded once. Serialized memory_context strings
    -- still contain original inner JSON escapes and need their own one encoding.
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: scratch rendered context differs' USING ERRCODE='check_violation';
        END IF;
        -- Each encoded SOH pair represents exactly one original UTF-8 byte.
        -- Count bytes from the original content, not the comparison encoding.
        total_bytes=total_bytes+octet_length(record->>'content')
            -(octet_length(record->>'content')-octet_length(regexp_replace(record->>'content',E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{reviewed,records}') LOOP
        pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR record->'content' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(f.content))
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=company AND t.id=u.target_id AND ortak_snapshot_scratch_jsonb(t.binding::json)=wire->'memory_binding')
            OR pin IS DISTINCT FROM ortak_snapshot_scratch_jsonb(jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,
                'fact_version',u.fact_version,'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
                'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
                'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at')::json)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: reviewed snapshot bytes differ from retained uses' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','reviewed_project_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: reviewed rendered context differs' USING ERRCODE='check_violation';
        END IF;
        total_bytes=total_bytes+octet_length(f.content);
        i=i+1;
    END LOOP;
    IF total_bytes>16384 OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: reviewed context authority expired before commit' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=NEW.target_id
        WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id AND f.employee_id=NEW.employee_id
        AND f.community_id=NEW.community_id AND NEW.content_hash=sha256(convert_to(f.content,'UTF8'))
        AND NEW.source_hash=ortak_reviewed_export_source_hash(f) AND t.employee_revision_id=NEW.employee_revision_id
        AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch AND (CASE WHEN f.audience_kind='conversation' THEN ortak_conversation_export_eligible(f.company_id,f.id,t.id) ELSE ortak_reviewed_export_eligible(f.company_id,f.id,t.id) END))
      OR NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
        AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id AND o.action='publish' AND o.result_version=0
        AND o.xmin::text::bigint=txid_current()%4294967296)
      OR (SELECT count(*) FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
        AND j.state='pending' AND j.attempt_count=0 AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'ortak: reviewed export requires current fact, atomic instruction and two jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;


DROP TRIGGER IF EXISTS conversation_work_output_at_commit ON runtime_work_outputs;
CREATE CONSTRAINT TRIGGER conversation_work_output_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
DROP TRIGGER IF EXISTS conversation_office_output_at_commit ON runtime_office_outputs;
CREATE CONSTRAINT TRIGGER conversation_office_output_at_commit AFTER INSERT OR UPDATE ON runtime_office_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
DROP TRIGGER IF EXISTS conversation_memory_write_at_commit ON runtime_memory_writes;
CREATE CONSTRAINT TRIGGER conversation_memory_write_at_commit AFTER INSERT OR UPDATE ON runtime_memory_writes
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
DROP TRIGGER IF EXISTS conversation_delivery_at_commit ON outbox;
CREATE CONSTRAINT TRIGGER conversation_delivery_at_commit AFTER INSERT OR UPDATE ON outbox
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();


-- Reviewed77 exact function convergence.
-- Preserve existing OIDs and every original function body byte.

CREATE OR REPLACE FUNCTION ortak_employee_memory_timestamp(value TIMESTAMPTZ)
RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT CASE WHEN value >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND value < TIMESTAMPTZ '10000-01-01 00:00:00+00'
        THEN to_char(value AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_observation(
    company UUID, employee TEXT, actor BYTEA, source_id BYTEA,
    source_created_at TIMESTAMPTZ, destination_channel UUID,
    memory_kind TEXT, relationship_human BYTEA
) RETURNS TABLE(community_id UUID, source_channel_id UUID,
    source_author_public_key BYTEA, source_evidence_hash BYTEA,
    employee_revision_id UUID, employee_lifecycle_epoch BIGINT,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE node RECORD; first_node RECORD; count_nodes INTEGER:=0;
    seen BYTEA[]:=ARRAY[]::bytea[]; expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ; expected_depth INTEGER;
    expected_root BYTEA; expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA; resolved_root_at TIMESTAMPTZ;
    tag JSONB; part JSONB; marker TEXT; reference_id BYTEA;
    claimed_root BYTEA; claimed_parent BYTEA; effective_depth INTEGER;
    evidence BYTEA;
BEGIN
    IF company IS NULL OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR employee IS NULL OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
        OR octet_length(employee) NOT BETWEEN 1 AND 64
        OR actor IS NULL OR octet_length(actor)<>32
        OR source_id IS NULL OR octet_length(source_id)<>32
        OR public.ortak_employee_memory_timestamp(source_created_at) IS NULL
        OR destination_channel IS NULL
        OR destination_channel='00000000-0000-0000-0000-000000000000'::uuid
        OR memory_kind IS NULL OR memory_kind NOT IN('experience','relationship')
        OR (memory_kind='experience' AND relationship_human IS NOT NULL)
        OR (memory_kind='relationship' AND relationship_human IS DISTINCT FROM actor) THEN RETURN; END IF;

    FOR node IN
      WITH RECURSIVE selection AS MATERIALIZED (
        SELECT ob.community_id,i.channel_id,i.event_created_at,i.event_kind,i.author_pubkey,
            e.active_revision_id,e.lifecycle_epoch,b.public_key AS employee_key,
            b.valid_until AS identity_valid_before,statement_timestamp() AS observed_at
        FROM public.companies co
        JOIN public.office_company_bindings ob ON ob.company_id=co.id
        JOIN public.communities cm ON cm.id=ob.community_id
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        JOIN public.employees e ON e.company_id=co.id AND e.id=$2 AND e.status='active'
        JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
            AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}'
            AND b.signer_ref=r.manifest#>>'{office,signer_ref}' AND b.verified_at IS NOT NULL
            AND b.valid_from<=statement_timestamp()
            AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.office_inbox i ON i.company_id=co.id AND i.event_id=$4
            AND i.event_created_at=$5 AND i.state='decided' AND i.author_pubkey=$3 AND i.event_kind IN(9,40002)
        WHERE co.id=$1 AND co.status='active' AND e.lifecycle_epoch>=0 AND b.public_key<>$3
            AND octet_length(b.public_key)=32
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$3
                AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
            AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$3)
            AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$3 AND bot.role='bot')
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key
                AND u.deactivated_at IS NOT NULL)
      ), accepted_channels AS MATERIALIZED (
        SELECT ch.id,ch.ttl_deadline
        FROM selection s JOIN public.channels ch ON ch.community_id=s.community_id AND ch.id IN(s.channel_id,$6)
        JOIN public.channel_members human_member ON human_member.community_id=ch.community_id
            AND human_member.channel_id=ch.id AND human_member.pubkey=$3
            AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.channel_members employee_member ON employee_member.community_id=ch.community_id
            AND employee_member.channel_id=ch.id AND employee_member.pubkey=s.employee_key AND employee_member.removed_at IS NULL
        WHERE ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
            AND (ch.channel_type='stream' OR (
                ch.channel_type='dm' AND ch.visibility='private'
                -- Same binary sorted retained-pair recipe as direct_channel_on.
                -- Both exact keys already have current rows above; counting ALL
                -- retained rows (including removed) refuses a third/replaced key.
                AND ch.participant_hash=sha256(CASE WHEN $3<s.employee_key
                    THEN $3||s.employee_key ELSE s.employee_key||$3 END)
                AND (SELECT count(*) FROM (SELECT m.pubkey FROM public.channel_members m
                    WHERE m.community_id=ch.community_id AND m.channel_id=ch.id ORDER BY m.pubkey LIMIT 3) retained)=2))
      ), visible AS MATERIALIZED (
        SELECT s.*,least(src.ttl_deadline,dst.ttl_deadline,s.identity_valid_before) AS valid_before
        FROM selection s JOIN accepted_channels src ON src.id=s.channel_id
        JOIN accepted_channels dst ON dst.id=$6
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.events e ON e.community_id=v.community_id
            AND e.id=$4 AND e.created_at=v.event_created_at
            AND e.channel_id=v.channel_id AND e.kind=v.event_kind AND e.pubkey=v.author_pubkey
        WHERE e.deleted_at IS NULL AND e.kind IN(9,40002) AND e.pubkey=$3
            AND octet_length(e.content)<=65536 AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
      ), ancestry AS (
        SELECT 0 AS hop,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END AS tags,
            t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
            t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
        FROM source s JOIN public.events e ON e.community_id=s.community_id AND e.id=s.id AND e.created_at=s.created_at
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        UNION ALL
        SELECT a.hop+1,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END,
            t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
            t.root_event_id,t.root_event_created_at,t.depth
        FROM ancestry a JOIN public.events e ON e.community_id=(SELECT s.community_id FROM source s)
            AND e.id=a.parent_event_id AND e.created_at=a.parent_event_created_at
            AND e.channel_id=(SELECT s.channel_id FROM source s) AND e.deleted_at IS NULL AND e.kind IN(9,40002)
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        WHERE a.hop<32
      )
      SELECT a.*,s.community_id,s.channel_id,s.active_revision_id,s.lifecycle_epoch,s.observed_at,s.valid_before,
        CASE WHEN a.hop=0 THEN s.content END AS source_content,
        CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
        CASE WHEN a.hop=0 THEN s.sig END AS source_signature,s.kind AS source_kind
      FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
    LOOP
        IF node.hop <> count_nodes OR octet_length(node.id) <> 32
           OR node.id = ANY(seen)
           OR NOT isfinite(node.created_at)
           OR node.created_at < '1970-01-01 00:00:00+00'::timestamptz
           OR node.created_at >= '10000-01-01 00:00:00+00'::timestamptz
           OR node.tags IS NULL OR jsonb_typeof(node.tags) <> 'array' THEN RETURN; END IF;
        seen := array_append(seen,node.id);
        IF count_nodes=0 THEN
            first_node := node;
            IF node.community_id = '00000000-0000-0000-0000-000000000000'::uuid
               OR node.channel_id = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN; END IF;
        ELSE
            IF expected_parent IS DISTINCT FROM node.id
               OR expected_parent_at IS DISTINCT FROM node.created_at THEN RETURN; END IF;
        END IF;

        -- Vec<Vec<String>> parity: even non-e tags must be arrays of strings.
        claimed_root := NULL; claimed_parent := NULL;
        FOR tag IN SELECT t.value FROM jsonb_array_elements(node.tags) AS t(value) LOOP
            IF jsonb_typeof(tag) <> 'array' THEN RETURN; END IF;
            FOR part IN SELECT t.value FROM jsonb_array_elements(tag) AS t(value) LOOP
                IF jsonb_typeof(part) <> 'string' THEN RETURN; END IF;
            END LOOP;
            IF tag->>0 IS DISTINCT FROM 'e' THEN CONTINUE; END IF;
            IF jsonb_array_length(tag)<4 OR octet_length(tag->>1)<>64
               OR (tag->>1) COLLATE "C" !~ '^[0-9a-fA-F]{64}$' THEN RETURN; END IF;
            reference_id := decode(tag->>1,'hex');
            marker := tag->>3;
            CASE marker
            WHEN 'root' THEN
                IF claimed_root IS NOT NULL THEN RETURN; END IF;
                claimed_root := reference_id;
            WHEN 'reply' THEN
                IF claimed_parent IS NOT NULL THEN RETURN; END IF;
                claimed_parent := reference_id;
            WHEN 'mention' THEN CONTINUE;
            ELSE RETURN;
            END CASE;
        END LOOP;
        IF claimed_root IS NOT NULL AND claimed_parent IS NULL THEN RETURN; END IF;
        claimed_root := coalesce(claimed_root,claimed_parent);

        -- Both locator halves are required, including exact UTC partition time.
        IF (node.parent_event_id IS NULL) <> (node.parent_event_created_at IS NULL)
           OR (node.root_event_id IS NULL) <> (node.root_event_created_at IS NULL) THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL AND (octet_length(node.parent_event_id)<>32
           OR NOT isfinite(node.parent_event_created_at)
           OR node.parent_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.parent_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;
        IF node.root_event_id IS NOT NULL AND (octet_length(node.root_event_id)<>32
           OR NOT isfinite(node.root_event_created_at)
           OR node.root_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.root_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;

        effective_depth := coalesce(node.depth,0);
        IF node.metadata_present THEN
            IF node.metadata_channel IS DISTINCT FROM first_node.channel_id THEN RETURN; END IF;
            IF node.parent_event_id IS NULL AND node.depth=0 AND claimed_parent IS NULL THEN
                IF node.root_event_id IS NOT NULL AND
                   (node.root_event_id IS DISTINCT FROM node.id OR node.root_event_created_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            ELSIF node.parent_event_id IS NOT NULL AND node.root_event_id IS NOT NULL
                  AND node.depth BETWEEN 1 AND 32
                  AND claimed_parent=node.parent_event_id AND claimed_root=node.root_event_id THEN
                NULL;
            ELSE RETURN;
            END IF;
        ELSIF node.parent_event_id IS NOT NULL OR node.root_event_id IS NOT NULL
              OR node.depth IS NOT NULL OR claimed_parent IS NOT NULL THEN RETURN;
        END IF;
        IF count_nodes>0 AND expected_depth IS DISTINCT FROM effective_depth THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL THEN
            IF count_nodes=0 THEN
                expected_root := node.root_event_id;
                expected_root_at := node.root_event_created_at;
            ELSIF node.root_event_id IS DISTINCT FROM expected_root
                  OR node.root_event_created_at IS DISTINCT FROM expected_root_at THEN RETURN;
            END IF;
        ELSE
            IF expected_root IS NOT NULL AND (expected_root IS DISTINCT FROM node.id
               OR expected_root_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            resolved_root := node.id; resolved_root_at := node.created_at;
        END IF;
        expected_parent := node.parent_event_id;
        expected_parent_at := node.parent_event_created_at;
        expected_depth := effective_depth-1;
        count_nodes := count_nodes+1;
    END LOOP;
    -- A missing/deleted/cross-channel parent, cycle or 33rd edge cannot become
    -- a top-level fallback. Every nonterminal depth decreases to an actual root.
    IF count_nodes=0 OR expected_parent IS NOT NULL OR resolved_root IS NULL THEN RETURN; END IF;

    -- Exact original source locator, never the resolved ancestry root. The root
    -- above establishes consistency; it is not an employee audience field.
    evidence=public.ortak_employee_memory_evidence_bytes($1,first_node.community_id,
        first_node.channel_id,first_node.id,first_node.created_at,first_node.source_author,
        first_node.source_kind,first_node.source_signature,first_node.tags,first_node.source_content);
    IF evidence IS NULL OR first_node.created_at IS DISTINCT FROM $5
        OR first_node.source_author IS DISTINCT FROM $3 THEN RETURN; END IF;
    community_id=first_node.community_id;
    source_channel_id=first_node.channel_id;
    source_author_public_key=first_node.source_author;
    source_evidence_hash=sha256(evidence);
    employee_revision_id=first_node.active_revision_id;
    employee_lifecycle_epoch=first_node.lifecycle_epoch;
    observed_at=first_node.observed_at;
    valid_before=first_node.valid_before;
    -- Statement time pins one read snapshot; wall time can pass its deadline
    -- during a bounded ancestry walk. The final caller still checks at commit.
    IF valid_before IS NOT NULL AND valid_before<=clock_timestamp() THEN RETURN; END IF;
    RETURN NEXT;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_command_current(
    company UUID, employee TEXT, actor BYTEA, action TEXT
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT coalesce($1 IS NOT NULL AND $2 IS NOT NULL AND octet_length($3)=32
        AND $4 IN('approve','stop','publish','retry_publish','retry_withdraw')
        AND EXISTS(
            SELECT 1 FROM public.companies co
            JOIN public.office_company_bindings b ON b.company_id=co.id
            JOIN public.communities cm ON cm.id=b.community_id
            JOIN public.employees e ON e.company_id=co.id AND e.id=$2
            WHERE co.id=$1 AND co.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL
                AND ($4 IN('stop','retry_withdraw') OR (e.status='active' AND e.active_revision_id IS NOT NULL))
                AND (EXISTS(SELECT 1 FROM public.relay_members rm WHERE rm.community_id=cm.id AND rm.pubkey=encode($3,'hex'))
                    OR EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=cm.id AND m.pubkey=$3 AND m.removed_at IS NULL))
                AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$3
                    AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
                AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$3)
                AND NOT EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=cm.id AND m.pubkey=$3 AND m.role='bot')
        ),false)
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_target_authorized(
    company UUID, employee TEXT, deployment UUID, namespace_bytes BYTEA,
    binding JSONB, creation_receipt JSONB, revision UUID, lifecycle BIGINT,
    destination UUID, valid_until TIMESTAMPTZ
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT coalesce($3 IS NOT NULL AND $3<>'00000000-0000-0000-0000-000000000000'::uuid
        AND $10>clock_timestamp() AND public.ortak_employee_memory_timestamp($10) IS NOT NULL
        AND $6->>'company_id'=$1::text AND $6->>'employee_id'=$2 AND $6->>'deployment_id'=$3::text
        AND $6->'binding'=$5 AND $6->>'protocol'='reviewed-employee/1'
        AND $6->>'namespace_hash'=encode(sha256($4),'hex')
        AND $6->>'request_hash' ~ '^[0-9a-f]{64}$' AND jsonb_typeof($6->'native_ids')='object'
        AND EXISTS(SELECT 1 FROM public.companies co
            JOIN public.office_company_bindings ob ON ob.company_id=co.id
            JOIN public.communities cm ON cm.id=ob.community_id
            JOIN public.employees e ON e.company_id=co.id AND e.id=$2
            JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
            JOIN public.employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=r.id
            JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
                AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}' AND b.signer_ref=r.manifest#>>'{office,signer_ref}'
            JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=$9
            JOIN public.channel_members member ON member.community_id=cm.id AND member.channel_id=ch.id
                AND member.pubkey=b.public_key AND member.removed_at IS NULL
            WHERE co.id=$1 AND co.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL
                AND e.status='active' AND r.id=$7 AND e.lifecycle_epoch=$8 AND mb.validated_at IS NOT NULL
                AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
                AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
                AND $5=r.manifest->'memory' AND $5=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,
                    'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
                AND $5->>'adapter'='honcho' AND $5->'options'='{}'::jsonb
                AND ch.archived_at IS NULL AND ch.deleted_at IS NULL AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
                AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
                AND (ch.channel_type='stream' OR (ch.channel_type='dm' AND ch.visibility='private'
                    AND (SELECT count(*) FROM (SELECT m.pubkey FROM public.channel_members m WHERE m.community_id=cm.id AND m.channel_id=ch.id LIMIT 3) all_members)=2
                    AND EXISTS(SELECT 1 FROM public.channel_members h WHERE h.community_id=cm.id AND h.channel_id=ch.id
                        AND h.pubkey<>b.public_key AND h.removed_at IS NULL AND h.role<>'bot'
                        AND ch.participant_hash=sha256(CASE WHEN h.pubkey<b.public_key THEN h.pubkey||b.public_key ELSE b.public_key||h.pubkey END)
                        AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=h.pubkey)
                        AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=h.pubkey
                            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL)))))
        ),false)
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_authority_guard() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'epoch'-'reason'-'changed_at') IS DISTINCT FROM
            (to_jsonb(OLD)-'epoch'-'reason'-'changed_at') OR OLD.epoch=9223372036854775807
            OR NEW.epoch<>OLD.epoch+1 OR NEW.reason='registered' THEN
            RAISE EXCEPTION 'employee memory authority only advances' USING ERRCODE='check_violation';
        END IF;
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NEW.epoch<>0 OR NEW.reason<>'registered' OR NOT EXISTS(
        SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id
        JOIN employees e ON e.company_id=c.id AND e.id=NEW.employee_id
        JOIN channels ch ON ch.community_id=cm.id AND ch.id=NEW.channel_id
        WHERE c.id=NEW.company_id AND cm.id=NEW.community_id AND c.status='active'
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND e.status='active'
            AND ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())) THEN
        RAISE EXCEPTION 'employee memory scope is not current' USING ERRCODE='check_violation';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||NEW.community_id::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM employee_memory_channel_authorities WHERE company_id=NEW.company_id)>=128
        OR (SELECT count(*) FROM employee_memory_channel_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'retained employee memory scope cap reached' USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp(); NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_register_employee_memory_authorities(
    company UUID, community UUID, employee TEXT, source_channel UUID, destination_channel UUID
) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE channel UUID;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    IF current_setting('transaction_isolation')<>'read committed'
        OR company IS NULL OR community IS NULL OR employee IS NULL
        OR source_channel IS NULL OR destination_channel IS NULL THEN
        RAISE EXCEPTION 'employee memory registration requires current scoped transaction'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||community::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    FOR channel IN SELECT DISTINCT v FROM unnest(ARRAY[source_channel,destination_channel]) v ORDER BY v LOOP
        -- No rebind/reset of retained keys; INSERT guard independently checks caps.
        PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=company
            AND a.community_id=community AND a.employee_id=employee AND a.channel_id=channel FOR SHARE;
        IF NOT FOUND THEN
            INSERT INTO employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id)
                VALUES(company,community,employee,channel);
        END IF;
    END LOOP;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_audience(f employee_reviewed_memory_facts)
RETURNS JSONB LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT jsonb_build_object('company_id',f.company_id,'employee_id',f.employee_id,
        'format','ortak-reviewed-employee-audience/1','kind',f.kind,
        'human_public_key',encode(f.human_public_key,'hex'),
        'destination_community_id',f.community_id,'destination_channel_id',f.destination_channel_id)
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_source(f employee_reviewed_memory_facts)
RETURNS JSONB LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT jsonb_build_object('community_id',f.community_id,'channel_id',f.source_channel_id,
        'event_id',encode(f.source_event_id,'hex'),
        'event_created_at',ortak_employee_memory_timestamp(f.source_event_created_at),
        'author_public_key',encode(f.source_author_public_key,'hex'),
        'evidence_hash',encode(f.source_evidence_hash,'hex'))
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_fact_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE audience JSONB; source JSONB; provenance JSONB;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF OLD.version<>1 OR NEW.version<>2 OR NEW.revoked_at IS NULL OR NEW.revoked_by IS DISTINCT FROM OLD.approved_by
            OR (to_jsonb(NEW)-'version'-'revoked_at'-'revoked_by') IS DISTINCT FROM
                (to_jsonb(OLD)-'version'-'revoked_at'-'revoked_by') THEN
            RAISE EXCEPTION 'employee memory fact only permits Stop' USING ERRCODE='check_violation';
        END IF;
        NEW.revoked_at=clock_timestamp(); RETURN NEW;
    END IF;
    IF NEW.version<>1 OR NEW.revoked_at IS NOT NULL OR NEW.revoked_by IS NOT NULL THEN
        RAISE EXCEPTION 'new employee memory fact must be approved' USING ERRCODE='check_violation';
    END IF;
    NEW.approved_at=clock_timestamp();
    audience=ortak_employee_memory_audience(NEW); source=ortak_employee_memory_source(NEW);
    provenance=jsonb_build_object('format','ortak-reviewed-employee-provenance/1',
        'audience',audience,'audience_hash',encode(NEW.audience_hash,'hex'),
        'source',source,'source_hash',encode(NEW.source_hash,'hex'),
        'approval',jsonb_build_object('format','ortak-reviewed-employee-sharing/1',
            'approval_id',NEW.approval_id,'approved_by',encode(NEW.approved_by,'hex'),
            'content_hash',encode(NEW.content_hash,'hex'),
            'expires_at',ortak_employee_memory_timestamp(NEW.expires_at)));
    IF NEW.audience_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(audience),'UTF8')
        OR NEW.source_hash IS DISTINCT FROM sha256(convert_to(ortak_conversation_json75(
            jsonb_build_object('audience_hash',encode(NEW.audience_hash,'hex'),
                'format','ortak-reviewed-employee-source/1','source',source)),'UTF8'))
        OR NEW.provenance_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(provenance),'UTF8') THEN
        RAISE EXCEPTION 'employee memory canonical bytes differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_submission(
    f employee_reviewed_memory_facts, operation UUID, action TEXT
) RETURNS BYTEA LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT convert_to(ortak_conversation_json75(CASE action
        WHEN 'approve' THEN jsonb_build_object('format','ortak-reviewed-employee-command/1',
            'action',action,'operation_id',operation,'employee_id',f.employee_id,'kind',f.kind,
            'human_public_key',encode(f.human_public_key,'hex'),
            'source_event_id',encode(f.source_event_id,'hex'),
            'source_event_created_at',ortak_employee_memory_timestamp(f.source_event_created_at),
            'destination_channel_id',f.destination_channel_id,
            'expected_audience_hash',encode(f.audience_hash,'hex'),
            'content',f.content,'expires_at',ortak_employee_memory_timestamp(f.expires_at),'reviewed',true)
        WHEN 'stop' THEN jsonb_build_object('format','ortak-reviewed-employee-command/1',
            'action',action,'operation_id',operation,'fact_id',f.id,'expected_version',1)
        END),'UTF8')
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_fact_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts; o employee_reviewed_memory_operations;
    observation RECORD; selected_action TEXT;
BEGIN
    -- INSERT guards validate NEW creation only. Immutable historical receipts
    -- are not rechecked on later Stop, remote cleanup, or read/restore.
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.id;
    PERFORM ortak_lock_office_authority(f.company_id);
    IF TG_OP='INSERT' THEN selected_action='approve'; ELSE selected_action='stop'; END IF;
    SELECT * INTO STRICT o FROM employee_reviewed_memory_operations op
        WHERE op.company_id=f.company_id AND op.fact_id=f.id AND op.action=selected_action
            AND op.xmin::text::bigint=txid_current()%4294967296;
    IF o.actor_public_key<>f.approved_by OR o.community_id<>f.community_id
        OR o.result_version<>f.version
        OR o.submitted_bytes IS DISTINCT FROM ortak_employee_memory_submission(f,o.operation_id,o.action)
        OR o.valid_before<=clock_timestamp()
        OR NOT coalesce(ortak_employee_memory_command_current(f.company_id,f.employee_id,
            o.actor_public_key,o.action),false) THEN
        RAISE EXCEPTION 'employee memory lacks its exact current atomic command' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='UPDATE' THEN RETURN NEW; END IF;
    IF o.operation_id<>f.approval_id THEN
        RAISE EXCEPTION 'employee memory approval identity mismatch' USING ERRCODE='check_violation';
    END IF;
    PERFORM ortak_lock_office_authority(f.company_id);
    PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
        AND a.community_id=f.community_id AND a.employee_id=f.employee_id
        AND a.channel_id IN(f.source_channel_id,f.destination_channel_id)
        ORDER BY a.channel_id FOR SHARE;
    SELECT * INTO STRICT observation FROM ortak_employee_memory_observation(f.company_id,f.employee_id,
        f.approved_by,f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key);
    IF (observation.community_id,observation.source_channel_id,observation.source_author_public_key,
        observation.source_evidence_hash) IS DISTINCT FROM
        (f.community_id,f.source_channel_id,f.source_author_public_key,f.source_evidence_hash)
        OR f.expires_at<=clock_timestamp()
        OR observation.observed_at IS NULL OR observation.observed_at>clock_timestamp()
        OR observation.employee_revision_id IS NULL OR observation.employee_lifecycle_epoch IS NULL
        OR NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=f.company_id AND e.id=f.employee_id
            AND e.status='active' AND e.active_revision_id=observation.employee_revision_id
            AND e.lifecycle_epoch=observation.employee_lifecycle_epoch)
        OR (observation.valid_before IS NOT NULL AND
            (observation.valid_before<=clock_timestamp() OR f.expires_at>observation.valid_before)) THEN
        RAISE EXCEPTION 'employee memory source/sharing authority changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_operation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        WHERE f.company_id=NEW.company_id AND f.community_id=NEW.community_id AND f.id=NEW.fact_id
            AND f.approved_by=NEW.actor_public_key AND f.version=NEW.result_version
            AND (NEW.action='stop' OR f.approval_id=NEW.operation_id)
            AND NEW.submitted_bytes=ortak_employee_memory_submission(f,NEW.operation_id,NEW.action)
            AND f.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'employee memory receipt lacks its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE expected_namespace BYTEA; expected_binding BYTEA; registration JSONB; diagnostic JSONB;
    observed TIMESTAMPTZ; cleanup_hash TEXT; recovery_only BOOLEAN=false;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    expected_namespace=convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-namespace/1','company_id',NEW.company_id,'employee_id',NEW.employee_id)),'UTF8');
    expected_binding=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'binding',NEW.binding,'namespace_hash',encode(NEW.namespace_hash,'hex'),'protocol',NEW.protocol)),'UTF8'));
    IF NEW.namespace_bytes IS DISTINCT FROM expected_namespace OR NEW.binding_hash IS DISTINCT FROM expected_binding THEN
        RAISE EXCEPTION 'employee memory target namespace differs' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        registration=NEW.registration_receipt; diagnostic=registration->'diagnostic';
        IF jsonb_typeof(registration)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(registration))<>3
            OR registration->>'format' IS DISTINCT FROM 'ortak-employee-namespace-registration/1'
            OR jsonb_typeof(diagnostic)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(diagnostic))<>8
            OR diagnostic->>'operation_id' IS NULL OR diagnostic->>'employee_revision_id' IS DISTINCT FROM NEW.employee_revision_id::text
            OR diagnostic->>'employee_lifecycle_epoch' IS DISTINCT FROM NEW.employee_lifecycle_epoch::text
            OR diagnostic->>'erased' IS DISTINCT FROM 'true'
            OR NOT coalesce(diagnostic->>'challenge_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'write_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'withdraw_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR diagnostic->>'tombstone_at' IS NULL OR registration->>'validated_at' IS NULL THEN
            RAISE EXCEPTION 'employee namespace registration metadata invalid' USING ERRCODE='check_violation';
        END IF;
        observed=(registration->>'validated_at')::timestamptz;
        IF (diagnostic->>'operation_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
            OR ortak_employee_memory_timestamp(observed) IS DISTINCT FROM registration->>'validated_at'
            OR ortak_employee_memory_timestamp((diagnostic->>'tombstone_at')::timestamptz) IS NULL
            OR observed>clock_timestamp()+interval '5 seconds' OR observed<=clock_timestamp()-interval '55 seconds'
            OR NEW.valid_until<=clock_timestamp() OR NEW.valid_until>observed+interval '90 days'
            OR NEW.consumption_epoch<>0 OR NEW.runtime_consumption_enabled THEN
            RAISE EXCEPTION 'employee namespace initial witness expired or selection invalid' USING ERRCODE='check_violation';
        END IF;
        cleanup_hash=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
            'format','ortak-reviewed-employee-diagnostic-withdraw/1','operation_id',(diagnostic->>'operation_id')::uuid,
            'namespace_hash',encode(NEW.namespace_hash,'hex'),'binding_hash',encode(NEW.binding_hash,'hex'),
            'employee_revision_id',NEW.employee_revision_id,'employee_lifecycle_epoch',NEW.employee_lifecycle_epoch,
            'challenge_hash',diagnostic->>'challenge_hash')),'UTF8')),'hex');
        IF diagnostic->>'withdraw_request_hash' IS DISTINCT FROM cleanup_hash THEN
            RAISE EXCEPTION 'employee namespace cleanup commitment differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        recovery_only=OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled
            AND (to_jsonb(NEW)-'runtime_consumption_enabled'-'updated_at')=(to_jsonb(OLD)-'runtime_consumption_enabled'-'updated_at');
        -- Includes registration receipt and original selection expiry. A model
        -- refresh cannot create ownership, renew an expired selection or rewrite
        -- the original I/O evidence. Explicit future renewal is a separate API.
        IF (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            OR NEW.consumption_epoch<>OLD.consumption_epoch THEN
            RAISE EXCEPTION 'employee memory target identity is immutable' USING ERRCODE='check_violation';
        END IF;
        IF (NEW.enabled,NEW.runtime_consumption_enabled,NEW.employee_lifecycle_epoch) IS DISTINCT FROM (OLD.enabled,OLD.runtime_consumption_enabled,OLD.employee_lifecycle_epoch) THEN
            IF OLD.consumption_epoch=9223372036854775807 THEN
                RAISE EXCEPTION 'employee memory target epoch exhausted' USING ERRCODE='program_limit_exceeded';
            END IF;
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
    END IF;
    IF NOT recovery_only AND (TG_OP='INSERT' OR NEW.enabled) AND NOT coalesce(ortak_employee_memory_target_authorized(
        NEW.company_id,NEW.employee_id,NEW.deployment_id,NEW.namespace_bytes,NEW.binding,NEW.creation_receipt,
        NEW.employee_revision_id,NEW.employee_lifecycle_epoch,NEW.destination_channel_id,NEW.valid_until),false) THEN
        RAISE EXCEPTION 'employee namespace current binding unavailable' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at=clock_timestamp(); RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        JOIN employee_reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=target
            AND t.community_id=f.community_id AND t.employee_id=f.employee_id
            AND t.destination_channel_id=f.destination_channel_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=r.id
        CROSS JOIN LATERAL ortak_employee_memory_observation(f.company_id,f.employee_id,f.approved_by,
            f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key) o
        WHERE f.company_id=company AND f.id=fact AND f.version=1 AND f.expires_at>clock_timestamp()
            AND e.status='active' AND t.enabled AND t.valid_until>clock_timestamp()
            AND t.employee_revision_id=r.id AND t.employee_lifecycle_epoch=e.lifecycle_epoch
            AND o.employee_revision_id=r.id AND o.employee_lifecycle_epoch=e.lifecycle_epoch
            AND mb.validated_at IS NOT NULL AND t.binding=r.manifest->'memory'
            AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,
                'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
            AND o.community_id=f.community_id AND o.source_channel_id=f.source_channel_id
            AND o.source_author_public_key=f.source_author_public_key AND o.source_evidence_hash=f.source_evidence_hash
            AND o.observed_at IS NOT NULL AND o.observed_at<=clock_timestamp()
            AND (o.valid_before IS NULL OR o.valid_before>clock_timestamp()))
$$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_request_hash(company UUID, fact UUID, action TEXT)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN action IN('publish','withdraw') THEN sha256(convert_to(ortak_conversation_json75(
        jsonb_build_object('format','ortak-reviewed-employee-remote-request/1','action',action,
            'company_id',x.company_id,'employee_id',x.employee_id,'fact_id',x.fact_id,'target_id',x.target_id,
            'namespace_hash',encode(t.namespace_hash,'hex'),'binding_hash',encode(t.binding_hash,'hex'),
            'content_hash',encode(x.content_hash,'hex'),'source_hash',encode(x.source_hash,'hex'),
            'sharing_hash',encode(x.sharing_hash,'hex'))),'UTF8')) END
    FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_targets t
        ON t.company_id=x.company_id AND t.id=x.target_id
    WHERE x.company_id=company AND x.fact_id=fact
$$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.fact_id;
    PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
        AND a.community_id=f.community_id AND a.employee_id=f.employee_id
        AND a.channel_id IN(f.source_channel_id,f.destination_channel_id) ORDER BY a.channel_id FOR SHARE;
    PERFORM 1 FROM employee_reviewed_memory_facts v WHERE v.company_id=f.company_id AND v.id=f.id FOR SHARE;
    PERFORM 1 FROM employee_reviewed_memory_targets t WHERE t.company_id=f.company_id AND t.id=NEW.target_id FOR SHARE;
    IF (NEW.community_id,NEW.employee_id,NEW.destination_channel_id,NEW.content_hash,NEW.source_hash,NEW.sharing_hash)
        IS DISTINCT FROM (f.community_id,f.employee_id,f.destination_channel_id,f.content_hash,f.source_hash,f.sharing_hash)
        OR NEW.requested_by<>encode(f.approved_by,'hex')
        OR NOT ortak_employee_reviewed_export_eligible(NEW.company_id,NEW.fact_id,NEW.target_id)
        OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_targets t WHERE t.company_id=NEW.company_id AND t.id=NEW.target_id
            AND t.employee_revision_id=NEW.employee_revision_id AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch)
        OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id
            AND o.fact_id=NEW.fact_id AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id
            AND o.action='publish' AND o.result_version=0 AND o.xmin::text::bigint=txid_current()%4294967296)
        OR (SELECT count(*) FROM employee_reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id
            AND j.fact_id=NEW.fact_id AND j.state='pending' AND j.attempt_count=0
            AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'employee memory publication requires current fact and atomic command/jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE employee_reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE allowed BOOLEAN:=false;
BEGIN
    IF (NEW.company_id,NEW.community_id,NEW.fact_id,NEW.action,NEW.idempotency_key,NEW.request_hash)
        IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.fact_id,OLD.action,OLD.idempotency_key,OLD.request_hash)
        OR OLD.state='acknowledged' OR NEW.total_attempts<OLD.total_attempts OR NEW.total_attempts>OLD.total_attempts+1
        OR NEW.retry_version<OLD.retry_version OR NEW.retry_version>OLD.retry_version+1 THEN
        RAISE EXCEPTION 'ortak: reviewed job identity and progress are retained' USING ERRCODE='check_violation';
    END IF;
    IF NEW.retry_version=OLD.retry_version+1 THEN
        allowed:=OLD.state='failed' AND OLD.lease_token IS NULL AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=OLD.total_attempts AND NEW.lease_token IS NULL AND NEW.last_error_code IS NULL
            AND NEW.next_attempt_at<=clock_timestamp();
    ELSIF NEW.attempt_count=OLD.attempt_count+1 AND NEW.total_attempts=OLD.total_attempts+1 THEN
        allowed:=OLD.state='pending' AND NEW.state='pending' AND OLD.next_attempt_at<=clock_timestamp()
            AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
            AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
            AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=clock_timestamp()+INTERVAL '60 seconds'
            AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code;
    ELSIF NEW.attempt_count=OLD.attempt_count AND NEW.total_attempts=OLD.total_attempts AND OLD.state='pending' THEN
        IF NEW.state='acknowledged' THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.lease_token=OLD.lease_token AND NEW.lease_expires_at=OLD.lease_expires_at
                AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NULL;
        ELSIF NEW.state='failed' AND NEW.last_error_code='lease_exhausted' THEN
            allowed:=OLD.attempt_count=20 AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
                AND NEW.lease_token IS NULL AND NEW.next_attempt_at=OLD.next_attempt_at;
        ELSIF NEW.state='pending' AND NEW.action='withdraw' AND NEW.next_attempt_at<=OLD.next_attempt_at THEN
            allowed:=(NEW.lease_token,NEW.lease_expires_at,NEW.last_error_code)
                IS NOT DISTINCT FROM (OLD.lease_token,OLD.lease_expires_at,OLD.last_error_code)
                AND EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
            IF NOT coalesce(allowed,false) THEN
                allowed:=OLD.attempt_count=0 AND OLD.lease_token IS NULL
                    AND NEW.lease_token IS NULL AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code
                    AND NEW.next_attempt_at<=clock_timestamp()
                    AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x
                        WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
                        AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
            END IF;
        ELSIF NEW.lease_token IS NULL AND NEW.last_error_code IS NOT NULL THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.next_attempt_at>clock_timestamp() AND NEW.next_attempt_at<=clock_timestamp()+INTERVAL '301 seconds'
                AND (NEW.state='failed' OR NEW.state='pending' AND OLD.attempt_count<20);
        END IF;
    END IF;
    IF NOT coalesce(allowed,false) THEN
        RAISE EXCEPTION 'ortak: reviewed job transition lacks a due claim, live lease, stop or audited retry' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='employee-reviewed:'||NEW.action||':'||NEW.company_id::text||':'||NEW.fact_id::text
            AND NEW.request_hash=ortak_employee_reviewed_request_hash(NEW.company_id,NEW.fact_id,NEW.action)
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.community_id=NEW.community_id AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts; expected BYTEA;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.fact_id;
    expected=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-export-command/1','operation_id',NEW.operation_id,
        'fact_id',NEW.fact_id,'action',NEW.action,
        'expected_version',CASE WHEN NEW.action='publish' THEN 1 ELSE NEW.result_version-1 END)),'UTF8'));
    IF NEW.actor_pubkey<>encode(f.approved_by,'hex') OR NEW.community_id<>f.community_id
        OR NEW.request_hash IS DISTINCT FROM expected OR NEW.valid_before IS NULL
        OR NOT coalesce(ortak_employee_memory_command_current(f.company_id,f.employee_id,
            decode(NEW.actor_pubkey,'hex'),NEW.action),false) THEN
        RAISE EXCEPTION 'employee export command lacks current/recovery data authority' USING ERRCODE='check_violation';
    END IF;
    IF NEW.action='retry_publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x
        WHERE x.company_id=f.company_id AND x.fact_id=f.id
            AND ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id)) THEN
        RAISE EXCEPTION 'employee publication retry is no longer eligible' USING ERRCODE='check_violation';
    END IF;
    IF NEW.valid_before IS NOT NULL AND NEW.valid_before<=clock_timestamp() THEN
        RAISE EXCEPTION 'ortak: reviewed command authority expired' USING ERRCODE='serialization_failure';
    END IF;
    IF (NEW.action='publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
        AND x.operation_id=NEW.operation_id AND x.requested_by=NEW.actor_pubkey AND NEW.result_version=0 AND x.xmin::text::bigint=txid_current()%4294967296))
        OR (NEW.action<>'publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
            AND 'retry_'||j.action=NEW.action AND j.retry_version=NEW.result_version AND j.xmin::text::bigint=txid_current()%4294967296)) THEN
        RAISE EXCEPTION 'ortak: reviewed command requires its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j
        JOIN employee_reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_schedule_cleanup(company UUID, fact UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
DECLARE affected INTEGER;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    UPDATE employee_reviewed_memory_export_jobs j SET next_attempt_at=clock_timestamp(),updated_at=clock_timestamp()
        WHERE j.company_id=company AND j.fact_id=fact AND j.action='withdraw'
            AND j.state='pending' AND j.attempt_count=0 AND j.lease_token IS NULL
            AND j.next_attempt_at>clock_timestamp()
            AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x WHERE x.company_id=j.company_id
                AND x.fact_id=j.fact_id AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
    GET DIAGNOSTICS affected=ROW_COUNT;
    RETURN affected=1;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_epoch_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE prior JSONB; proposed JSONB; kind TEXT:=TG_ARGV[0]; reason TEXT:=TG_ARGV[1];
    changed BOOLEAN:=TG_OP<>'UPDATE'; field TEXT; co UUID[]; cm UUID[];
    channels UUID[]; employee_keys TEXT[]; target UUID; keys JSONB; selected JSONB;
    old_identity JSONB; new_identity JSONB;
BEGIN
    IF TG_OP<>'INSERT' THEN prior=to_jsonb(OLD); END IF;
    IF TG_OP<>'DELETE' THEN proposed=to_jsonb(NEW); END IF;
    -- Only plaintext Office events can be a canonical employee-memory source
    -- or ancestor. Native NIP-RS (30078) replaces its old encrypted read-state
    -- payload by deletion; a NULL channel there is not a company-wide source
    -- revocation. Check BOTH sides so changing into or out of 9/40002 still
    -- retires the old use. The existing Office mutation fence remains intact.
    IF kind='event' AND NOT (
        coalesce((prior->>'kind')::integer IN (9,40002),false)
        OR coalesce((proposed->>'kind')::integer IN (9,40002),false)
    ) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[2:TG_NARGS-1] LOOP
            IF prior->field IS DISTINCT FROM proposed->field THEN changed=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN
            changed=changed OR ((prior->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL));
        END IF;
        IF kind='memory_identity' THEN
            changed=changed OR ((prior->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL));
        END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(prior->>'deletion_state','')<>'active' THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- A new unrelated reply cannot revoke a running memory consumer while
        -- that consumer is delivering it. Restoration of a referenced anchor
        -- is different, and the existing parent/root indexes bound that lookup.
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE f.community_id=(proposed->>'community_id')::uuid
                AND f.source_event_id=(proposed->>'event_id')::bytea
                AND f.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
            AND NOT EXISTS(SELECT 1 FROM thread_metadata t
                WHERE t.community_id=(proposed->>'community_id')::uuid
                    AND (t.event_id,t.event_created_at) IS DISTINCT FROM
                        ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
                    AND ((t.parent_event_id=(proposed->>'event_id')::bytea
                        AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
                        OR (t.root_event_id=(proposed->>'event_id')::bytea
                        AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN
            RETURN NEW;
        END IF;
    END IF;
    IF kind='inbox' AND coalesce(prior->>'state','')<>'decided'
        AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE (f.company_id,f.source_event_id,f.source_event_created_at) IN(
                ((prior->>'company_id')::uuid,(prior->>'event_id')::bytea,(prior->>'event_created_at')::timestamptz),
                ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
        AND coalesce(proposed,prior)->>'agent_type' IS NULL
        AND coalesce(proposed,prior)->>'agent_owner_pubkey' IS NULL
        AND coalesce(proposed,prior)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE'
        AND (prior->'company_id',prior->'id',prior->'status',prior->'lifecycle_epoch')
            IS NOT DISTINCT FROM
            (proposed->'company_id',proposed->'id',proposed->'status',proposed->'lifecycle_epoch') THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_identity
            FROM employee_revisions r WHERE r.company_id=(prior->>'company_id')::uuid
                AND r.employee_id=prior->>'id' AND r.id=(prior->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_identity
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid
                AND r.employee_id=proposed->>'id' AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_identity IS NOT NULL AND old_identity IS NOT DISTINCT FROM new_identity THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN(
            ((prior->>'company_id')::uuid,prior->>'employee_id',(prior->>'revision_id')::uuid),
            ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO co FROM (VALUES
        (prior->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO cm FROM (VALUES
        (prior->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (prior->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employee_keys FROM (VALUES
        (prior->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    IF kind='office_identity' THEN
        -- A new/removed employee key also changes the community-wide human
        -- classification of that key in other employees' approved sources.
        -- Retire bounded company scopes, not only the binding's employee.
        employee_keys=ARRAY[]::text[];
    END IF;
    IF kind='membership' AND (prior->>'role'='bot' OR proposed->>'role'='bot') THEN
        channels=ARRAY[]::uuid[];
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'employee memory authority requires READ COMMITTED' USING ERRCODE='invalid_transaction_state';
    END IF;
    -- Do not rely only on currently visible retained rows: a first registration
    -- may be in flight. These exclusive try-locks conflict with that shared read.
    FOR target IN SELECT unnest(cm) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory community fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR target IN SELECT unnest(co) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    SELECT coalesce(jsonb_agg(to_jsonb(k) ORDER BY company_id,community_id,employee_id,channel_id),'[]'::jsonb)
        INTO keys FROM (
            SELECT a.company_id,a.community_id,a.employee_id,a.channel_id
            FROM employee_memory_channel_authorities a JOIN communities c ON c.id=a.community_id
            WHERE (a.company_id=ANY(co) OR a.community_id=ANY(cm))
                AND (cardinality(channels)=0 OR a.channel_id=ANY(channels))
                AND (cardinality(employee_keys)=0 OR a.employee_id=ANY(employee_keys))
                AND c.deletion_state='active' AND c.deleted_at IS NULL
            ORDER BY a.company_id,a.community_id,a.employee_id,a.channel_id LIMIT 769
        ) k;
    IF jsonb_array_length(keys)>768 THEN
        RAISE EXCEPTION 'employee memory mutation scope cap exceeded' USING ERRCODE='program_limit_exceeded';
    END IF;
    FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'retained employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR selected IN SELECT value FROM jsonb_array_elements(keys) LOOP
        PERFORM 1 FROM employee_memory_channel_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid
            FOR UPDATE NOWAIT;
        UPDATE employee_memory_channel_authorities a SET epoch=epoch+1,reason=TG_ARGV[1]
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END $$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_evidence_bytes(
    company UUID, community UUID, channel UUID, event_id BYTEA,
    event_created_at TIMESTAMPTZ, author BYTEA, event_kind INTEGER,
    signature BYTEA, tags JSONB, content TEXT
) RETURNS BYTEA LANGUAGE plpgsql IMMUTABLE SECURITY INVOKER PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE tag JSONB; part JSONB; encoded TEXT;
BEGIN
    IF company IS NULL OR community IS NULL OR channel IS NULL
        OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR community='00000000-0000-0000-0000-000000000000'::uuid
        OR channel='00000000-0000-0000-0000-000000000000'::uuid
        OR event_id IS NULL OR octet_length(event_id)<>32
        OR public.ortak_employee_memory_timestamp(event_created_at) IS NULL
        OR author IS NULL OR octet_length(author)<>32
        OR event_kind IS NULL OR event_kind NOT IN(9,40002)
        OR signature IS NULL OR octet_length(signature)<>64
        OR tags IS NULL OR jsonb_typeof(tags)<>'array' OR octet_length(tags::text)>16384
        OR content IS NULL OR octet_length(content)>65536 THEN RETURN NULL; END IF;
    FOR tag IN SELECT value FROM jsonb_array_elements(tags) LOOP
        IF jsonb_typeof(tag)<>'array' THEN RETURN NULL; END IF;
        FOR part IN SELECT value FROM jsonb_array_elements(tag) LOOP
            IF jsonb_typeof(part)<>'string' THEN RETURN NULL; END IF;
        END LOOP;
    END LOOP;
    encoded=public.ortak_conversation_json75(jsonb_build_object(
        'author_public_key',encode(author,'hex'),'channel_id',channel,
        'community_id',community,'company_id',company,'content',content,
        'event_created_at',public.ortak_employee_memory_timestamp(event_created_at),
        'event_id',encode(event_id,'hex'),'format','ortak-reviewed-employee-evidence/1',
        'kind',event_kind,'sig',encode(signature,'hex'),'tags',tags));
    IF encoded IS NULL OR octet_length(encoded)>524288 THEN RETURN NULL; END IF;
    RETURN convert_to(encoded,'UTF8');
END $$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_pair_current(s encrypted_dm_selections)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(
  SELECT 1 FROM public.office_company_bindings cb
  JOIN public.companies co ON co.id=cb.company_id AND co.status='active'
  JOIN public.communities cm ON cm.id=cb.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
  JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=s.channel_id
  JOIN public.employees e ON e.company_id=co.id AND e.id=s.employee_id AND e.status='active'
  JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
  JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id AND b.id=s.office_binding_id
  WHERE cb.company_id=s.company_id AND cb.community_id=s.community_id
    AND ch.channel_type='dm' AND ch.visibility='private'
    AND ch.archived_at IS NULL AND ch.deleted_at IS NULL AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
    AND b.public_key=s.employee_public_key AND b.signer_ref=s.decrypt_ref
    AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
    AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
    AND r.manifest#>>'{office,public_key}'=encode(s.employee_public_key,'hex')
    AND r.manifest#>>'{office,signer_ref}'=s.decrypt_ref
    AND ch.participant_hash=public.digest(
        least(s.human_public_key,s.employee_public_key)||greatest(s.human_public_key,s.employee_public_key),'sha256')
    AND (SELECT count(*) FROM (SELECT 1 FROM public.channel_members m
        WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id LIMIT 3) members)=2
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.human_public_key AND m.removed_at IS NULL)
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.employee_public_key AND m.removed_at IS NULL)
    AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings other WHERE other.company_id=s.company_id AND other.public_key=s.human_public_key)
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.human_public_key
        AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
    AND NOT EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.pubkey=s.human_public_key AND m.role='bot')
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.employee_public_key AND u.deactivated_at IS NOT NULL)
 )
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_selection_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION 'Encrypted DM selection is retained' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF (to_jsonb(NEW)-ARRAY['enabled','generation','changed_at','enabled_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['enabled','generation','changed_at','enabled_at'])
     OR NEW.generation<>OLD.generation OR NEW.changed_at<>OLD.changed_at
     OR NEW.enabled_at IS DISTINCT FROM OLD.enabled_at THEN
   RAISE EXCEPTION 'Encrypted DM selection identity is immutable' USING ERRCODE='check_violation';
  END IF;
  IF NEW.enabled=OLD.enabled THEN RETURN OLD; END IF;
 END IF;
 -- Config changes are Office mutations. Try-lock fails rather than upgrading
 -- across another signed reader; no caller holds this fence through crypto.
 PERFORM public.ortak_advance_office_authority(NEW.company_id,'encrypted_dm_selections');
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 IF TG_OP='INSERT' THEN
  IF NEW.generation<>1 OR (SELECT count(*) FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id)>=128 THEN
   RAISE EXCEPTION 'Encrypted DM retained selection bound' USING ERRCODE='check_violation';
  END IF;
  NEW.created_at:=clock_timestamp();
 ELSE NEW.generation:=OLD.generation+1;
 END IF;
 IF (TG_OP='INSERT' OR NEW.enabled) AND NOT public.ortak_encrypted_dm_pair_current(NEW) THEN
  RAISE EXCEPTION 'Encrypted DM selected pair unavailable' USING ERRCODE='check_violation';
 END IF;
 NEW.changed_at:=clock_timestamp();
 IF NEW.enabled THEN NEW.enabled_at:=NEW.changed_at; END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_selection_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_selections;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_selections
  WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id;
 IF current_row.enabled AND NOT public.ortak_encrypted_dm_pair_current(current_row) THEN
  RAISE EXCEPTION 'Encrypted DM selection expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_outer(target UUID, community UUID, source BYTEA, at_time TIMESTAMPTZ, recipient BYTEA)
RETURNS BYTEA LANGUAGE plpgsql VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE ev RECORD; canonical TEXT;
BEGIN
 SELECT e.id,e.pubkey,e.created_at,e.kind,e.tags,e.content,e.sig INTO ev
 FROM public.office_inbox i JOIN public.events e
  ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
 WHERE i.company_id=target AND i.event_id=source AND i.event_created_at=at_time
  AND i.event_kind=1059 AND e.kind=1059 AND e.channel_id IS NULL AND i.channel_id IS NULL
  AND i.author_pubkey=e.pubkey AND e.deleted_at IS NULL
  AND i.state='pending' AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
  AND e.created_at>=timestamptz '1970-01-01 00:00:00+00' AND e.created_at<timestamptz '10000-01-01 00:00:00+00'
  AND date_trunc('second',e.created_at)=e.created_at
  AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
  AND octet_length(e.content) BETWEEN 132 AND 60000 AND e.content~'^[A-Za-z0-9+/]*={0,2}$'
  AND octet_length(e.tags::text)<=256 AND e.tags=jsonb_build_array(jsonb_build_array('p',encode(recipient,'hex')));
 IF NOT FOUND THEN RETURN NULL; END IF;
 canonical:=public.ortak_conversation_json75(jsonb_build_object(
  'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
  'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex')));
 IF canonical IS NULL OR octet_length(canonical)>65536 THEN RETURN NULL; END IF;
 RETURN convert_to(canonical,'UTF8');
END
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=company AND source_id=source)
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_current(j encrypted_dm_decrypt_jobs)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.encrypted_dm_selections s
  JOIN public.employees e ON e.company_id=s.company_id AND e.id=s.employee_id
  JOIN public.office_routing_cohorts c ON c.company_id=s.company_id AND c.community_id=s.community_id AND c.state='enabled'
  JOIN public.office_routing_channels ch ON ch.company_id=c.company_id AND ch.community_id=c.community_id AND ch.channel_id=s.channel_id
  JOIN public.office_routing_employees ce ON ce.company_id=c.company_id AND ce.employee_id=e.id
  JOIN public.office_inbox i ON i.company_id=j.company_id AND i.event_id=j.source_id
  WHERE s.company_id=j.company_id AND s.selection_id=j.selection_id AND s.community_id=j.community_id
   AND s.enabled AND s.generation=j.selection_generation AND s.employee_id=j.employee_id
   AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
   AND i.received_at=j.source_received_at AND i.received_at>=s.enabled_at AND i.author_pubkey=j.source_author
   AND clock_timestamp()<j.valid_before
   AND coalesce((SELECT generation FROM public.office_authority_generations g WHERE g.company_id=j.company_id),0)=j.office_generation
   AND public.ortak_encrypted_dm_pair_current(s)
   AND public.digest(public.ortak_encrypted_dm_outer(j.company_id,j.community_id,j.source_id,j.source_created_at,s.employee_public_key),'sha256')=j.source_hash)
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Encrypted DM job is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.verified_at IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Encrypted DM job initial state' USING ERRCODE='check_violation';
  END IF;
  fresh:=true;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) THEN
   RAISE EXCEPTION 'Encrypted DM job source is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.state IN('failed','cancelled') THEN
   IF NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION 'Encrypted DM terminal job retained' USING ERRCODE='check_violation'; END IF;
   RETURN OLD;
  END IF;
  IF OLD.verified_at IS NOT NULL AND
   (NEW.seal_id,NEW.seal_created_at,NEW.rumor_id,NEW.rumor_created_at,NEW.rumor_hash,NEW.reply_to,NEW.verified_at) IS DISTINCT FROM
   (OLD.seal_id,OLD.seal_created_at,OLD.rumor_id,OLD.rumor_created_at,OLD.rumor_hash,OLD.reply_to,OLD.verified_at) THEN
   RAISE EXCEPTION 'Encrypted DM verified metadata is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.verified_at IS NULL AND NEW.verified_at IS NOT NULL
    AND NOT(OLD.state='claimed' AND NEW.state='verified') THEN
   RAISE EXCEPTION 'Encrypted DM metadata requires current verification' USING ERRCODE='check_violation';
  END IF;
  -- Identical in-budget receipt replay has no new effect and cannot renew a
  -- token or deadline. Deferred current checks still apply to the result row.
  IF OLD.state='verified' AND NEW IS NOT DISTINCT FROM OLD THEN RETURN OLD; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.claim_generation=OLD.claim_generation+1 AND NEW.state='claimed'
   AND (OLD.state='pending' OR OLD.claim_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp()) AND OLD.next_attempt_at<=clock_timestamp()
   AND NEW.claim_token IS NOT NULL AND NEW.claim_token IS DISTINCT FROM OLD.claim_token THEN fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.claim_generation=OLD.claim_generation THEN
   IF NEW.state='verified' AND OLD.state='claimed' AND OLD.crypto_deadline>clock_timestamp()
    AND (OLD.verified_at IS NOT NULL OR NEW.verified_at>=OLD.claimed_at) AND NEW.verified_at<=clock_timestamp()
    AND (NEW.claim_token,NEW.worker_id,NEW.claimed_at,NEW.claim_expires_at,NEW.crypto_deadline) IS NOT DISTINCT FROM
        (OLD.claim_token,OLD.worker_id,OLD.claimed_at,OLD.claim_expires_at,OLD.crypto_deadline) THEN fresh:=true;
   ELSIF NEW.state IN('failed','cancelled') AND NEW.error_code IS NOT NULL THEN NULL;
   ELSIF NEW.state='pending' AND OLD.state IN('claimed','verified') AND OLD.claim_expires_at>clock_timestamp()
    AND NEW.error_code='material_unavailable' AND OLD.attempts<3
    AND NEW.next_attempt_at>=statement_timestamp()+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END) THEN NULL;
   ELSE RAISE EXCEPTION 'Encrypted DM job transition refused' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Encrypted DM claim generation refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF fresh THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
  -- Inbox claim-state changes deliberately do not advance Office generation.
  -- Retain its row lock through commit as well as comparing canonical facts.
  PERFORM 1 FROM public.office_inbox WHERE company_id=NEW.company_id AND event_id=NEW.source_id FOR SHARE;
  IF NEW.state='claimed' THEN
   IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-claims:'||NEW.company_id::text,0))
     OR NEW.claimed_at>clock_timestamp() OR NEW.crypto_deadline<=clock_timestamp()
     OR (SELECT count(*) FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=NEW.company_id
          AND j.source_id<>NEW.source_id AND j.state IN('claimed','verified') AND NOT public.ortak_encrypted_dm_job_consumed(j.company_id,j.source_id) AND j.claim_expires_at>clock_timestamp())>=2 THEN
    RAISE EXCEPTION 'Encrypted DM finite claim slot unavailable' USING ERRCODE='serialization_failure';
   END IF;
  END IF;
  IF NOT public.ortak_encrypted_dm_job_current(NEW) THEN
   RAISE EXCEPTION 'Encrypted DM job authority changed' USING ERRCODE='serialization_failure';
  END IF;
  IF NEW.state='verified' AND NEW.reply_to IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM public.encrypted_dm_decrypt_jobs previous
    JOIN public.encrypted_dm_selections p ON p.company_id=previous.company_id AND p.selection_id=previous.selection_id
    JOIN public.encrypted_dm_selections s ON s.company_id=NEW.company_id AND s.selection_id=NEW.selection_id
    WHERE previous.company_id=NEW.company_id AND previous.employee_id=NEW.employee_id
      AND previous.rumor_id=NEW.reply_to AND previous.verified_at IS NOT NULL AND previous.source_id<>NEW.source_id
      AND (p.community_id,p.channel_id,p.human_public_key,p.employee_public_key)=(s.community_id,s.channel_id,s.human_public_key,s.employee_public_key)) THEN
   RAISE EXCEPTION 'Encrypted DM reply lacks same-pair verified provenance' USING ERRCODE='check_violation';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_decrypt_jobs;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 IF current_row.state IN('pending','claimed','verified') AND
  (TG_OP='INSERT' OR NEW.state='verified' OR NEW.attempts>OLD.attempts) THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  IF NOT public.ortak_encrypted_dm_job_current(current_row)
   OR (current_row.state IN('claimed','verified') AND clock_timestamp()>=current_row.crypto_deadline) THEN
   RAISE EXCEPTION 'Encrypted DM job expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_runtime_binding(company UUID,revision UUID)
RETURNS JSONB LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 FROM public.employee_runtime_bindings b JOIN public.employee_revisions r
 ON r.company_id=b.company_id AND r.id=b.revision_id AND r.employee_id=b.employee_id
 WHERE b.company_id=company AND b.revision_id=revision AND b.validated_at IS NOT NULL
 AND r.manifest->'runtime'=jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 AND r.manifest->'permissions'='{"allowed_tools":[],"allowed_workspaces":[],"allowed_networks":[],"approval_required":[]}'::jsonb
 AND r.manifest#>'{routing,enabled}'='true'::jsonb
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dm_run_id(company UUID,source BYTEA)
RETURNS UUID LANGUAGE SQL IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT substr(encode(public.digest(convert_to('ortak-confidential-run-id/1:'||company::text||':'||encode(source,'hex'),'UTF8'),'sha256'),'hex'),1,32)::uuid
 WHERE octet_length(source)=32
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dm_source(company UUID,source BYTEA)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'format','ortak-confidential-dm-source/1','company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,
  'employee_public_key',encode(s.employee_public_key,'hex'),'human_public_key',encode(s.human_public_key,'hex'),
  'office_binding_id',s.office_binding_id,'key_version',s.key_version::text,
  'outer_event_id',encode(j.source_id,'hex'),'outer_event_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'outer_json_sha256',encode(j.source_hash,'hex'),'seal_event_id',encode(j.seal_id,'hex'),
  'seal_event_created_at',to_char(j.seal_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_event_id',encode(j.rumor_id,'hex'),'rumor_event_created_at',to_char(j.rumor_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_json_sha256',encode(j.rumor_hash,'hex'),'reply_rumor_id',encode(j.reply_to,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dm_identity(company UUID,source BYTEA,run UUID,key UUID)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'authority_epoch',j.office_generation::text,'company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,'employee_lifecycle_epoch',j.employee_lifecycle_epoch::text,
  'employee_public_key',encode(s.employee_public_key,'hex'),'employee_revision_id',j.employee_revision_id,
  'human_public_key',encode(s.human_public_key,'hex'),'key_id',key,'key_version',s.key_version::text,
  'office_binding_id',s.office_binding_id,'rumor_id',encode(j.rumor_id,'hex'),'run_id',run,
  'source_evidence_hash',encode(public.digest(public.ortak_confidential_dm_source(company,source),'sha256'),'hex'),
  'source_outer_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'source_outer_id',encode(j.source_id,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
 AND run=public.ortak_confidential_dm_run_id(company,source)
 AND key<>'00000000-0000-0000-0000-000000000000'
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dm_current(company UUID,run UUID)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_runs c
 JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
 JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
 JOIN public.encrypted_dm_selections s ON s.company_id=j.company_id AND s.selection_id=j.selection_id
 JOIN public.employees e ON e.company_id=j.company_id AND e.id=j.employee_id
 JOIN public.office_routing_cohorts co ON co.company_id=c.company_id AND co.community_id=c.community_id AND co.state='enabled'
 JOIN public.office_routing_channels ch ON ch.company_id=co.company_id AND ch.community_id=co.community_id AND ch.channel_id=s.channel_id
 JOIN public.office_routing_employees ce ON ce.company_id=co.company_id AND ce.employee_id=e.id
 JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=j.source_id
 JOIN public.events ev ON ev.community_id=c.community_id AND ev.id=j.source_id AND ev.created_at=j.source_created_at
 WHERE c.company_id=company AND c.run_id=run AND r.payload_mode='confidential_dm_v1'
 AND r.status IN('queued','running','waiting','completed') AND r.work_item_id IS NULL
 AND r.employee_id=j.employee_id AND r.employee_revision_id=j.employee_revision_id AND r.employee_lifecycle_epoch=j.employee_lifecycle_epoch
 AND r.message_id=j.source_id AND r.root_message_id=j.source_id
 AND s.enabled AND s.generation=j.selection_generation AND s.community_id=c.community_id
 AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
 AND public.ortak_encrypted_dm_pair_current(s) AND public.ortak_confidential_runtime_binding(company,j.employee_revision_id) IS NOT NULL
 AND coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=company),0)=j.office_generation
 AND clock_timestamp()<c.execution_deadline
 AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
 AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author AND i.received_at=j.source_received_at
 AND ev.kind=1059 AND ev.channel_id IS NULL AND ev.pubkey=j.source_author AND ev.deleted_at IS NULL
 AND ev.tags=jsonb_build_array(jsonb_build_array('p',encode(s.employee_public_key,'hex')))
 AND octet_length(ev.content) BETWEEN 132 AND 60000 AND octet_length(ev.tags::text)<=256
 AND public.digest(convert_to(public.ortak_conversation_json75(jsonb_build_object(
 'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
 'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex'))),'UTF8'),'sha256')=j.source_hash
 AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations stop WHERE stop.company_id=company AND stop.run_id=run))
$$;

CREATE OR REPLACE FUNCTION ortak_lock_confidential_dm(company UUID,run UUID) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE target RECORD;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id,source_id INTO target FROM public.confidential_runs WHERE company_id=company AND run_id=run;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=target.selection_id FOR SHARE;
 PERFORM 1 FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=target.source_id FOR SHARE;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=target.source_id FOR SHARE;
 RETURN public.ortak_confidential_dm_current(company,run);
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_payload_valid(bytes BYTEA,identity BYTEA,purpose TEXT,ordinal INTEGER)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE wire JSONB; header JSONB; size INTEGER; nonce BYTEA; cipher BYTEA; maximum INTEGER;
BEGIN
 IF octet_length(bytes)>98304 OR octet_length(identity)>2048 THEN RETURN false; END IF;
 wire:=convert_from(bytes,'UTF8')::jsonb;
 IF jsonb_typeof(wire) IS DISTINCT FROM 'object' OR NOT wire ?& ARRAY['ciphertext','header','nonce'] OR wire-ARRAY['ciphertext','header','nonce']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wire),'UTF8')<>bytes THEN RETURN false; END IF;
 header:=wire->'header';
 IF jsonb_typeof(header) IS DISTINCT FROM 'object' OR NOT header ?& ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose'] OR header-ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose']<>'{}'::jsonb
  OR header->>'algorithm' IS DISTINCT FROM 'A256GCM' OR header->>'format' IS DISTINCT FROM 'ortak-confidential-payload/1'
  OR header->>'purpose' IS DISTINCT FROM purpose OR header->'ordinal' IS DISTINCT FROM to_jsonb(ordinal)
  OR convert_to(public.ortak_conversation_json75(header->'identity'),'UTF8') IS DISTINCT FROM identity
  OR jsonb_typeof(header->'plaintext_bytes')<>'number' THEN RETURN false; END IF;
 maximum:=CASE purpose WHEN 'snapshot' THEN 49152 WHEN 'runtime_event' THEN 32768 WHEN 'reply_draft' THEN 16384 END;
 IF maximum IS NULL OR (purpose='runtime_event' AND ordinal NOT BETWEEN 1 AND 512)
  OR (purpose<>'runtime_event' AND ordinal<>0) THEN RETURN false; END IF;
 IF (header->>'plaintext_bytes')!~'^(0|[1-9][0-9]{0,5})$' THEN RETURN false; END IF;
 size:=(header->>'plaintext_bytes')::integer;
 IF size>maximum OR jsonb_typeof(wire->'nonce')<>'string' OR length(wire->>'nonce')<>16
  OR jsonb_typeof(wire->'ciphertext')<>'string' OR length(wire->>'ciphertext')>65560 THEN RETURN false; END IF;
 nonce:=decode(wire->>'nonce','base64'); cipher:=decode(wire->>'ciphertext','base64');
 RETURN octet_length(nonce)=12 AND octet_length(cipher)=size+16
  AND replace(encode(nonce,'base64'),E'\n','')=wire->>'nonce'
  AND replace(encode(cipher,'base64'),E'\n','')=wire->>'ciphertext';
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_run_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; r public.runs; wrapped JSONB; cipher BYTEA;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential run bytes are immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT r FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR (j.community_id,j.selection_id,j.employee_id,j.rumor_id,s.human_public_key) IS DISTINCT FROM
     (NEW.community_id,NEW.selection_id,NEW.employee_id,NEW.rumor_id,NEW.human_public_key)
  OR r.payload_mode<>'confidential_dm_v1' OR r.status<>'queued' OR r.runtime_run_ref IS NOT NULL
  OR (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.message_id,r.root_message_id) IS DISTINCT FROM
     (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.source_id,j.source_id)
  OR r.work_item_id IS NOT NULL OR public.ortak_confidential_runtime_binding(NEW.company_id,j.employee_revision_id) IS NULL
  OR NEW.run_id<>public.ortak_confidential_dm_run_id(NEW.company_id,NEW.source_id)
  OR NEW.identity_bytes IS DISTINCT FROM public.ortak_confidential_dm_identity(NEW.company_id,NEW.source_id,NEW.run_id,NEW.key_id)
  OR NEW.source_bytes IS DISTINCT FROM public.ortak_confidential_dm_source(NEW.company_id,NEW.source_id)
  OR NEW.admission_deadline IS DISTINCT FROM j.claim_expires_at THEN
  RAISE EXCEPTION 'Confidential run requires exact current verified claim' USING ERRCODE='check_violation';
 END IF;
 NEW.admitted_at:=clock_timestamp();
 IF NEW.execution_deadline>NEW.admitted_at+interval '10 minutes' THEN
  RAISE EXCEPTION 'Confidential execution deadline exceeds bound' USING ERRCODE='check_violation';
 END IF;
 wrapped:=convert_from(NEW.wrapped_key,'UTF8')::jsonb;
 IF jsonb_typeof(wrapped) IS DISTINCT FROM 'object' OR NOT wrapped ?& ARRAY['ciphertext','format','identity','purpose','signer_ref'] OR wrapped-ARRAY['ciphertext','format','identity','purpose','signer_ref']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wrapped),'UTF8')<>NEW.wrapped_key
  OR wrapped->>'format' IS DISTINCT FROM 'ortak-confidential-key-envelope/1'
  OR wrapped->>'purpose' IS DISTINCT FROM 'confidential_master'
  OR convert_to(wrapped->>'identity','UTF8') IS DISTINCT FROM NEW.identity_bytes
  OR wrapped->>'signer_ref' IS DISTINCT FROM s.decrypt_ref
  OR jsonb_typeof(wrapped->'ciphertext') IS DISTINCT FROM 'string'
  OR length(wrapped->>'ciphertext') NOT BETWEEN 132 AND 8192 THEN
  RAISE EXCEPTION 'Confidential wrapped key identity differs' USING ERRCODE='check_violation';
 END IF;
 cipher:=decode(wrapped->>'ciphertext','base64');
 IF octet_length(cipher)<99 OR get_byte(cipher,0)<>2 OR replace(encode(cipher,'base64'),E'\n','')<>wrapped->>'ciphertext' THEN
  RAISE EXCEPTION 'Confidential wrapped key encoding differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_payload_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; initial BOOLEAN; prior INTEGER;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential ciphertext is immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 initial:=NEW.purpose='snapshot' AND NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id);
 IF initial THEN
  IF NOT EXISTS(SELECT 1 FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=c.company_id AND j.source_id=c.source_id
   AND j.state='verified' AND j.claim_token=c.claim_token AND j.claim_expires_at>clock_timestamp() AND public.ortak_encrypted_dm_job_current(j)) THEN
   RAISE EXCEPTION 'Confidential initial snapshot claim expired' USING ERRCODE='check_violation';
  END IF;
 ELSIF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential payload authority retired' USING ERRCODE='check_violation';
 END IF;
 -- Serialize event ordinals without any plaintext parser or per-run unbounded scan.
 PERFORM 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
 IF NEW.community_id<>c.community_id OR NOT public.ortak_confidential_payload_valid(NEW.envelope_bytes,c.identity_bytes,NEW.purpose,NEW.ordinal)
  OR NEW.nonce IS DISTINCT FROM decode(convert_from(NEW.envelope_bytes,'UTF8')::jsonb->>'nonce','base64') THEN
  RAISE EXCEPTION 'Confidential payload wire differs' USING ERRCODE='check_violation';
 END IF;
 IF NEW.purpose='runtime_event' THEN
  IF NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('queued','running','waiting')) THEN
   RAISE EXCEPTION 'Confidential runtime event follows terminal run' USING ERRCODE='check_violation';
  END IF;
  SELECT coalesce(max(ordinal),0) INTO prior FROM public.confidential_run_payloads
   WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='runtime_event';
  IF NEW.ordinal<>prior+1 THEN RAISE EXCEPTION 'Confidential event sequence gap' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_receipt_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; c public.confidential_runs; s public.encrypted_dm_selections;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential receipt is retained' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR j.community_id<>NEW.community_id OR c.employee_id<>j.employee_id OR c.human_public_key<>s.human_public_key OR c.rumor_id<>j.rumor_id
  OR NEW.duplicate_rumor IS DISTINCT FROM (NEW.source_id<>c.source_id) THEN
  RAISE EXCEPTION 'Confidential receipt needs exact verified rumor' USING ERRCODE='check_violation';
 END IF;
 NEW.committed_at:=clock_timestamp();
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_consumed_job() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=OLD.company_id AND source_id=OLD.source_id)
  AND NEW IS DISTINCT FROM OLD THEN
  RAISE EXCEPTION 'Consumed decrypt job cannot be reclaimed' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; receipt public.confidential_dm_receipts;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF TG_TABLE_NAME='confidential_dm_receipts' THEN
  SELECT * INTO STRICT receipt FROM public.confidential_dm_receipts WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=j.company_id AND selection_id=j.selection_id;
  IF j.claim_expires_at<=clock_timestamp() OR j.valid_before<=clock_timestamp()
   OR coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=j.company_id),0)<>j.office_generation
   OR NOT s.enabled OR s.generation<>j.selection_generation OR NOT public.ortak_encrypted_dm_pair_current(s)
   OR NOT EXISTS(SELECT 1 FROM public.office_inbox i WHERE i.company_id=j.company_id AND i.event_id=j.source_id
       AND i.state=(CASE WHEN receipt.duplicate_rumor THEN 'dropped' ELSE 'decided' END) AND i.finalized_at IS NOT NULL) THEN
   RAISE EXCEPTION 'Confidential receipt authority expired before commit' USING ERRCODE='serialization_failure';
  END IF;
  IF receipt.duplicate_rumor THEN RETURN NEW; END IF;
 END IF;
 IF NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential current authority expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 IF TG_TABLE_NAME='confidential_runs' OR (TG_TABLE_NAME='confidential_run_payloads' AND to_jsonb(NEW)->>'purpose'='snapshot') THEN
  IF c.admission_deadline<=clock_timestamp()
   OR NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id AND run_id=c.run_id AND NOT duplicate_rumor)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=c.company_id AND run_id=c.run_id AND purpose='snapshot' AND ordinal=0)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_dispatches WHERE company_id=c.company_id AND run_id=c.run_id AND state='pending' AND attempts=0) THEN
   RAISE EXCEPTION 'Confidential admission is incomplete or expired' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_run_mode_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.payload_mode IS DISTINCT FROM OLD.payload_mode THEN
  RAISE EXCEPTION 'Run payload mode is immutable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 IF NEW.work_item_id IS NOT NULL OR NEW.routing_decision_id IS NULL OR NEW.message_id IS NULL OR NEW.root_message_id<>NEW.message_id
  OR NEW.error_message IS NOT NULL OR (NEW.error_code IS NOT NULL AND NEW.error_code NOT IN('confidential_failed','confidential_cancelled'))
  OR (NEW.cancel_reason IS NOT NULL AND NEW.cancel_reason NOT IN('office_revoked','human_requested'))
  OR (NEW.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref!~'^[A-Za-z0-9][A-Za-z0-9:._/-]{0,255}$') THEN
  RAISE EXCEPTION 'Confidential run permits bounded metadata only' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND
  (to_jsonb(NEW)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) IS DISTINCT FROM
  (to_jsonb(OLD)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) THEN
  RAISE EXCEPTION 'Confidential run authority is immutable' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.status IN('completed','failed','cancelled') AND NEW.status<>OLD.status THEN
  RAISE EXCEPTION 'Confidential terminal status cannot revive' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref IS DISTINCT FROM OLD.runtime_run_ref THEN
  RAISE EXCEPTION 'Confidential start correlation cannot change' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential fresh execution authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_reject_ordinary() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.payload_mode='confidential_dm_v1') THEN
  RAISE EXCEPTION 'Confidential run cannot use an ordinary content path' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dispatch_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Confidential dispatch is retained' USING ERRCODE='check_violation'; END IF;
 SELECT execution_deadline INTO STRICT deadline FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Confidential dispatch initial state' USING ERRCODE='check_violation';
  END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
   OR OLD.state<>'pending' THEN RAISE EXCEPTION 'Confidential dispatch identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.state='pending'
   AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
   AND OLD.next_attempt_at<=clock_timestamp() AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp())
   AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=least(deadline,clock_timestamp()+interval '30 seconds') THEN
   fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- Exact lease accounting remains possible after source/Office revocation.
   -- A delivered result requires a retained start reference; it grants no start.
   IF NEW.state='delivered' AND (OLD.lease_expires_at<=clock_timestamp() OR OLD.lease_token IS NULL
      OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND runtime_run_ref IS NOT NULL)) THEN
    RAISE EXCEPTION 'Confidential delivery needs retained start receipt' USING ERRCODE='check_violation';
   ELSIF NEW.state='pending' AND (NEW.error_code<>'unavailable' OR OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
     OR NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+(CASE WHEN NEW.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)) THEN
    RAISE EXCEPTION 'Confidential retry is not bounded lease accounting' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Confidential dispatch lease transition refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF NEW.next_attempt_at>deadline+interval '5 seconds' THEN RAISE EXCEPTION 'Confidential retry deadline exceeded' USING ERRCODE='check_violation'; END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential dispatch authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_dispatch_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
  IF NEW.lease_expires_at<=clock_timestamp() OR NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
   RAISE EXCEPTION 'Confidential dispatch expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_commit_confidential_dm(company UUID,source BYTEA,run UUID,key UUID,identity BYTEA,wrapped BYTEA,snapshot BYTEA,nonce BYTEA)
RETURNS TABLE(committed_run_id UUID,duplicate_rumor BOOLEAN)
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; old public.confidential_runs;
 decision UUID; input_hash BYTEA; policy_hash TEXT; binding JSONB;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source FOR UPDATE;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR identity IS DISTINCT FROM public.ortak_confidential_dm_identity(company,source,run,key) THEN
  RAISE EXCEPTION 'Confidential verified commit claim changed' USING ERRCODE='check_violation';
 END IF;
 -- Serialize absent rumor discovery before allocating any run/chain. The hash
 -- lock is collision-conservative; the full unique tuple is the authority.
 IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-confidential-rumor:'||company::text||':'||j.employee_id||':'||encode(s.human_public_key,'hex')||':'||encode(j.rumor_id,'hex'),0)) THEN
  RAISE EXCEPTION 'Confidential rumor commit busy' USING ERRCODE='serialization_failure';
 END IF;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=source FOR UPDATE;
 IF NOT public.ortak_encrypted_dm_job_current(j) THEN RAISE EXCEPTION 'Confidential source claimed elsewhere' USING ERRCODE='serialization_failure'; END IF;
 SELECT * INTO old FROM public.confidential_runs WHERE company_id=company AND employee_id=j.employee_id AND human_public_key=s.human_public_key AND rumor_id=j.rumor_id;
 IF FOUND THEN
  INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
   VALUES(company,j.community_id,source,old.run_id,true,j.claim_generation,j.claim_token,j.worker_id);
  UPDATE public.office_inbox SET state='dropped',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
  RETURN QUERY SELECT old.run_id,true; RETURN;
 END IF;
 binding:=public.ortak_confidential_runtime_binding(company,j.employee_revision_id);
 IF binding IS NULL THEN RAISE EXCEPTION 'Confidential selected policy is not empty' USING ERRCODE='check_violation'; END IF;
 decision:=run; input_hash:=public.digest(public.ortak_confidential_dm_source(company,source),'sha256');
 policy_hash:='sha256:'||encode(public.digest(convert_to('ortak-confidential-dm-direct/1','UTF8'),'sha256'),'hex');
 INSERT INTO public.delivery_chains(company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes,hop_count,wake_count)
  VALUES(company,source,'confidential_dm_v1',policy_hash,1,1,0,0);
 INSERT INTO public.routing_decisions(company_id,id,message_id,root_message_id,inbox_claim_generation,origin_type,origin_id,mode,summary_reason,
  policy_version,policy_fingerprint,input_hash,candidate_revision_ids,wake_count,hop_consumed,chain_hop_count,chain_wake_count,
  office_authority_generation,office_authority_valid_before,office_input_hash)
 VALUES(company,decision,source,source,0,'human',encode(s.human_public_key,'hex'),'deterministic','direct_message',
  'confidential_dm_v1',policy_hash,input_hash,jsonb_build_array(j.employee_revision_id),1,true,1,1,j.office_generation,j.claim_expires_at,j.source_hash);
 INSERT INTO public.routing_recipients(company_id,routing_decision_id,employee_id,position,action,reason,employee_revision_id,employee_lifecycle_epoch)
  VALUES(company,decision,j.employee_id,0,'wake','direct_message',j.employee_revision_id,j.employee_lifecycle_epoch);
 INSERT INTO public.delivery_chain_visits(company_id,root_message_id,employee_id,routing_decision_id,recipient_action,batch_hop)
  VALUES(company,source,j.employee_id,decision,'wake',1);
 UPDATE public.delivery_chains SET hop_count=1,wake_count=1,updated_at=clock_timestamp() WHERE company_id=company AND root_message_id=source;
 INSERT INTO public.runs(company_id,id,employee_id,employee_revision_id,routing_decision_id,message_id,root_message_id,runtime_adapter,
  payload_mode,employee_lifecycle_epoch,office_admission_generation,office_admission_valid_before,office_admission_token)
 VALUES(company,run,j.employee_id,j.employee_revision_id,decision,source,source,binding->>'adapter','confidential_dm_v1',j.employee_lifecycle_epoch,
  j.office_generation,j.claim_expires_at,j.claim_token);
 INSERT INTO public.confidential_runs(company_id,community_id,run_id,source_id,selection_id,employee_id,human_public_key,rumor_id,key_id,
  identity_bytes,source_bytes,wrapped_key,start_key,admission_deadline,execution_deadline,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,run,source,s.selection_id,j.employee_id,s.human_public_key,j.rumor_id,key,identity,
  public.ortak_confidential_dm_source(company,source),wrapped,'ortak-run:'||company::text||':'||run::text,j.claim_expires_at,clock_timestamp()+interval '10 minutes',j.claim_generation,j.claim_token,j.worker_id);
 INSERT INTO public.confidential_run_payloads(company_id,community_id,run_id,purpose,ordinal,envelope_bytes,nonce)
 VALUES(company,j.community_id,run,'snapshot',0,snapshot,nonce);
 INSERT INTO public.confidential_run_dispatches(company_id,community_id,run_id) VALUES(company,j.community_id,run);
 INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,source,run,false,j.claim_generation,j.claim_token,j.worker_id);
 UPDATE public.office_inbox SET state='decided',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
 RETURN QUERY SELECT run,false;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_run_complete_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs;
BEGIN
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 SELECT * INTO c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Confidential run has no protected admission' USING ERRCODE='check_violation'; END IF;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=c.company_id AND source_id=c.source_id;
 IF NOT EXISTS(SELECT 1 FROM public.routing_decisions d
  JOIN public.routing_recipients rr ON rr.company_id=d.company_id AND rr.routing_decision_id=d.id AND rr.employee_id=j.employee_id
  JOIN public.delivery_chain_visits v ON v.company_id=d.company_id AND v.root_message_id=d.root_message_id AND v.employee_id=rr.employee_id AND v.routing_decision_id=d.id
  JOIN public.delivery_chains ch ON ch.company_id=d.company_id AND ch.root_message_id=d.root_message_id
  WHERE d.company_id=c.company_id AND d.id=NEW.routing_decision_id AND d.message_id=j.source_id AND d.root_message_id=j.source_id
   AND d.id=NEW.id AND d.mode='deterministic' AND d.summary_reason='direct_message'
   AND d.policy_version='confidential_dm_v1' AND d.inbox_claim_generation=0 AND d.origin_type='human' AND d.origin_id=encode(c.human_public_key,'hex')
   AND d.input_hash=public.digest(c.source_bytes,'sha256') AND d.office_input_hash=j.source_hash
   AND d.wake_count=1 AND d.hop_consumed AND d.chain_hop_count=1 AND d.chain_wake_count=1
   AND rr.action='wake' AND rr.employee_revision_id=j.employee_revision_id AND rr.employee_lifecycle_epoch=j.employee_lifecycle_epoch
   AND v.batch_hop=1 AND ch.hop_count=1 AND ch.wake_count=1 AND ch.max_hops=1 AND ch.max_wakes=1)
  OR NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential admission routing provenance differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_run_transition_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.payload_mode='confidential_dm_v1' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential execution expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_check_routing_claim_expiry() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_claim RECORD;
BEGIN
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id
        AND r.id=NEW.id AND r.routing_decision_id=NEW.id AND r.payload_mode='confidential_dm_v1') THEN
        PERFORM public.ortak_lock_office_authority(NEW.company_id);
        IF NOT EXISTS(SELECT 1 FROM public.confidential_runs c
          JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
          JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
          JOIN public.confidential_dm_receipts receipt ON receipt.company_id=c.company_id AND receipt.source_id=c.source_id AND receipt.run_id=c.run_id
          JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=c.source_id
          WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id AND c.source_id=NEW.message_id
            AND NEW.root_message_id=c.source_id AND NEW.inbox_claim_generation=0
            AND NEW.policy_version='confidential_dm_v1' AND NEW.mode='deterministic'
            AND NEW.origin_type='human' AND NEW.origin_id=encode(c.human_public_key,'hex')
            AND NEW.wake_count=1 AND NEW.hop_consumed
            AND NEW.office_authority_generation=j.office_generation
            AND NEW.office_authority_valid_before=j.claim_expires_at AND NEW.office_input_hash=j.source_hash
            AND NEW.input_hash=public.digest(c.source_bytes,'sha256')
            AND j.state='verified' AND j.claim_expires_at>clock_timestamp() AND j.valid_before>clock_timestamp()
            AND c.admission_deadline=j.claim_expires_at AND c.admission_deadline>clock_timestamp()
            AND (c.claim_generation,c.claim_token,c.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND (receipt.claim_generation,receipt.claim_token,receipt.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND NOT receipt.duplicate_rumor
            AND (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.office_admission_token)=
                (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.claim_token)
            AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
            AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author
            AND public.ortak_confidential_dm_current(c.company_id,c.run_id)) THEN
            RAISE EXCEPTION 'ortak: confidential decrypt claim changed or expired before commit'
                USING ERRCODE='serialization_failure';
        END IF;
        RETURN NEW;
    END IF;
    -- Unchanged migration53 ordinary inbox-claim branch.
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
    IF NOT FOUND OR current_claim.state NOT IN ('claimed', 'decided')
       OR current_claim.claim_generation IS DISTINCT FROM NEW.inbox_claim_generation
       OR current_claim.claim_expires_at IS NULL
       OR clock_timestamp() >= current_claim.claim_expires_at THEN
        RAISE EXCEPTION 'ortak: waking routing claim changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_run_origin(company UUID, run UUID, destination UUID)
RETURNS TABLE(origin_bytes BYTEA,observed_at TIMESTAMPTZ,valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    WITH base AS MATERIALIZED (
        SELECT r.*, b.community_id,$3 AS destination_channel_id,active.manifest AS active_manifest,
            pinned.manifest AS pinned_manifest
        FROM runs r
        JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
            AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_revisions pinned ON pinned.company_id=r.company_id
            AND pinned.employee_id=r.employee_id AND pinned.id=r.employee_revision_id
        JOIN employee_revisions active ON active.company_id=e.company_id
            AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN office_company_bindings b ON b.company_id=r.company_id
        JOIN office_routing_cohorts cohort ON cohort.company_id=r.company_id
            AND cohort.community_id=b.community_id AND cohort.state='enabled'
        JOIN office_routing_channels ch ON ch.company_id=cohort.company_id
            AND ch.community_id=cohort.community_id AND ch.channel_id=$3
        JOIN office_routing_employees selected ON selected.company_id=r.company_id
            AND selected.employee_id=r.employee_id
        WHERE r.company_id=$1 AND r.id=$2
            AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary'
            AND pinned.manifest->'office'=active.manifest->'office'
            AND pinned.manifest->'memory'=active.manifest->'memory'
            AND NOT EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
            AND NOT EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=r.company_id AND c.run_id=r.id)
    ), origins AS (
        SELECT i.author_pubkey AS human,r.message_id AS source,i.event_created_at AS source_created_at,r.employee_id
        FROM base r
        JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
            AND d.message_id=r.message_id AND d.root_message_id=r.root_message_id
            AND d.origin_type='human' AND d.office_authority_generation IS NOT NULL
            AND d.office_input_hash IS NOT NULL
        JOIN routing_recipients recipient ON recipient.company_id=r.company_id
            AND recipient.routing_decision_id=d.id AND recipient.employee_id=r.employee_id
            AND recipient.action='wake' AND recipient.employee_revision_id=r.employee_revision_id
            AND recipient.employee_lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN delivery_chain_visits visit ON visit.company_id=r.company_id
            AND visit.root_message_id=d.root_message_id AND visit.employee_id=r.employee_id
            AND visit.routing_decision_id=d.id
        JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
            AND i.channel_id=r.destination_channel_id AND i.state='decided'
            AND d.origin_id=encode(i.author_pubkey,'hex')
        WHERE r.work_item_id IS NULL AND r.status IN('queued','running','waiting','completed')
        UNION ALL
        SELECT decode(x.requested_by,'hex'), w.source_message_id, input.event_created_at, r.employee_id
        FROM base r JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
            AND x.work_item_id=r.work_item_id
            AND x.employee_id=r.employee_id AND x.employee_revision_id=r.employee_revision_id
        JOIN work_items w ON w.company_id=x.company_id AND w.project_id=x.project_id AND w.id=x.work_item_id
        JOIN project_api_bindings project_binding ON project_binding.company_id=x.company_id
            AND project_binding.project_id=x.project_id AND project_binding.community_id=r.community_id
            AND project_binding.channel_id=r.destination_channel_id
        JOIN office_inbox input ON input.company_id=x.company_id AND input.event_id=w.source_message_id
            AND input.author_pubkey=decode(x.requested_by,'hex') AND input.state='decided'
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants acl ON acl.company_id=x.company_id AND acl.project_id=x.project_id
            AND acl.actor_pubkey=x.requested_by AND acl.role IN('owner','contributor') AND acl.revoked_at IS NULL
        WHERE w.source_message_id IS NOT NULL AND r.routing_decision_id IS NULL
            AND r.message_id IS NULL AND r.root_message_id IS NULL
            AND EXISTS(SELECT 1 FROM work_assignments a WHERE a.company_id=x.company_id
                AND a.work_item_id=x.work_item_id AND a.employee_id=x.employee_id
                AND a.status='active' AND a.role IN('owner','contributor'))
            AND ((w.state='in_progress' AND w.version=x.execution_version
                AND x.reconciled_at IS NULL AND r.status IN('queued','running','waiting','completed')
                AND (r.work_admission_generation=g.generation OR r.status='queued' AND r.work_admission_generation IS NULL)
                AND NOT EXISTS(SELECT 1 FROM work_dependencies d JOIN work_items dependency
                    ON dependency.company_id=d.company_id AND dependency.id=d.depends_on_work_item_id
                    WHERE d.company_id=x.company_id AND d.work_item_id=x.work_item_id AND d.released_at IS NULL
                        AND dependency.state NOT IN('completed','cancelled'))
                AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria c WHERE c.company_id=x.company_id
                    AND c.work_item_id=x.work_item_id AND c.status<>'pending')
                AND NOT EXISTS(SELECT 1 FROM work_approvals a WHERE a.company_id=x.company_id
                    AND a.work_item_id=x.work_item_id AND a.status<>'pending'))
              -- A materialized result remains inspectable after human review.
              -- This branch cannot create a new run or first artifact: both
              -- exact retained artifact and materialized output must exist.
              OR (r.status='completed' AND w.state IN('review','completed') AND x.result_code='result_ready'
                AND x.reconciled_at IS NOT NULL AND EXISTS(SELECT 1 FROM runtime_work_outputs output
                    JOIN artifacts artifact ON artifact.company_id=output.company_id AND artifact.id=output.artifact_id
                        AND artifact.run_id=output.run_id AND artifact.project_id=x.project_id AND artifact.work_item_id=x.work_item_id
                    WHERE output.company_id=r.company_id AND output.run_id=r.id AND output.state='materialized')))
    ), unique_origin AS (
        SELECT * FROM origins WHERE (SELECT count(*) FROM origins)=1
    )
    SELECT convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-run-origin/1','company_id',$1,
        'employee_id',o.employee_id,'destination_channel_id',$3,
        'requester_public_key',encode(o.human,'hex'),
        'source_authority_epoch',source_scope.epoch,'destination_authority_epoch',destination_scope.epoch,
        'source',jsonb_build_object('community_id',s.community_id,'channel_id',s.source_channel_id,
            'event_id',encode(o.source,'hex'),'event_created_at',ortak_employee_memory_timestamp(o.source_created_at),
            'author_public_key',encode(s.source_author_public_key,'hex'),
            'evidence_hash',encode(s.source_evidence_hash,'hex')))),'UTF8'),s.observed_at,s.valid_before
    FROM unique_origin o CROSS JOIN LATERAL
        ortak_employee_memory_observation($1,o.employee_id,o.human,o.source,o.source_created_at,$3,'experience',NULL) s
    JOIN employee_memory_channel_authorities source_scope ON source_scope.company_id=$1
        AND source_scope.community_id=s.community_id AND source_scope.employee_id=o.employee_id
        AND source_scope.channel_id=s.source_channel_id
    JOIN employee_memory_channel_authorities destination_scope ON destination_scope.company_id=$1
        AND destination_scope.community_id=s.community_id AND destination_scope.employee_id=o.employee_id
        AND destination_scope.channel_id=$3
    WHERE s.valid_before IS NULL OR s.valid_before>clock_timestamp()
$$;

CREATE OR REPLACE FUNCTION ortak_employee_reviewed_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    source_epoch BIGINT,destination_epoch BIGINT,target_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        JOIN runs r ON r.company_id=f.company_id AND r.id=$2 AND r.employee_id=f.employee_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id AND e.status='active'
            AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
            AND x.employee_id=f.employee_id AND x.community_id=f.community_id
            AND x.destination_channel_id=f.destination_channel_id
            AND x.content_hash=f.content_hash AND x.source_hash=f.source_hash AND x.sharing_hash=f.sharing_hash
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            AND t.employee_id=f.employee_id AND t.community_id=f.community_id AND t.destination_channel_id=f.destination_channel_id
        JOIN employee_reviewed_memory_export_receipts ack ON ack.company_id=x.company_id AND ack.fact_id=x.fact_id
            AND ack.action='publish' AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store
            AND ack.binding_hash=t.binding_hash AND ack.content_hash=f.content_hash
        JOIN employee_memory_channel_authorities source_scope ON source_scope.company_id=f.company_id
            AND source_scope.community_id=f.community_id AND source_scope.employee_id=f.employee_id
            AND source_scope.channel_id=f.source_channel_id AND source_scope.epoch=$5
        JOIN employee_memory_channel_authorities destination_scope ON destination_scope.company_id=f.company_id
            AND destination_scope.community_id=f.community_id AND destination_scope.employee_id=f.employee_id
            AND destination_scope.channel_id=f.destination_channel_id AND destination_scope.epoch=$6
        CROSS JOIN LATERAL ortak_employee_memory_run_origin($1,$2,f.destination_channel_id) run_origin
        CROSS JOIN LATERAL (SELECT convert_from(run_origin.origin_bytes,'UTF8')::jsonb AS value) origin
        CROSS JOIN LATERAL ortak_employee_memory_observation(f.company_id,f.employee_id,f.approved_by,
            f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key) observed
        WHERE f.company_id=$1 AND f.id=$3 AND t.id=$4 AND f.version=1 AND f.revoked_at IS NULL
            AND f.expires_at>clock_timestamp() AND t.enabled AND t.runtime_consumption_enabled
            AND t.consumption_epoch=$7 AND t.employee_lifecycle_epoch=e.lifecycle_epoch
            AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary'
            -- A model-only change keeps namespace identity. Use still requires
            -- the exact current memory binding and unchanged lifecycle/expiry.
            AND ortak_employee_memory_target_authorized(t.company_id,t.employee_id,t.deployment_id,t.namespace_bytes,
                t.binding,t.creation_receipt,e.active_revision_id,e.lifecycle_epoch,t.destination_channel_id,t.valid_until)
            AND observed.community_id=f.community_id AND observed.source_channel_id=f.source_channel_id
            AND observed.source_author_public_key=f.source_author_public_key AND observed.source_evidence_hash=f.source_evidence_hash
            AND observed.employee_revision_id=e.active_revision_id AND observed.employee_lifecycle_epoch=e.lifecycle_epoch
            AND (observed.valid_before IS NULL OR observed.valid_before>clock_timestamp())
            AND (run_origin.valid_before IS NULL OR run_origin.valid_before>clock_timestamp())
            AND (f.kind='experience' OR f.kind='relationship'
                AND origin.value->>'requester_public_key'=encode(f.human_public_key,'hex'))
            AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts stop
                WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw'))
$$;

CREATE OR REPLACE FUNCTION ortak_employee_use_ordinary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id
        AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary') THEN
        RAISE EXCEPTION 'employee memory requires ordinary run' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION ortak_run_employee_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        LEFT JOIN employee_reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        LEFT JOIN run_context_snapshots s ON s.company_id=u.company_id AND s.run_id=u.run_id
        WHERE u.company_id=$1 AND u.run_id=$2 AND (
            f.id IS NULL OR t.id IS NULL OR s.run_id IS NULL OR f.community_id IS DISTINCT FROM u.community_id
            OR f.version IS DISTINCT FROM u.fact_version OR f.approval_id IS DISTINCT FROM u.approval_id
            OR encode(f.approved_by,'hex') IS DISTINCT FROM u.approved_by OR f.expires_at IS DISTINCT FROM u.expires_at
            OR f.content_hash IS DISTINCT FROM u.content_hash OR f.source_hash IS DISTINCT FROM u.source_hash
            OR f.sharing_hash IS DISTINCT FROM u.sharing_hash OR f.audience_hash IS DISTINCT FROM u.audience_hash
            OR t.binding_hash IS DISTINCT FROM u.binding_hash OR t.namespace_hash IS DISTINCT FROM u.namespace_hash
            OR NOT coalesce(ortak_employee_reviewed_runtime_eligible($1,$2,u.fact_id,u.target_id,
                u.source_authority_epoch,u.destination_authority_epoch,u.consumption_epoch),false)
            OR NOT EXISTS(SELECT 1 FROM ortak_employee_memory_run_origin($1,$2,f.destination_channel_id) origin
                WHERE ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{employee,origin}'
                    =ortak_snapshot_scratch_jsonb(to_json(convert_from(origin.origin_bytes,'UTF8'))))))
$$;

CREATE OR REPLACE FUNCTION ortak_employee_snapshot_v5(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    r runs; revision employee_revisions; work work_executions;
    selected_project UUID; origin RECORD; context JSONB; record JSONB; pin JSONB;
    wrapped JSONB; rendered JSONB; expected_pin JSONB; expected_record JSONB;
    u run_reviewed_memory_uses; f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
    eu run_employee_reviewed_memory_uses; ef employee_reviewed_memory_facts; selected_destination UUID; employees INTEGER=0;
    previous_priority INTEGER=-1; previous_employee UUID; priority INTEGER;
    used_count INTEGER; scratch_count INTEGER; i INTEGER=0; conversations INTEGER=0;
    reviewed_bytes INTEGER=0; total_bytes INTEGER=0; content TEXT; seen UUID[]=ARRAY[]::uuid[];
BEGIN
    SELECT * INTO r FROM runs x WHERE x.company_id=company AND x.id=run;
    SELECT * INTO revision FROM employee_revisions x WHERE x.company_id=company
        AND x.employee_id=r.employee_id AND x.id=r.employee_revision_id;
    context=wire->'employee';
    IF r.id IS NULL OR revision.id IS NULL OR r.status NOT IN('queued','running','waiting')
        OR wire->'version' IS DISTINCT FROM '5'::jsonb
        OR wire ? 'reviewed' OR wire ? 'conversation'
        OR coalesce(to_jsonb(r)->>'payload_mode','ordinary')<>'ordinary' OR jsonb_typeof(context) IS DISTINCT FROM 'object'
        OR (context-'origin'-'conversation_origin'-'records'-'truncated')<>'{}'::jsonb
        OR jsonb_typeof(context->'truncated') IS DISTINCT FROM 'boolean'
        OR jsonb_typeof(context->'records') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array'
        OR wire->>'company_id' IS DISTINCT FROM company::text
        OR wire#>>'{spec,run_id}' IS DISTINCT FROM run::text
        OR wire#>>'{spec,employee_id}' IS DISTINCT FROM r.employee_id
        OR wire#>>'{spec,revision_id}' IS DISTINCT FROM r.employee_revision_id::text
        OR wire#>>'{spec,idempotency_key}' IS DISTINCT FROM 'ortak-run:'||company::text||':'||run::text
        OR wire#>'{spec,binding}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'runtime')::json)
        OR wire#>'{spec,permissions}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'permissions')::json)
        OR wire->'memory_binding' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'memory')::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot shape or run identity differs' USING ERRCODE='check_violation';
    END IF;
    SELECT (SELECT count(*) FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run)
        +(SELECT count(*) FROM run_employee_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run) INTO used_count;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF used_count NOT BETWEEN 1 AND 8 OR jsonb_array_length(context->'records')<>used_count
        OR scratch_count+used_count>8
        OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: conversation snapshot count differs' USING ERRCODE='check_violation';
    END IF;
    -- Select the project from immutable use/fact rows, never from the caller's
    -- JSON provenance. Every reviewed record below must have this same project.
    SELECT min(fact.project_id::text)::uuid INTO selected_project
        FROM run_reviewed_memory_uses used JOIN reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.project_id)=1;
    SELECT min(fact.destination_channel_id::text)::uuid INTO selected_destination
        FROM run_employee_reviewed_memory_uses used JOIN employee_reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.destination_channel_id)=1;
    SELECT * INTO origin FROM ortak_employee_memory_run_origin(company,run,selected_destination);
    IF NOT FOUND OR context->'origin' IS DISTINCT FROM
        ortak_snapshot_scratch_jsonb(to_json(convert_from(origin.origin_bytes,'UTF8'))) THEN
        RAISE EXCEPTION 'employee snapshot actual origin differs' USING ERRCODE='check_violation';
    END IF;
    IF r.work_item_id IS NULL THEN
        IF wire ? 'work_origin' OR wire->>'message_id' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire->>'root_message_id' IS DISTINCT FROM encode(r.root_message_id,'hex')
            OR wire->>'routing_decision_id' IS DISTINCT FROM r.routing_decision_id::text
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire#>>'{spec,context,reply_to_message_id}' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire#>'{spec,context,work_item_id}' IS DISTINCT FROM 'null'::jsonb
            OR NOT EXISTS(SELECT 1 FROM office_inbox inbox
                JOIN office_company_bindings office ON office.company_id=inbox.company_id
                JOIN events event ON event.community_id=office.community_id AND event.id=inbox.event_id
                    AND event.created_at=inbox.event_created_at AND event.kind=inbox.event_kind
                    AND event.channel_id=inbox.channel_id AND event.pubkey=inbox.author_pubkey
                CROSS JOIN LATERAL (SELECT regexp_replace(event.content,
                    U&'[\0001-\0008\000B\000C\000E-\001F\007F-\009F]','','g') AS cleaned) input
                WHERE inbox.company_id=company AND inbox.event_id=r.message_id
                AND wire->'event_kind'=to_jsonb(inbox.event_kind)
                AND wire#>>'{spec,context,conversation_ref}'=inbox.channel_id::text
                -- Source75 already caps the original text at65536 bytes;
                -- control removal cannot require UTF-8 truncation afterwards.
                AND event.deleted_at IS NULL AND octet_length(event.content)<=65536
                AND btrim(input.cleaned,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
                AND wire#>'{spec,input}'=ortak_snapshot_scratch_jsonb(to_json(input.cleaned))) THEN
            RAISE EXCEPTION 'ortak: conversation Office origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        SELECT * INTO work FROM work_executions x WHERE x.company_id=company AND x.run_id=run;
        IF work.run_id IS NULL OR (selected_project IS NOT NULL AND work.project_id<>selected_project)
            OR wire ? 'message_id' OR wire ? 'root_message_id' OR wire ? 'routing_decision_id'
            OR wire->'event_kind' IS DISTINCT FROM '0'::jsonb
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire->'work_origin' IS DISTINCT FROM jsonb_build_object('run_id',work.run_id,
                'work_item_id',work.work_item_id,'project_id',work.project_id,'execution_version',work.execution_version,
                'definition_hash',encode(work.definition_hash,'hex'))
            OR wire#>'{spec,input}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(convert_from(work.definition_bytes,'UTF8')))
            OR wire#>>'{spec,context,work_item_id}' IS DISTINCT FROM r.work_item_id::text
            OR wire#>'{spec,context,reply_to_message_id}' IS DISTINCT FROM 'null'::jsonb
            OR wire#>'{spec,context,conversation_ref}' IS DISTINCT FROM 'null'::jsonb THEN
            RAISE EXCEPTION 'ortak: conversation Work origin differs' USING ERRCODE='check_violation';
        END IF;
    END IF;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(used_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',(used_count+i)::text])>8192
            OR jsonb_typeof(record->'content') IS DISTINCT FROM 'string' THEN
            RAISE EXCEPTION 'ortak: conversation scratch rendering differs' USING ERRCODE='check_violation';
        END IF;
        content=record->>'content';
        total_bytes=total_bytes+octet_length(content)
            -(octet_length(content)-octet_length(regexp_replace(content,E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR wrapped IN SELECT value FROM jsonb_array_elements(context->'records') LOOP
        record=wrapped->'record'; pin=record->'pin';
        IF wrapped->>'scope'='employee' THEN
            SELECT * INTO eu FROM run_employee_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
            SELECT * INTO ef FROM employee_reviewed_memory_facts x WHERE x.company_id=company AND x.id=eu.fact_id;
            priority=CASE WHEN ef.kind='relationship' THEN 0 ELSE 1 END;
            IF eu.run_id IS NULL OR ef.id IS NULL OR ef.destination_channel_id<>selected_destination
                OR eu.fact_id=ANY(seen) OR i<>employees OR priority<previous_priority
                OR (priority=previous_priority AND eu.fact_id<=previous_employee)
                OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_targets target WHERE target.company_id=company
                    AND target.id=eu.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
                RAISE EXCEPTION 'employee snapshot retained identity or order differs' USING ERRCODE='check_violation';
            END IF;
            seen=array_append(seen,eu.fact_id); previous_priority=priority; previous_employee=eu.fact_id;
            expected_pin=jsonb_build_object('fact_id',eu.fact_id,'target_id',eu.target_id,'fact_version',eu.fact_version,
                'content_hash',encode(eu.content_hash,'hex'),'source_hash',encode(eu.source_hash,'hex'),
                'sharing_hash',encode(eu.sharing_hash,'hex'),'audience_hash',encode(eu.audience_hash,'hex'),
                'binding_hash',encode(eu.binding_hash,'hex'),'namespace_hash',encode(eu.namespace_hash,'hex'),
                'approval_id',eu.approval_id,'approved_by',eu.approved_by,'expires_at',pin->>'expires_at',
                'source_authority_epoch',eu.source_authority_epoch,'destination_authority_epoch',eu.destination_authority_epoch,
                'consumption_epoch',eu.consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',ef.content,'provenance',convert_from(ef.provenance_bytes,'UTF8'));
            IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
                OR wrapped IS DISTINCT FROM jsonb_build_object('scope','employee','record',record)
                OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM eu.expires_at THEN
                RAISE EXCEPTION 'employee snapshot retained bytes differ' USING ERRCODE='check_violation';
            END IF;
            employees=employees+1; reviewed_bytes=reviewed_bytes+octet_length(ef.content);
        ELSE
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR f.project_id<>selected_project OR u.fact_id=ANY(seen)
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets target WHERE target.company_id=company
                AND target.id=u.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
            RAISE EXCEPTION 'ortak: conversation retained record identity differs' USING ERRCODE='check_violation';
        END IF;
        seen=array_append(seen,u.fact_id);
        expected_pin=jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
            'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
            'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
            'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at');
        IF wrapped->>'scope'='conversation' AND f.audience_kind='conversation' THEN
            SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=f.id;
            IF NOT FOUND OR u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM a.audience_hash THEN
                RAISE EXCEPTION 'ortak: conversation audience pin differs' USING ERRCODE='check_violation';
            END IF;
            expected_pin=expected_pin||jsonb_build_object('conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
                'conversation_authority_epoch',u.conversation_authority_epoch,
                'conversation_consumption_epoch',u.conversation_consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content,'provenance',convert_from(a.provenance_bytes,'UTF8'));
            conversations=conversations+1;
        ELSIF wrapped->>'scope'='project' AND f.audience_kind='project' AND r.work_item_id IS NOT NULL THEN
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content);
        ELSE RAISE EXCEPTION 'ortak: conversation record scope differs' USING ERRCODE='check_violation';
        END IF;
        IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
            OR wrapped IS DISTINCT FROM jsonb_build_object('scope',wrapped->>'scope','record',record)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: conversation record bytes differ from retained use' USING ERRCODE='check_violation';
        END IF;
        reviewed_bytes=reviewed_bytes+octet_length(f.content);
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type',CASE WHEN wrapped->>'scope'='project'
                THEN 'reviewed_project_memory' WHEN wrapped->>'scope'='employee' THEN 'reviewed_employee_memory' ELSE 'reviewed_conversation_memory' END,'trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',i::text])>8192 THEN
            RAISE EXCEPTION 'ortak: conversation rendered bytes differ' USING ERRCODE='check_violation';
        END IF;
        i=i+1;
    END LOOP;
    IF conversations>0 THEN
        SELECT * INTO origin FROM ortak_conversation_run_origin(company,run,selected_project);
        IF NOT FOUND OR context->'conversation_origin' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(
            jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
                'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json) THEN
            RAISE EXCEPTION 'employee mixed conversation origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSIF context ? 'conversation_origin' THEN
        RAISE EXCEPTION 'employee context has unused conversation origin' USING ERRCODE='check_violation';
    END IF;
    IF employees=0 OR reviewed_bytes>8192 OR total_bytes+reviewed_bytes>16384
        OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: conversation budget or current authority differs' USING ERRCODE='check_violation';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.payload_mode='confidential_dm_v1' THEN RETURN NEW; END IF;
    IF NEW.work_item_id IS NULL AND NEW.routing_decision_id IS NOT NULL
       AND NEW.status='completed' AND NEW.delivery_intent IN('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES(NEW.company_id,NEW.id)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION ortak_confidential_execution_immutable() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN RAISE EXCEPTION 'Confidential execution history is retained' USING ERRCODE='check_violation'; END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_execution_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; fresh BOOLEAN:=false;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state NOT IN('observing','cancelling') OR NEW.generation<>0 OR NEW.failures<>0 OR NEW.cancel_attempts<>0 OR NEW.lease_token IS NOT NULL THEN
   RAISE EXCEPTION 'Invalid confidential supervision admission' USING ERRCODE='check_violation'; END IF;
  fresh:=NEW.state='observing';
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
    OR OLD.state IN('stopped','unconfirmed') OR (OLD.state='complete' AND NOT (NEW.state='cancelling'
        AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL
        AND EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='pending'))) THEN
   RAISE EXCEPTION 'Confidential supervision cannot revive' USING ERRCODE='check_violation'; END IF;
  IF NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>OLD.state OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+interval '30 seconds'
    OR NEW.failures<>OLD.failures OR NEW.cancel_attempts<>OLD.cancel_attempts+(CASE WHEN NEW.state='cancelling' THEN 1 ELSE 0 END) THEN
    RAISE EXCEPTION 'Confidential supervision lease refused' USING ERRCODE='check_violation'; END IF;
   fresh:=NEW.state IN('observing','sealing');
  ELSIF NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   IF NEW.cancel_attempts<>OLD.cancel_attempts OR (NEW.state=OLD.state AND NEW.state IN('observing','sealing')
      AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
       OR NEW.next_attempt_at<statement_timestamp()+interval '1 second'
       OR NEW.failures NOT IN(0,OLD.failures+1)))
    OR (OLD.state='cancelling' AND NEW.state NOT IN('cancelling','stopped','unconfirmed')) THEN
    RAISE EXCEPTION 'Confidential supervision settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential supervision generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND (NEW.generation>=124 OR clock_timestamp()>=c.execution_deadline OR NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id)) THEN
  RAISE EXCEPTION 'Confidential supervision authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_execution_commit() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=TG_OP='INSERT';
BEGIN
 IF NOT EXISTS(SELECT 1 FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id) THEN
  RAISE EXCEPTION 'Confidential execution community mismatch' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='confidential_execution_leases' THEN
  fresh:=NEW.state IN('observing','sealing') AND (TG_OP='INSERT' OR NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL);
  IF NEW.state='stopped' AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='acknowledged') THEN
   RAISE EXCEPTION 'Confidential stopped state lacks containment acknowledgement' USING ERRCODE='check_violation'; END IF;
  IF NEW.state IN('complete','sealing') AND NOT EXISTS(SELECT 1 FROM public.runs r
     WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status='completed'
      AND ((r.delivery_intent='silent' AND NEW.state='complete'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=3)
       OR (r.delivery_intent='reply'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=4
          AND EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0)
          AND (NEW.state='sealing' OR EXISTS(SELECT 1 FROM public.confidential_reply_bundles WHERE company_id=NEW.company_id AND run_id=NEW.run_id))))) THEN
   RAISE EXCEPTION 'Confidential terminal projection is incomplete' USING ERRCODE='check_violation'; END IF;
 ELSIF TG_TABLE_NAME='confidential_reply_outbox' AND TG_OP='UPDATE' THEN
  fresh:=NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL;
 END IF;
 IF fresh AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential execution authority expired at commit' USING ERRCODE='serialization_failure'; END IF;
 IF TG_TABLE_NAME='confidential_reply_bundles' THEN
  IF (SELECT count(*) FROM public.confidential_reply_outbox WHERE company_id=NEW.company_id AND run_id=NEW.run_id)<>2
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0) THEN
   RAISE EXCEPTION 'Confidential reply freeze is incomplete' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF TG_TABLE_NAME='confidential_run_payloads' THEN
  IF NEW.purpose='runtime_event' AND NOT EXISTS(SELECT 1 FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND ordinal=NEW.ordinal) THEN
   RAISE EXCEPTION 'Confidential event time receipt absent' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_reply_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE identity JSONB; wire JSONB; bytes BYTEA; target TEXT; expected BYTEA; n INTEGER;
BEGIN
 IF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status='completed' AND delivery_intent='reply') THEN
  RAISE EXCEPTION 'Confidential reply has no current completion' USING ERRCODE='check_violation'; END IF;
 SELECT convert_from(identity_bytes,'UTF8')::jsonb INTO STRICT identity FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 FOR n IN 0..1 LOOP
  bytes:=CASE n WHEN 0 THEN NEW.recipient_bytes ELSE NEW.history_bytes END;
  expected:=CASE n WHEN 0 THEN NEW.recipient_id ELSE NEW.history_id END;
  target:=identity->>(CASE n WHEN 0 THEN 'human_public_key' ELSE 'employee_public_key' END);
  wire:=convert_from(bytes,'UTF8')::jsonb;
  IF jsonb_typeof(wire)<>'object' OR NOT wire ?& ARRAY['id','pubkey','created_at','kind','tags','content','sig']
   OR wire-ARRAY['id','pubkey','created_at','kind','tags','content','sig']<>'{}'::jsonb
   OR wire->>'id' IS DISTINCT FROM encode(expected,'hex') OR wire->'kind' IS DISTINCT FROM '1059'::jsonb
   OR wire->'tags' IS DISTINCT FROM jsonb_build_array(jsonb_build_array('p',target))
   OR ((wire->>'pubkey')~'^[0-9a-f]{64}$') IS DISTINCT FROM true OR ((wire->>'sig')~'^[0-9a-f]{128}$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'created_at') IS DISTINCT FROM 'number'
   OR ((wire->>'created_at')~'^(0|[1-9][0-9]{0,11})$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'content') IS DISTINCT FROM 'string' OR octet_length(wire->>'content') NOT BETWEEN 132 AND 60000 THEN
   RAISE EXCEPTION 'Confidential reply copy mismatch' USING ERRCODE='check_violation'; END IF;
 END LOOP;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_confidential_reply_lease_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 SELECT c.execution_deadline INTO STRICT deadline FROM public.confidential_runs c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id AND c.community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL THEN RAISE EXCEPTION 'Invalid confidential output admission' USING ERRCODE='check_violation'; END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id,NEW.copy) IS DISTINCT FROM(OLD.company_id,OLD.community_id,OLD.run_id,OLD.copy) OR OLD.state<>'pending' THEN
   RAISE EXCEPTION 'Confidential output identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>'pending' OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>least(deadline,clock_timestamp()+interval '30 seconds') THEN
    RAISE EXCEPTION 'Confidential output lease refused' USING ERRCODE='check_violation'; END IF;fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- A known ACK for the unchanged locked owner is receipt-only after expiry.
   -- Pending retry still needs a live lease and cannot gain new authority here.
   IF NEW.state='acked' AND OLD.lease_token IS NULL
    OR NEW.state='pending' AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp())
    OR NEW.state='pending' AND (NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+interval '5 seconds') THEN
    RAISE EXCEPTION 'Confidential output settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential output generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential output authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;

CREATE OR REPLACE FUNCTION ortak_routing_notify() RETURNS TRIGGER AS $$
DECLARE
    message TEXT;
BEGIN
    IF TG_TABLE_NAME = 'routing_decisions' THEN
        message := encode(NEW.message_id, 'hex');
    ELSIF TG_TABLE_NAME <> 'office_authority_generations' THEN
        RAISE EXCEPTION 'invalid routing notification source' USING ERRCODE='55000';
    END IF;
    PERFORM pg_notify('ortak_routing_v1', json_build_object(
        'company_id', NEW.company_id, 'message_id', message)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;


-- Reviewed77 retained catalog convergence.
-- Only missing declarations and the known lost-deferrability form converge.
-- All other event/argument/WHEN/column/function/parent changes refuse.
SELECT attach_community_write_fence('employee_memory_channel_authorities');
SELECT attach_community_write_fence('employee_reviewed_memory_facts');
SELECT attach_community_write_fence('employee_reviewed_memory_operations');
SELECT attach_community_write_fence('employee_reviewed_memory_targets');
SELECT attach_community_write_fence('employee_reviewed_memory_exports');
SELECT attach_community_write_fence('employee_reviewed_memory_export_jobs');
SELECT attach_community_write_fence('employee_reviewed_memory_export_commands');
SELECT attach_community_write_fence('employee_reviewed_memory_export_receipts');
SELECT attach_community_write_fence('run_employee_reviewed_memory_uses');
SELECT attach_community_write_fence('encrypted_dm_selections');
SELECT attach_community_write_fence('encrypted_dm_decrypt_jobs');
SELECT attach_community_write_fence('confidential_runs');
SELECT attach_community_write_fence('confidential_run_payloads');
SELECT attach_community_write_fence('confidential_dm_receipts');
SELECT attach_community_write_fence('confidential_run_dispatches');
SELECT attach_community_write_fence('confidential_execution_leases');
SELECT attach_community_write_fence('confidential_event_receipts');
SELECT attach_community_write_fence('confidential_reply_bundles');
SELECT attach_community_write_fence('confidential_reply_outbox');

DO $reconcile77_triggers$
DECLARE item RECORD; observed JSONB; immediate_copy JSONB;
BEGIN
    FOR item IN SELECT * FROM (VALUES
        ('artifacts','confidential_no_artifact','["artifacts","confidential_no_artifact","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_artifact BEFORE INSERT OR UPDATE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('channel_members','employee_memory_epoch_members','["channel_members","employee_memory_epoch_members","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","6d656d626572736869700061756469656e63655f6368616e67656400636f6d6d756e6974795f6964006368616e6e656c5f6964007075626b657900726f6c650072656d6f7665645f617400",7,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_members AFTER INSERT OR DELETE OR UPDATE ON channel_members FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''membership'', ''audience_changed'', ''community_id'', ''channel_id'', ''pubkey'', ''role'', ''removed_at'')'),
        ('channels','employee_memory_epoch_channels','["channels","employee_memory_epoch_channels","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","6368616e6e656c0061756469656e63655f6368616e67656400636f6d6d756e6974795f6964006964006368616e6e656c5f74797065007669736962696c6974790061726368697665645f61740064656c657465645f6174007061727469636970616e745f686173680074746c5f7365636f6e64730074746c5f646561646c696e6500",11,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_channels AFTER INSERT OR DELETE OR UPDATE ON channels FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''channel'', ''audience_changed'', ''community_id'', ''id'', ''channel_type'', ''visibility'', ''archived_at'', ''deleted_at'', ''participant_hash'', ''ttl_seconds'', ''ttl_deadline'')'),
        ('communities','ortak_z_employee_memory_epoch_communities','["communities","ortak_z_employee_memory_epoch_communities","O",27,false,false,"public","ortak_employee_memory_epoch_mutation","636f6d6d756e6974790073636f70655f636c6f7365640069640064656c6574696f6e5f73746174650064656c6574696f6e5f66656e63655f67656e65726174696f6e0064656c657465645f617400",6,[],true,false,null]'::jsonb,'CREATE TRIGGER ortak_z_employee_memory_epoch_communities BEFORE DELETE OR UPDATE ON communities FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''community'', ''scope_closed'', ''id'', ''deletion_state'', ''deletion_fence_generation'', ''deleted_at'')'),
        ('companies','employee_memory_epoch_companies','["companies","employee_memory_epoch_companies","O",25,false,false,"public","ortak_employee_memory_epoch_mutation","636f6d70616e790073636f70655f636c6f7365640069640073746174757300",4,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_companies AFTER DELETE OR UPDATE ON companies FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''company'', ''scope_closed'', ''id'', ''status'')'),
        ('confidential_dm_receipts','community_write_fence_confidential_dm_receipts','["confidential_dm_receipts","community_write_fence_confidential_dm_receipts","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_dm_receipts BEFORE INSERT OR DELETE OR UPDATE ON confidential_dm_receipts FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_dm_receipts','confidential_receipt_at_commit','["confidential_dm_receipts","confidential_receipt_at_commit","O",5,true,true,"public","ortak_confidential_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_receipt_at_commit AFTER INSERT ON confidential_dm_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard()'),
        ('confidential_dm_receipts','confidential_receipt_guard','["confidential_dm_receipts","confidential_receipt_guard","O",31,false,false,"public","ortak_confidential_receipt_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_receipt_guard BEFORE INSERT OR DELETE OR UPDATE ON confidential_dm_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_receipt_guard()'),
        ('confidential_dm_receipts','confidential_receipts_no_truncate','["confidential_dm_receipts","confidential_receipts_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_receipts_no_truncate BEFORE TRUNCATE ON confidential_dm_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('confidential_event_receipts','community_write_fence_confidential_event_receipts','["confidential_event_receipts","community_write_fence_confidential_event_receipts","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_event_receipts BEFORE INSERT OR DELETE OR UPDATE ON confidential_event_receipts FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_event_receipts','confidential_event_at_commit','["confidential_event_receipts","confidential_event_at_commit","O",5,true,true,"public","ortak_confidential_execution_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_event_at_commit AFTER INSERT ON confidential_event_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit()'),
        ('confidential_event_receipts','confidential_event_immutable','["confidential_event_receipts","confidential_event_immutable","O",27,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_event_immutable BEFORE DELETE OR UPDATE ON confidential_event_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_event_receipts','confidential_event_no_truncate','["confidential_event_receipts","confidential_event_no_truncate","O",34,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_event_no_truncate BEFORE TRUNCATE ON confidential_event_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_execution_leases','community_write_fence_confidential_execution_leases','["confidential_execution_leases","community_write_fence_confidential_execution_leases","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_execution_leases BEFORE INSERT OR DELETE OR UPDATE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_execution_leases','confidential_execution_at_commit','["confidential_execution_leases","confidential_execution_at_commit","O",21,true,true,"public","ortak_confidential_execution_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_execution_at_commit AFTER INSERT OR UPDATE ON confidential_execution_leases DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit()'),
        ('confidential_execution_leases','confidential_execution_guard','["confidential_execution_leases","confidential_execution_guard","O",23,false,false,"public","ortak_confidential_execution_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_execution_guard BEFORE INSERT OR UPDATE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_guard()'),
        ('confidential_execution_leases','confidential_execution_no_truncate','["confidential_execution_leases","confidential_execution_no_truncate","O",34,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_execution_no_truncate BEFORE TRUNCATE ON confidential_execution_leases FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_execution_leases','confidential_execution_retain','["confidential_execution_leases","confidential_execution_retain","O",11,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_execution_retain BEFORE DELETE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_reply_bundles','community_write_fence_confidential_reply_bundles','["confidential_reply_bundles","community_write_fence_confidential_reply_bundles","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_reply_bundles BEFORE INSERT OR DELETE OR UPDATE ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_reply_bundles','confidential_reply_at_commit','["confidential_reply_bundles","confidential_reply_at_commit","O",5,true,true,"public","ortak_confidential_execution_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_reply_at_commit AFTER INSERT ON confidential_reply_bundles DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit()'),
        ('confidential_reply_bundles','confidential_reply_guard','["confidential_reply_bundles","confidential_reply_guard","O",7,false,false,"public","ortak_confidential_reply_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_reply_guard BEFORE INSERT ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_guard()'),
        ('confidential_reply_bundles','confidential_reply_immutable','["confidential_reply_bundles","confidential_reply_immutable","O",27,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_reply_immutable BEFORE DELETE OR UPDATE ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_reply_bundles','confidential_reply_no_truncate','["confidential_reply_bundles","confidential_reply_no_truncate","O",34,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_reply_no_truncate BEFORE TRUNCATE ON confidential_reply_bundles FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_reply_outbox','community_write_fence_confidential_reply_outbox','["confidential_reply_outbox","community_write_fence_confidential_reply_outbox","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_reply_outbox BEFORE INSERT OR DELETE OR UPDATE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_reply_outbox','confidential_outbox_at_commit','["confidential_reply_outbox","confidential_outbox_at_commit","O",21,true,true,"public","ortak_confidential_execution_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_outbox_at_commit AFTER INSERT OR UPDATE ON confidential_reply_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit()'),
        ('confidential_reply_outbox','confidential_outbox_no_truncate','["confidential_reply_outbox","confidential_outbox_no_truncate","O",34,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_outbox_no_truncate BEFORE TRUNCATE ON confidential_reply_outbox FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_reply_outbox','confidential_outbox_retain','["confidential_reply_outbox","confidential_outbox_retain","O",11,false,false,"public","ortak_confidential_execution_immutable","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_outbox_retain BEFORE DELETE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable()'),
        ('confidential_reply_outbox','confidential_reply_lease_guard','["confidential_reply_outbox","confidential_reply_lease_guard","O",23,false,false,"public","ortak_confidential_reply_lease_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_reply_lease_guard BEFORE INSERT OR UPDATE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_lease_guard()'),
        ('confidential_run_dispatches','community_write_fence_confidential_run_dispatches','["confidential_run_dispatches","community_write_fence_confidential_run_dispatches","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_run_dispatches BEFORE INSERT OR DELETE OR UPDATE ON confidential_run_dispatches FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_run_dispatches','confidential_dispatch_at_commit','["confidential_run_dispatches","confidential_dispatch_at_commit","O",17,true,true,"public","ortak_confidential_dispatch_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_dispatch_at_commit AFTER UPDATE ON confidential_run_dispatches DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_commit_guard()'),
        ('confidential_run_dispatches','confidential_dispatch_guard','["confidential_run_dispatches","confidential_dispatch_guard","O",31,false,false,"public","ortak_confidential_dispatch_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_dispatch_guard BEFORE INSERT OR DELETE OR UPDATE ON confidential_run_dispatches FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_guard()'),
        ('confidential_run_dispatches','confidential_dispatches_no_truncate','["confidential_run_dispatches","confidential_dispatches_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_dispatches_no_truncate BEFORE TRUNCATE ON confidential_run_dispatches FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('confidential_run_payloads','community_write_fence_confidential_run_payloads','["confidential_run_payloads","community_write_fence_confidential_run_payloads","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_run_payloads BEFORE INSERT OR DELETE OR UPDATE ON confidential_run_payloads FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_run_payloads','confidential_event_payload_at_commit','["confidential_run_payloads","confidential_event_payload_at_commit","O",5,true,true,"public","ortak_confidential_execution_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_event_payload_at_commit AFTER INSERT ON confidential_run_payloads DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit()'),
        ('confidential_run_payloads','confidential_payload_at_commit','["confidential_run_payloads","confidential_payload_at_commit","O",5,true,true,"public","ortak_confidential_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_payload_at_commit AFTER INSERT ON confidential_run_payloads DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard()'),
        ('confidential_run_payloads','confidential_payload_guard','["confidential_run_payloads","confidential_payload_guard","O",31,false,false,"public","ortak_confidential_payload_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_payload_guard BEFORE INSERT OR DELETE OR UPDATE ON confidential_run_payloads FOR EACH ROW EXECUTE FUNCTION ortak_confidential_payload_guard()'),
        ('confidential_run_payloads','confidential_payloads_no_truncate','["confidential_run_payloads","confidential_payloads_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_payloads_no_truncate BEFORE TRUNCATE ON confidential_run_payloads FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('confidential_runs','community_write_fence_confidential_runs','["confidential_runs","community_write_fence_confidential_runs","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_confidential_runs BEFORE INSERT OR DELETE OR UPDATE ON confidential_runs FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('confidential_runs','confidential_run_at_commit','["confidential_runs","confidential_run_at_commit","O",5,true,true,"public","ortak_confidential_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_run_at_commit AFTER INSERT ON confidential_runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard()'),
        ('confidential_runs','confidential_run_guard','["confidential_runs","confidential_run_guard","O",31,false,false,"public","ortak_confidential_run_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_run_guard BEFORE INSERT OR DELETE OR UPDATE ON confidential_runs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_guard()'),
        ('confidential_runs','confidential_runs_no_truncate','["confidential_runs","confidential_runs_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_runs_no_truncate BEFORE TRUNCATE ON confidential_runs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_memory_bindings','employee_memory_epoch_memory_identity','["employee_memory_bindings","employee_memory_epoch_memory_identity","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","6d656d6f72795f6964656e74697479006964656e746974795f6368616e67656400636f6d70616e795f696400656d706c6f7965655f6964007265766973696f6e5f6964006164617074657200656e64706f696e745f72656600776f726b737061636500757365725f7065657200656d706c6f7965655f70656572006f7074696f6e7300",11,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_memory_identity AFTER INSERT OR DELETE OR UPDATE ON employee_memory_bindings FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''memory_identity'', ''identity_changed'', ''company_id'', ''employee_id'', ''revision_id'', ''adapter'', ''endpoint_ref'', ''workspace'', ''user_peer'', ''employee_peer'', ''options'')'),
        ('employee_memory_channel_authorities','community_write_fence_employee_memory_channel_authorities','["employee_memory_channel_authorities","community_write_fence_employee_memory_channel_authorities","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_memory_channel_authorities BEFORE INSERT OR DELETE OR UPDATE ON employee_memory_channel_authorities FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_memory_channel_authorities','employee_memory_authority_guard','["employee_memory_channel_authorities","employee_memory_authority_guard","O",23,false,false,"public","ortak_employee_memory_authority_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_authority_guard BEFORE INSERT OR UPDATE ON employee_memory_channel_authorities FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_authority_guard()'),
        ('employee_memory_channel_authorities','employee_memory_no_delete','["employee_memory_channel_authorities","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_memory_channel_authorities FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_memory_channel_authorities','employee_memory_no_truncate','["employee_memory_channel_authorities","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_memory_channel_authorities FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_office_bindings','employee_memory_epoch_office_identity','["employee_office_bindings","employee_memory_epoch_office_identity","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","6f66666963655f6964656e74697479006964656e746974795f6368616e67656400636f6d70616e795f696400656d706c6f7965655f6964007075626c69635f6b6579007369676e65725f7265660076616c69645f66726f6d0076616c69645f756e74696c00",8,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_office_identity AFTER INSERT OR DELETE OR UPDATE ON employee_office_bindings FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''office_identity'', ''identity_changed'', ''company_id'', ''employee_id'', ''public_key'', ''signer_ref'', ''valid_from'', ''valid_until'')'),
        ('employee_reviewed_memory_export_commands','community_write_fence_employee_reviewed_memory_export_commands','["employee_reviewed_memory_export_commands","community_write_fence_employee_reviewed_memory_export_commands","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_export_commands BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_export_commands FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_export_commands','employee_memory_immutable','["employee_reviewed_memory_export_commands","employee_memory_immutable","O",19,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_export_commands FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_export_commands','employee_memory_no_delete','["employee_reviewed_memory_export_commands","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_commands FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_export_commands','employee_memory_no_truncate','["employee_reviewed_memory_export_commands","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_commands FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_export_commands','employee_reviewed_export_command_at_commit','["employee_reviewed_memory_export_commands","employee_reviewed_export_command_at_commit","O",5,true,true,"public","ortak_employee_reviewed_export_command_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_reviewed_export_command_at_commit AFTER INSERT ON employee_reviewed_memory_export_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_command_at_commit()'),
        ('employee_reviewed_memory_export_jobs','community_write_fence_employee_reviewed_memory_export_jobs','["employee_reviewed_memory_export_jobs","community_write_fence_employee_reviewed_memory_export_jobs","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_export_jobs BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_export_jobs','employee_memory_no_delete','["employee_reviewed_memory_export_jobs","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_export_jobs','employee_memory_no_truncate','["employee_reviewed_memory_export_jobs","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_jobs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_export_job_at_commit','["employee_reviewed_memory_export_jobs","employee_reviewed_export_job_at_commit","O",21,true,true,"public","ortak_employee_reviewed_export_job_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_reviewed_export_job_at_commit AFTER INSERT OR UPDATE ON employee_reviewed_memory_export_jobs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_at_commit()'),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_export_job_guard','["employee_reviewed_memory_export_jobs","employee_reviewed_export_job_guard","O",19,false,false,"public","ortak_employee_reviewed_export_job_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_reviewed_export_job_guard BEFORE UPDATE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_guard()'),
        ('employee_reviewed_memory_export_receipts','community_write_fence_employee_reviewed_memory_export_receipts','["employee_reviewed_memory_export_receipts","community_write_fence_employee_reviewed_memory_export_receipts","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_export_receipts BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_export_receipts FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_export_receipts','employee_memory_immutable','["employee_reviewed_memory_export_receipts","employee_memory_immutable","O",19,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_export_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_export_receipts','employee_memory_no_delete','["employee_reviewed_memory_export_receipts","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_export_receipts','employee_memory_no_truncate','["employee_reviewed_memory_export_receipts","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_export_receipt_at_commit','["employee_reviewed_memory_export_receipts","employee_reviewed_export_receipt_at_commit","O",5,true,true,"public","ortak_employee_reviewed_export_receipt_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_reviewed_export_receipt_at_commit AFTER INSERT ON employee_reviewed_memory_export_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_receipt_at_commit()'),
        ('employee_reviewed_memory_exports','community_write_fence_employee_reviewed_memory_exports','["employee_reviewed_memory_exports","community_write_fence_employee_reviewed_memory_exports","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_exports BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_exports FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_exports','employee_memory_immutable','["employee_reviewed_memory_exports","employee_memory_immutable","O",19,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_exports FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_exports','employee_memory_no_delete','["employee_reviewed_memory_exports","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_exports FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_exports','employee_memory_no_truncate','["employee_reviewed_memory_exports","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_exports FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_exports','employee_reviewed_export_at_commit','["employee_reviewed_memory_exports","employee_reviewed_export_at_commit","O",5,true,true,"public","ortak_employee_reviewed_export_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_reviewed_export_at_commit AFTER INSERT ON employee_reviewed_memory_exports DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_at_commit()'),
        ('employee_reviewed_memory_facts','community_write_fence_employee_reviewed_memory_facts','["employee_reviewed_memory_facts","community_write_fence_employee_reviewed_memory_facts","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_facts BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_facts','employee_memory_fact_at_commit','["employee_reviewed_memory_facts","employee_memory_fact_at_commit","O",21,true,true,"public","ortak_employee_memory_fact_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_memory_fact_at_commit AFTER INSERT OR UPDATE ON employee_reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_at_commit()'),
        ('employee_reviewed_memory_facts','employee_memory_fact_guard','["employee_reviewed_memory_facts","employee_memory_fact_guard","O",23,false,false,"public","ortak_employee_memory_fact_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_fact_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_guard()'),
        ('employee_reviewed_memory_facts','employee_memory_no_delete','["employee_reviewed_memory_facts","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_facts','employee_memory_no_truncate','["employee_reviewed_memory_facts","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_facts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_facts','employee_reviewed_export_stop','["employee_reviewed_memory_facts","employee_reviewed_export_stop","O",17,false,false,"public","ortak_employee_reviewed_export_stop","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_reviewed_export_stop AFTER UPDATE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_stop()'),
        ('employee_reviewed_memory_operations','community_write_fence_employee_reviewed_memory_operations','["employee_reviewed_memory_operations","community_write_fence_employee_reviewed_memory_operations","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_operations BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_operations','employee_memory_immutable','["employee_reviewed_memory_operations","employee_memory_immutable","O",19,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_operations','employee_memory_no_delete','["employee_reviewed_memory_operations","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_operations','employee_memory_no_truncate','["employee_reviewed_memory_operations","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_operations FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_operations','employee_memory_operation_at_commit','["employee_reviewed_memory_operations","employee_memory_operation_at_commit","O",5,true,true,"public","ortak_employee_memory_operation_at_commit","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_memory_operation_at_commit AFTER INSERT ON employee_reviewed_memory_operations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_operation_at_commit()'),
        ('employee_reviewed_memory_targets','community_write_fence_employee_reviewed_memory_targets','["employee_reviewed_memory_targets","community_write_fence_employee_reviewed_memory_targets","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_employee_reviewed_memory_targets BEFORE INSERT OR DELETE OR UPDATE ON employee_reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('employee_reviewed_memory_targets','employee_memory_no_delete','["employee_reviewed_memory_targets","employee_memory_no_delete","O",11,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('employee_reviewed_memory_targets','employee_memory_no_truncate','["employee_reviewed_memory_targets","employee_memory_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_targets FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('employee_reviewed_memory_targets','employee_memory_target_guard','["employee_reviewed_memory_targets","employee_memory_target_guard","O",23,false,false,"public","ortak_employee_memory_target_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_target_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_target_guard()'),
        ('employees','employee_memory_epoch_employees','["employees","employee_memory_epoch_employees","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","656d706c6f796565006964656e746974795f6368616e67656400636f6d70616e795f696400696400737461747573006163746976655f7265766973696f6e5f6964006c6966656379636c655f65706f636800",7,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_employees AFTER INSERT OR DELETE OR UPDATE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''employee'', ''identity_changed'', ''company_id'', ''id'', ''status'', ''active_revision_id'', ''lifecycle_epoch'')'),
        ('encrypted_dm_decrypt_jobs','community_write_fence_encrypted_dm_decrypt_jobs','["encrypted_dm_decrypt_jobs","community_write_fence_encrypted_dm_decrypt_jobs","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_encrypted_dm_decrypt_jobs BEFORE INSERT OR DELETE OR UPDATE ON encrypted_dm_decrypt_jobs FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('encrypted_dm_decrypt_jobs','confidential_consumed_job','["encrypted_dm_decrypt_jobs","confidential_consumed_job","O",19,false,false,"public","ortak_confidential_consumed_job","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_consumed_job BEFORE UPDATE ON encrypted_dm_decrypt_jobs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_consumed_job()'),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_no_truncate','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER encrypted_dm_decrypt_jobs_no_truncate BEFORE TRUNCATE ON encrypted_dm_decrypt_jobs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_job_current_at_commit','["encrypted_dm_decrypt_jobs","encrypted_dm_job_current_at_commit","O",21,true,true,"public","ortak_encrypted_dm_job_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER encrypted_dm_job_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_decrypt_jobs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_commit_guard()'),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_job_guard','["encrypted_dm_decrypt_jobs","encrypted_dm_job_guard","O",31,false,false,"public","ortak_encrypted_dm_job_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER encrypted_dm_job_guard BEFORE INSERT OR DELETE OR UPDATE ON encrypted_dm_decrypt_jobs FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_guard()'),
        ('encrypted_dm_selections','community_write_fence_encrypted_dm_selections','["encrypted_dm_selections","community_write_fence_encrypted_dm_selections","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_encrypted_dm_selections BEFORE INSERT OR DELETE OR UPDATE ON encrypted_dm_selections FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('encrypted_dm_selections','encrypted_dm_selection_current_at_commit','["encrypted_dm_selections","encrypted_dm_selection_current_at_commit","O",21,true,true,"public","ortak_encrypted_dm_selection_commit_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER encrypted_dm_selection_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_selections DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_commit_guard()'),
        ('encrypted_dm_selections','encrypted_dm_selection_guard','["encrypted_dm_selections","encrypted_dm_selection_guard","O",31,false,false,"public","ortak_encrypted_dm_selection_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER encrypted_dm_selection_guard BEFORE INSERT OR DELETE OR UPDATE ON encrypted_dm_selections FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_guard()'),
        ('encrypted_dm_selections','encrypted_dm_selections_no_truncate','["encrypted_dm_selections","encrypted_dm_selections_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER encrypted_dm_selections_no_truncate BEFORE TRUNCATE ON encrypted_dm_selections FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('events','employee_memory_epoch_events','["events","employee_memory_epoch_events","O",25,false,false,"public","ortak_employee_memory_epoch_mutation","6576656e7400736f757263655f6368616e67656400636f6d6d756e6974795f696400696400637265617465645f6174007075626b6579006b696e64007461677300636f6e74656e7400736967006368616e6e656c5f69640064656c657465645f617400",12,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_events AFTER DELETE OR UPDATE ON events FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''event'', ''source_changed'', ''community_id'', ''id'', ''created_at'', ''pubkey'', ''kind'', ''tags'', ''content'', ''sig'', ''channel_id'', ''deleted_at'')'),
        ('office_authority_generations','trg_routing_authority_notify','["office_authority_generations","trg_routing_authority_notify","O",21,false,false,"public","ortak_routing_notify","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER trg_routing_authority_notify AFTER INSERT OR UPDATE ON office_authority_generations FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify()'),
        ('office_company_bindings','employee_memory_epoch_company_bindings','["office_company_bindings","employee_memory_epoch_company_bindings","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","636f6d70616e795f62696e64696e670073636f70655f636c6f73656400636f6d70616e795f696400636f6d6d756e6974795f696400",4,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_company_bindings AFTER INSERT OR DELETE OR UPDATE ON office_company_bindings FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''company_binding'', ''scope_closed'', ''company_id'', ''community_id'')'),
        ('office_inbox','employee_memory_epoch_inbox','["office_inbox","employee_memory_epoch_inbox","O",25,false,false,"public","ortak_employee_memory_epoch_mutation","696e626f7800736f757263655f6368616e67656400636f6d70616e795f6964006576656e745f6964006576656e745f637265617465645f6174006576656e745f6b696e6400617574686f725f7075626b6579006368616e6e656c5f696400737461746500",9,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_inbox AFTER DELETE OR UPDATE ON office_inbox FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''inbox'', ''source_changed'', ''company_id'', ''event_id'', ''event_created_at'', ''event_kind'', ''author_pubkey'', ''channel_id'', ''state'')'),
        ('outbox','confidential_no_ordinary_outbox','["outbox","confidential_no_ordinary_outbox","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_outbox BEFORE INSERT OR UPDATE ON outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('routing_decisions','trg_routing_decisions_notify','["routing_decisions","trg_routing_decisions_notify","O",5,false,false,"public","ortak_routing_notify","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER trg_routing_decisions_notify AFTER INSERT ON routing_decisions FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify()'),
        ('run_context_snapshots','confidential_no_ordinary_snapshot','["run_context_snapshots","confidential_no_ordinary_snapshot","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_snapshot BEFORE INSERT OR UPDATE ON run_context_snapshots FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('run_employee_reviewed_memory_uses','community_write_fence_run_employee_reviewed_memory_uses','["run_employee_reviewed_memory_uses","community_write_fence_run_employee_reviewed_memory_uses","O",31,false,false,"public","enforce_community_write_fence","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER community_write_fence_run_employee_reviewed_memory_uses BEFORE INSERT OR DELETE OR UPDATE ON run_employee_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()'),
        ('run_employee_reviewed_memory_uses','employee_memory_snapshot_at_commit','["run_employee_reviewed_memory_uses","employee_memory_snapshot_at_commit","O",5,true,true,"public","ortak_reviewed_snapshot_consistent","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER employee_memory_snapshot_at_commit AFTER INSERT ON run_employee_reviewed_memory_uses DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent()'),
        ('run_employee_reviewed_memory_uses','employee_memory_use_immutable','["run_employee_reviewed_memory_uses","employee_memory_use_immutable","O",27,false,false,"public","ortak_reject_row_mutation","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_use_immutable BEFORE DELETE OR UPDATE ON run_employee_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()'),
        ('run_employee_reviewed_memory_uses','employee_memory_use_no_truncate','["run_employee_reviewed_memory_uses","employee_memory_use_no_truncate","O",34,false,false,"public","ortak_reject_office_truncate","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_use_no_truncate BEFORE TRUNCATE ON run_employee_reviewed_memory_uses FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()'),
        ('run_employee_reviewed_memory_uses','employee_memory_use_ordinary','["run_employee_reviewed_memory_uses","employee_memory_use_ordinary","O",7,false,false,"public","ortak_employee_use_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_use_ordinary BEFORE INSERT ON run_employee_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_employee_use_ordinary()'),
        ('run_events','confidential_no_ordinary_events','["run_events","confidential_no_ordinary_events","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_events BEFORE INSERT OR UPDATE ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('run_reviewed_memory_uses','confidential_no_reviewed_use','["run_reviewed_memory_uses","confidential_no_reviewed_use","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_reviewed_use BEFORE INSERT OR UPDATE ON run_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('run_workspace_uses','confidential_no_workspace_use','["run_workspace_uses","confidential_no_workspace_use","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_workspace_use BEFORE INSERT OR UPDATE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('runs','confidential_run_complete_at_commit','["runs","confidential_run_complete_at_commit","O",5,true,true,"public","ortak_confidential_run_complete_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_run_complete_at_commit AFTER INSERT ON runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_complete_guard()'),
        ('runs','confidential_run_mode_guard','["runs","confidential_run_mode_guard","O",23,false,false,"public","ortak_confidential_run_mode_guard","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_run_mode_guard BEFORE INSERT OR UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_mode_guard()'),
        ('runs','confidential_run_transition_at_commit','["runs","confidential_run_transition_at_commit","O",17,true,true,"public","ortak_confidential_run_transition_guard","",0,[],true,false,null]'::jsonb,'CREATE CONSTRAINT TRIGGER confidential_run_transition_at_commit AFTER UPDATE ON runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_transition_guard()'),
        ('runtime_memory_writes','confidential_no_ordinary_memory','["runtime_memory_writes","confidential_no_ordinary_memory","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_memory BEFORE INSERT OR UPDATE ON runtime_memory_writes FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('runtime_office_outputs','confidential_no_ordinary_office','["runtime_office_outputs","confidential_no_ordinary_office","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_office BEFORE INSERT OR UPDATE ON runtime_office_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('runtime_work_outputs','confidential_no_ordinary_work','["runtime_work_outputs","confidential_no_ordinary_work","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_ordinary_work BEFORE INSERT OR UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('thread_metadata','employee_memory_epoch_threads','["thread_metadata","employee_memory_epoch_threads","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","74687265616400736f757263655f6368616e67656400636f6d6d756e6974795f6964006576656e745f6964006576656e745f637265617465645f6174006368616e6e656c5f696400706172656e745f6576656e745f696400706172656e745f6576656e745f637265617465645f617400726f6f745f6576656e745f696400726f6f745f6576656e745f637265617465645f617400646570746800",11,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_threads AFTER INSERT OR DELETE OR UPDATE ON thread_metadata FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''thread'', ''source_changed'', ''community_id'', ''event_id'', ''event_created_at'', ''channel_id'', ''parent_event_id'', ''parent_event_created_at'', ''root_event_id'', ''root_event_created_at'', ''depth'')'),
        ('users','employee_memory_epoch_users','["users","employee_memory_epoch_users","O",29,false,false,"public","ortak_employee_memory_epoch_mutation","75736572006964656e746974795f6368616e67656400636f6d6d756e6974795f6964007075626b6579006167656e745f74797065006167656e745f6f776e65725f7075626b65790064656163746976617465645f617400",7,[],true,false,null]'::jsonb,'CREATE TRIGGER employee_memory_epoch_users AFTER INSERT OR DELETE OR UPDATE ON users FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation(''user'', ''identity_changed'', ''community_id'', ''pubkey'', ''agent_type'', ''agent_owner_pubkey'', ''deactivated_at'')'),
        ('work_attachments','confidential_no_work_attachment','["work_attachments","confidential_no_work_attachment","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_work_attachment BEFORE INSERT OR UPDATE ON work_attachments FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('work_executions','confidential_no_work_execution','["work_executions","confidential_no_work_execution","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_work_execution BEFORE INSERT OR UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('workspace_reader_executions','confidential_no_workspace_reader','["workspace_reader_executions","confidential_no_workspace_reader","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_workspace_reader BEFORE INSERT OR UPDATE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('workspace_tool_actions','confidential_no_workspace_action','["workspace_tool_actions","confidential_no_workspace_action","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_workspace_action BEFORE INSERT OR UPDATE ON workspace_tool_actions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()'),
        ('workspace_tool_receipts','confidential_no_workspace_receipt','["workspace_tool_receipts","confidential_no_workspace_receipt","O",23,false,false,"public","ortak_confidential_reject_ordinary","",0,[],true,false,null]'::jsonb,'CREATE TRIGGER confidential_no_workspace_receipt BEFORE INSERT OR UPDATE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary()')
    ) AS required(relation,name,metadata,ddl) LOOP
        SELECT jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
            t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
            ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
                JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),
            t.tgqual IS NULL,t.tgisinternal,
            CASE WHEN t.tgparentid=0 THEN NULL ELSE jsonb_build_array(parent_n.nspname,parent_c.relname,parent_t.tgname) END)
        INTO observed
        FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
        JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
        LEFT JOIN pg_trigger parent_t ON parent_t.oid=t.tgparentid
        LEFT JOIN pg_class parent_c ON parent_c.oid=parent_t.tgrelid
        LEFT JOIN pg_namespace parent_n ON parent_n.oid=parent_c.relnamespace
        WHERE n.nspname='public' AND c.relname=item.relation AND t.tgname=item.name;
        IF observed IS NULL THEN
            EXECUTE item.ddl;
        ELSIF observed IS DISTINCT FROM item.metadata THEN
            immediate_copy=jsonb_set(jsonb_set(item.metadata,'{4}','false'::jsonb),'{5}','false'::jsonb);
            IF item.metadata->4='true'::jsonb AND observed=immediate_copy THEN
                -- pgschema's exact immediate copy of a deferred constraint.
                EXECUTE format('DROP TRIGGER %I ON public.%I',item.name,item.relation);
                EXECUTE item.ddl;
            ELSE
                RAISE EXCEPTION 'unexpected77 trigger %.%',item.relation,item.name;
            END IF;
        END IF;
        SELECT jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
            t.tgdeferrable,t.tginitdeferred,pn.nspname,p.proname,encode(t.tgargs,'hex'),t.tgnargs,
            ARRAY(SELECT a.attname FROM unnest(t.tgattr::smallint[]) WITH ORDINALITY k(attnum,ord)
                JOIN pg_attribute a ON a.attrelid=t.tgrelid AND a.attnum=k.attnum ORDER BY ord),
            t.tgqual IS NULL,t.tgisinternal,
            CASE WHEN t.tgparentid=0 THEN NULL ELSE jsonb_build_array(parent_n.nspname,parent_c.relname,parent_t.tgname) END)
        INTO observed
        FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
        JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_namespace pn ON pn.oid=p.pronamespace
        LEFT JOIN pg_trigger parent_t ON parent_t.oid=t.tgparentid
        LEFT JOIN pg_class parent_c ON parent_c.oid=parent_t.tgrelid
        LEFT JOIN pg_namespace parent_n ON parent_n.oid=parent_c.relnamespace
        WHERE n.nspname='public' AND c.relname=item.relation AND t.tgname=item.name;
        IF observed IS DISTINCT FROM item.metadata THEN
            RAISE EXCEPTION '77 trigger did not converge %.%',item.relation,item.name;
        END IF;
    END LOOP;
END $reconcile77_triggers$;

-- All named CHECKs are actual77 definitions. Add only absent declarations;
-- unknown existing checks are never silently replaced or normalized.
DO $reconcile77_constraints$
DECLARE item RECORD; observed JSONB; bootstrap JSONB; definition TEXT;
BEGIN
    FOR item IN SELECT * FROM (VALUES
        ('confidential_dm_receipts','confidential_dm_receipts_company_id_run_id_fkey','["confidential_dm_receipts","confidential_dm_receipts_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_runs(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_dm_receipts','confidential_dm_receipts_company_id_source_id_fkey','["confidential_dm_receipts","confidential_dm_receipts_company_id_source_id_fkey","f","FOREIGN KEY (company_id, source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id, source_id)",true,false,false]'::jsonb),
        ('confidential_dm_receipts','confidential_dm_receipts_pkey','["confidential_dm_receipts","confidential_dm_receipts_pkey","p","PRIMARY KEY (company_id, source_id)",true,false,false]'::jsonb),
        ('confidential_dm_receipts','confidential_receipt_at_commit','["confidential_dm_receipts","confidential_receipt_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_event_receipts','confidential_event_at_commit','["confidential_event_receipts","confidential_event_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_event_receipts','confidential_event_receipts_company_id_run_id_purpose_ordi_fkey','["confidential_event_receipts","confidential_event_receipts_company_id_run_id_purpose_ordi_fkey","f","FOREIGN KEY (company_id, run_id, purpose, ordinal) REFERENCES confidential_run_payloads(company_id, run_id, purpose, ordinal)",true,false,false]'::jsonb),
        ('confidential_event_receipts','confidential_event_receipts_occurred_at_check','["confidential_event_receipts","confidential_event_receipts_occurred_at_check","c","CHECK (isfinite(occurred_at))",true,false,false]'::jsonb),
        ('confidential_event_receipts','confidential_event_receipts_ordinal_check','["confidential_event_receipts","confidential_event_receipts_ordinal_check","c","CHECK (((ordinal >= 1) AND (ordinal <= 512)))",true,false,false]'::jsonb),
        ('confidential_event_receipts','confidential_event_receipts_pkey','["confidential_event_receipts","confidential_event_receipts_pkey","p","PRIMARY KEY (company_id, run_id, ordinal)",true,false,false]'::jsonb),
        ('confidential_event_receipts','confidential_event_receipts_purpose_check','["confidential_event_receipts","confidential_event_receipts_purpose_check","c","CHECK ((purpose = ''runtime_event''::text))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_at_commit','["confidential_execution_leases","confidential_execution_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_cancel_attempts_check','["confidential_execution_leases","confidential_execution_leases_cancel_attempts_check","c","CHECK (((cancel_attempts >= 0) AND (cancel_attempts <= 3)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_check','["confidential_execution_leases","confidential_execution_leases_check","c","CHECK (((lease_token IS NULL) = (lease_expires_at IS NULL)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_check1','["confidential_execution_leases","confidential_execution_leases_check1","c","CHECK (((state = ANY (ARRAY[''observing''::text, ''sealing''::text, ''cancelling''::text])) OR (lease_token IS NULL)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_check2','["confidential_execution_leases","confidential_execution_leases_check2","c","CHECK (((state = ANY (ARRAY[''complete''::text, ''stopped''::text, ''unconfirmed''::text])) = (finished_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_check3','["confidential_execution_leases","confidential_execution_leases_check3","c","CHECK ((isfinite(next_attempt_at) AND ((lease_expires_at IS NULL) OR isfinite(lease_expires_at))))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_company_id_run_id_fkey','["confidential_execution_leases","confidential_execution_leases_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_runs(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_error_code_check','["confidential_execution_leases","confidential_execution_leases_error_code_check","c","CHECK ((error_code = ANY (ARRAY[''unavailable''::text, ''authority_changed''::text, ''protocol''::text, ''deadline_exceeded''::text, ''cancelled''::text])))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_failures_check','["confidential_execution_leases","confidential_execution_leases_failures_check","c","CHECK (((failures >= 0) AND (failures <= 3)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_generation_check','["confidential_execution_leases","confidential_execution_leases_generation_check","c","CHECK (((generation >= 0) AND (generation <= 128)))",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_pkey','["confidential_execution_leases","confidential_execution_leases_pkey","p","PRIMARY KEY (company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_execution_leases','confidential_execution_leases_state_check','["confidential_execution_leases","confidential_execution_leases_state_check","c","CHECK ((state = ANY (ARRAY[''observing''::text, ''sealing''::text, ''cancelling''::text, ''complete''::text, ''stopped''::text, ''unconfirmed''::text])))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_at_commit','["confidential_reply_bundles","confidential_reply_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_check','["confidential_reply_bundles","confidential_reply_bundles_check","c","CHECK ((recipient_id <> history_id))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_company_id_history_id_key','["confidential_reply_bundles","confidential_reply_bundles_company_id_history_id_key","u","UNIQUE (company_id, history_id)",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_company_id_recipient_id_key','["confidential_reply_bundles","confidential_reply_bundles_company_id_recipient_id_key","u","UNIQUE (company_id, recipient_id)",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_company_id_run_id_fkey','["confidential_reply_bundles","confidential_reply_bundles_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_runs(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_history_bytes_check','["confidential_reply_bundles","confidential_reply_bundles_history_bytes_check","c","CHECK (((octet_length(history_bytes) >= 1) AND (octet_length(history_bytes) <= 65536)))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_history_id_check','["confidential_reply_bundles","confidential_reply_bundles_history_id_check","c","CHECK ((octet_length(history_id) = 32))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_pkey','["confidential_reply_bundles","confidential_reply_bundles_pkey","p","PRIMARY KEY (company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_recipient_bytes_check','["confidential_reply_bundles","confidential_reply_bundles_recipient_bytes_check","c","CHECK (((octet_length(recipient_bytes) >= 1) AND (octet_length(recipient_bytes) <= 65536)))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_recipient_id_check','["confidential_reply_bundles","confidential_reply_bundles_recipient_id_check","c","CHECK ((octet_length(recipient_id) = 32))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_rumor_hash_check','["confidential_reply_bundles","confidential_reply_bundles_rumor_hash_check","c","CHECK ((octet_length(rumor_hash) = 32))",true,false,false]'::jsonb),
        ('confidential_reply_bundles','confidential_reply_bundles_rumor_id_check','["confidential_reply_bundles","confidential_reply_bundles_rumor_id_check","c","CHECK ((octet_length(rumor_id) = 32))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_outbox_at_commit','["confidential_reply_outbox","confidential_outbox_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_attempts_check','["confidential_reply_outbox","confidential_reply_outbox_attempts_check","c","CHECK (((attempts >= 0) AND (attempts <= 3)))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check','["confidential_reply_outbox","confidential_reply_outbox_check","c","CHECK ((generation = attempts))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check1','["confidential_reply_outbox","confidential_reply_outbox_check1","c","CHECK (((lease_token IS NULL) = (lease_expires_at IS NULL)))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check2','["confidential_reply_outbox","confidential_reply_outbox_check2","c","CHECK (((state = ''pending''::text) OR (lease_token IS NULL)))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check3','["confidential_reply_outbox","confidential_reply_outbox_check3","c","CHECK (((state <> ''pending''::text) = (finished_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check4','["confidential_reply_outbox","confidential_reply_outbox_check4","c","CHECK (((state = ''acked''::text) = (acknowledged_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_check5','["confidential_reply_outbox","confidential_reply_outbox_check5","c","CHECK ((isfinite(next_attempt_at) AND ((lease_expires_at IS NULL) OR isfinite(lease_expires_at))))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_company_id_run_id_fkey','["confidential_reply_outbox","confidential_reply_outbox_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_reply_bundles(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_copy_check','["confidential_reply_outbox","confidential_reply_outbox_copy_check","c","CHECK ((copy = ANY (ARRAY[0, 1])))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_error_code_check','["confidential_reply_outbox","confidential_reply_outbox_error_code_check","c","CHECK ((error_code = ANY (ARRAY[''unavailable''::text, ''authority_changed''::text, ''deadline_exceeded''::text])))",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_pkey','["confidential_reply_outbox","confidential_reply_outbox_pkey","p","PRIMARY KEY (company_id, run_id, copy)",true,false,false]'::jsonb),
        ('confidential_reply_outbox','confidential_reply_outbox_state_check','["confidential_reply_outbox","confidential_reply_outbox_state_check","c","CHECK ((state = ANY (ARRAY[''pending''::text, ''acked''::text, ''failed''::text, ''retired''::text])))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_dispatch_at_commit','["confidential_run_dispatches","confidential_dispatch_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_attempts_check','["confidential_run_dispatches","confidential_run_dispatches_attempts_check","c","CHECK (((attempts >= 0) AND (attempts <= 3)))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_check','["confidential_run_dispatches","confidential_run_dispatches_check","c","CHECK ((generation = attempts))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_check1','["confidential_run_dispatches","confidential_run_dispatches_check1","c","CHECK (((lease_token IS NULL) = (lease_expires_at IS NULL)))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_check2','["confidential_run_dispatches","confidential_run_dispatches_check2","c","CHECK (((state <> ''pending''::text) = (finished_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_check3','["confidential_run_dispatches","confidential_run_dispatches_check3","c","CHECK (((state = ''pending''::text) OR (lease_token IS NULL)))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_check4','["confidential_run_dispatches","confidential_run_dispatches_check4","c","CHECK ((isfinite(next_attempt_at) AND ((lease_expires_at IS NULL) OR isfinite(lease_expires_at))))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_company_id_run_id_fkey','["confidential_run_dispatches","confidential_run_dispatches_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_runs(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_error_code_check','["confidential_run_dispatches","confidential_run_dispatches_error_code_check","c","CHECK ((error_code = ANY (ARRAY[''unavailable''::text, ''authority_changed''::text, ''deadline_exceeded''::text, ''cancelled''::text])))",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_pkey','["confidential_run_dispatches","confidential_run_dispatches_pkey","p","PRIMARY KEY (company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_run_dispatches','confidential_run_dispatches_state_check','["confidential_run_dispatches","confidential_run_dispatches_state_check","c","CHECK ((state = ANY (ARRAY[''pending''::text, ''delivered''::text, ''failed''::text, ''cancelled''::text])))",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_event_payload_at_commit','["confidential_run_payloads","confidential_event_payload_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_run_payloads','confidential_payload_at_commit','["confidential_run_payloads","confidential_payload_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_check','["confidential_run_payloads","confidential_run_payloads_check","c","CHECK ((((purpose = ANY (ARRAY[''snapshot''::text, ''reply_draft''::text])) AND (ordinal = 0)) OR ((purpose = ''runtime_event''::text) AND ((ordinal >= 1) AND (ordinal <= 512)))))",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_company_id_run_id_fkey','["confidential_run_payloads","confidential_run_payloads_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES confidential_runs(company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_company_id_run_id_purpose_nonce_key','["confidential_run_payloads","confidential_run_payloads_company_id_run_id_purpose_nonce_key","u","UNIQUE (company_id, run_id, purpose, nonce)",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_envelope_bytes_check','["confidential_run_payloads","confidential_run_payloads_envelope_bytes_check","c","CHECK (((octet_length(envelope_bytes) >= 1) AND (octet_length(envelope_bytes) <= 98304)))",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_nonce_check','["confidential_run_payloads","confidential_run_payloads_nonce_check","c","CHECK ((octet_length(nonce) = 12))",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_pkey','["confidential_run_payloads","confidential_run_payloads_pkey","p","PRIMARY KEY (company_id, run_id, purpose, ordinal)",true,false,false]'::jsonb),
        ('confidential_run_payloads','confidential_run_payloads_purpose_check','["confidential_run_payloads","confidential_run_payloads_purpose_check","c","CHECK ((purpose = ANY (ARRAY[''snapshot''::text, ''runtime_event''::text, ''reply_draft''::text])))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_run_at_commit','["confidential_runs","confidential_run_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('confidential_runs','confidential_runs_check','["confidential_runs","confidential_runs_check","c","CHECK ((start_key = (((''ortak-run:''::text || (company_id)::text) || '':''::text) || (run_id)::text)))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_check1','["confidential_runs","confidential_runs_check1","c","CHECK ((isfinite(admitted_at) AND isfinite(admission_deadline) AND isfinite(execution_deadline) AND (admission_deadline > admitted_at) AND (execution_deadline > admitted_at) AND (execution_deadline <= (admitted_at + ''00:10:00''::interval))))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_claim_generation_check','["confidential_runs","confidential_runs_claim_generation_check","c","CHECK (((claim_generation >= 1) AND (claim_generation <= 3)))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_employee_id_human_public_key_r_key','["confidential_runs","confidential_runs_company_id_employee_id_human_public_key_r_key","u","UNIQUE (company_id, employee_id, human_public_key, rumor_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_fkey','["confidential_runs","confidential_runs_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_key_id_key','["confidential_runs","confidential_runs_company_id_key_id_key","u","UNIQUE (company_id, key_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_run_id_fkey','["confidential_runs","confidential_runs_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_selection_id_fkey','["confidential_runs","confidential_runs_company_id_selection_id_fkey","f","FOREIGN KEY (company_id, selection_id) REFERENCES encrypted_dm_selections(company_id, selection_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_source_id_fkey','["confidential_runs","confidential_runs_company_id_source_id_fkey","f","FOREIGN KEY (company_id, source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id, source_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_company_id_source_id_key','["confidential_runs","confidential_runs_company_id_source_id_key","u","UNIQUE (company_id, source_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_human_public_key_check','["confidential_runs","confidential_runs_human_public_key_check","c","CHECK ((octet_length(human_public_key) = 32))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_identity_bytes_check','["confidential_runs","confidential_runs_identity_bytes_check","c","CHECK (((octet_length(identity_bytes) >= 1) AND (octet_length(identity_bytes) <= 2048)))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_pkey','["confidential_runs","confidential_runs_pkey","p","PRIMARY KEY (company_id, run_id)",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_rumor_id_check','["confidential_runs","confidential_runs_rumor_id_check","c","CHECK ((octet_length(rumor_id) = 32))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_source_bytes_check','["confidential_runs","confidential_runs_source_bytes_check","c","CHECK (((octet_length(source_bytes) >= 1) AND (octet_length(source_bytes) <= 4096)))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_source_id_check','["confidential_runs","confidential_runs_source_id_check","c","CHECK ((octet_length(source_id) = 32))",true,false,false]'::jsonb),
        ('confidential_runs','confidential_runs_wrapped_key_check','["confidential_runs","confidential_runs_wrapped_key_check","c","CHECK (((octet_length(wrapped_key) >= 1) AND (octet_length(wrapped_key) <= 12288)))",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_channel_id_check','["employee_memory_channel_authorities","employee_memory_channel_authorities_channel_id_check","c","CHECK ((channel_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_check','["employee_memory_channel_authorities","employee_memory_channel_authorities_check","c","CHECK (((company_id <> ''00000000-0000-0000-0000-000000000000''::uuid) AND (community_id <> ''00000000-0000-0000-0000-000000000000''::uuid)))",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_check1','["employee_memory_channel_authorities","employee_memory_channel_authorities_check1","c","CHECK ((changed_at >= created_at))",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_community_id_fkey','["employee_memory_channel_authorities","employee_memory_channel_authorities_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_company_id_employee_id_fkey','["employee_memory_channel_authorities","employee_memory_channel_authorities_company_id_employee_id_fkey","f","FOREIGN KEY (company_id, employee_id) REFERENCES employees(company_id, id)",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_company_id_fkey','["employee_memory_channel_authorities","employee_memory_channel_authorities_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_epoch_check','["employee_memory_channel_authorities","employee_memory_channel_authorities_epoch_check","c","CHECK ((epoch >= 0))",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_pkey','["employee_memory_channel_authorities","employee_memory_channel_authorities_pkey","p","PRIMARY KEY (company_id, community_id, employee_id, channel_id)",true,false,false]'::jsonb),
        ('employee_memory_channel_authorities','employee_memory_channel_authorities_reason_check','["employee_memory_channel_authorities","employee_memory_channel_authorities_reason_check","c","CHECK ((reason = ANY (ARRAY[''registered''::text, ''source_changed''::text, ''audience_changed''::text, ''identity_changed''::text, ''scope_closed''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_export_command_at_commit','["employee_reviewed_memory_export_commands","employee_reviewed_export_command_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_expo_company_id_fact_id_action_res_key','["employee_reviewed_memory_export_commands","employee_reviewed_memory_expo_company_id_fact_id_action_res_key","u","UNIQUE (company_id, fact_id, action, result_version)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_command_company_id_fact_id_fkey','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_command_company_id_fact_id_fkey","f","FOREIGN KEY (company_id, fact_id) REFERENCES employee_reviewed_memory_exports(company_id, fact_id) DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_action_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_action_check","c","CHECK ((action = ANY (ARRAY[''publish''::text, ''retry_publish''::text, ''retry_withdraw''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_actor_pubkey_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_actor_pubkey_check","c","CHECK ((actor_pubkey ~ ''^[0-9a-f]{64}$''::text))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_auth_event_id_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_auth_event_id_check","c","CHECK ((octet_length(auth_event_id) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_check","c","CHECK ((((action = ''publish''::text) AND (result_version = 0)) OR ((action = ANY (ARRAY[''retry_publish''::text, ''retry_withdraw''::text])) AND ((result_version >= 1) AND (result_version <= 8)))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_community_id_fkey','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_operation_id_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_operation_id_check","c","CHECK ((operation_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_pkey','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_pkey","p","PRIMARY KEY (company_id, actor_pubkey, operation_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_request_hash_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_request_hash_check","c","CHECK ((octet_length(request_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_commands','employee_reviewed_memory_export_commands_valid_before_check','["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_valid_before_check","c","CHECK ((ortak_employee_memory_timestamp(valid_before) IS NOT NULL))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_export_job_at_commit','["employee_reviewed_memory_export_jobs","employee_reviewed_export_job_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export__company_id_idempotency_key_key','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export__company_id_idempotency_key_key","u","UNIQUE (company_id, idempotency_key)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_action_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_action_check","c","CHECK ((action = ANY (ARRAY[''publish''::text, ''withdraw''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_attempt_count_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_attempt_count_check","c","CHECK (((attempt_count >= 0) AND (attempt_count <= 20)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_check","c","CHECK (((lease_token IS NULL) = (lease_expires_at IS NULL)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_check1','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_check1","c","CHECK (((total_attempts >= attempt_count) AND (total_attempts <= (20 * (retry_version + 1)))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_check2','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_check2","c","CHECK (((state <> ''failed''::text) OR (lease_token IS NULL)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_community_id_fkey','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_company_id_fact_id_fkey','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_company_id_fact_id_fkey","f","FOREIGN KEY (company_id, fact_id) REFERENCES employee_reviewed_memory_exports(company_id, fact_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_idempotency_key_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_idempotency_key_check","c","CHECK ((idempotency_key ~ ''^[a-z0-9:-]{1,200}$''::text))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_last_error_code_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_last_error_code_check","c","CHECK ((last_error_code = ANY (ARRAY[''authority_refused''::text, ''target_unavailable''::text, ''service_retry''::text, ''service_refused''::text, ''database_retry''::text, ''deadline''::text, ''lease_exhausted''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_pkey','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_pkey","p","PRIMARY KEY (company_id, fact_id, action)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_request_hash_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_request_hash_check","c","CHECK ((octet_length(request_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_retry_version_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_retry_version_check","c","CHECK (((retry_version >= 0) AND (retry_version <= 8)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_state_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_state_check","c","CHECK ((state = ANY (ARRAY[''pending''::text, ''acknowledged''::text, ''failed''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_jobs_total_attempts_check','["employee_reviewed_memory_export_jobs","employee_reviewed_memory_export_jobs_total_attempts_check","c","CHECK (((total_attempts >= 0) AND (total_attempts <= 180)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_export_receipt_at_commit','["employee_reviewed_memory_export_receipts","employee_reviewed_export_receipt_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export__company_id_fact_id_action_fkey','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export__company_id_fact_id_action_fkey","f","FOREIGN KEY (company_id, fact_id, action) REFERENCES employee_reviewed_memory_export_jobs(company_id, fact_id, action)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_action_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_action_check","c","CHECK ((action = ANY (ARRAY[''publish''::text, ''withdraw''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_binding_hash_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_binding_hash_check","c","CHECK ((octet_length(binding_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_check","c","CHECK ((erased_from_reviewed_store = (tombstone_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_check2','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_check2","c","CHECK (((action <> ''withdraw''::text) OR (erased_from_reviewed_store AND (remote_status <> ''active''::text))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_community_id_fkey','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_content_hash_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_content_hash_check","c","CHECK ((octet_length(content_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_pkey','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_pkey","p","PRIMARY KEY (company_id, fact_id, action)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_remote_status_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_remote_status_check","c","CHECK ((remote_status = ANY (ARRAY[''active''::text, ''expired''::text, ''withdrawn''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_request_hash_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_request_hash_check","c","CHECK ((octet_length(request_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_memory_export_receipts_total_attempts_check','["employee_reviewed_memory_export_receipts","employee_reviewed_memory_export_receipts_total_attempts_check","c","CHECK (((total_attempts >= 1) AND (total_attempts <= 180)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_export_receipts','employee_reviewed_receipt_erasure_state','["employee_reviewed_memory_export_receipts","employee_reviewed_receipt_erasure_state","c","CHECK (((remote_status = ''withdrawn''::text) = erased_from_reviewed_store))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_export_at_commit','["employee_reviewed_memory_exports","employee_reviewed_export_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_export_instruction','["employee_reviewed_memory_exports","employee_reviewed_export_instruction","f","FOREIGN KEY (company_id, requested_by, operation_id) REFERENCES employee_reviewed_memory_export_commands(company_id, actor_pubkey, operation_id) DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_expo_company_id_community_id_empl_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_expo_company_id_community_id_empl_fkey","f","FOREIGN KEY (company_id, community_id, employee_id, destination_channel_id) REFERENCES employee_memory_channel_authorities(company_id, community_id, employee_id, channel_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_expo_company_id_employee_id_emplo_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_expo_company_id_employee_id_emplo_fkey","f","FOREIGN KEY (company_id, employee_id, employee_revision_id) REFERENCES employee_revisions(company_id, employee_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_community_id_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_company_id_fact_id_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_company_id_fact_id_fkey","f","FOREIGN KEY (company_id, fact_id) REFERENCES employee_reviewed_memory_facts(company_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_company_id_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_company_id_target_id_fkey','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_company_id_target_id_fkey","f","FOREIGN KEY (company_id, target_id) REFERENCES employee_reviewed_memory_targets(company_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_content_hash_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_content_hash_check","c","CHECK ((octet_length(content_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_employee_lifecycle_epoch_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_employee_lifecycle_epoch_check","c","CHECK ((employee_lifecycle_epoch >= 0))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_operation_id_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_operation_id_check","c","CHECK ((operation_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_pkey','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_pkey","p","PRIMARY KEY (company_id, fact_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_requested_by_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_requested_by_check","c","CHECK ((requested_by ~ ''^[0-9a-f]{64}$''::text))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_sharing_hash_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_sharing_hash_check","c","CHECK ((octet_length(sharing_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_exports','employee_reviewed_memory_exports_source_hash_check','["employee_reviewed_memory_exports","employee_reviewed_memory_exports_source_hash_check","c","CHECK ((octet_length(source_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_memory_fact_at_commit','["employee_reviewed_memory_facts","employee_memory_fact_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_facts','employee_memory_original_approval','["employee_reviewed_memory_facts","employee_memory_original_approval","f","FOREIGN KEY (company_id, approved_by, approval_id) REFERENCES employee_reviewed_memory_operations(company_id, actor_public_key, operation_id) DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_fac_company_id_community_id_empl_fkey1','["employee_reviewed_memory_facts","employee_reviewed_memory_fac_company_id_community_id_empl_fkey1","f","FOREIGN KEY (company_id, community_id, employee_id, destination_channel_id) REFERENCES employee_memory_channel_authorities(company_id, community_id, employee_id, channel_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_fact_company_id_approved_by_approv_key','["employee_reviewed_memory_facts","employee_reviewed_memory_fact_company_id_approved_by_approv_key","u","UNIQUE (company_id, approved_by, approval_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_fact_company_id_community_id_empl_fkey','["employee_reviewed_memory_facts","employee_reviewed_memory_fact_company_id_community_id_empl_fkey","f","FOREIGN KEY (company_id, community_id, employee_id, source_channel_id) REFERENCES employee_memory_channel_authorities(company_id, community_id, employee_id, channel_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_approval_id_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_approval_id_check","c","CHECK ((approval_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_audience_bytes_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_audience_bytes_check","c","CHECK (((octet_length(audience_bytes) >= 1) AND (octet_length(audience_bytes) <= 2048)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check","c","CHECK (((octet_length(audience_hash) = 32) AND (audience_hash = sha256(audience_bytes))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check1','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check1","c","CHECK (((octet_length(sharing_hash) = 32) AND (sharing_hash = sha256(provenance_bytes))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check2','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check2","c","CHECK ((content_hash = sha256(convert_to(content, ''UTF8''::name))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check3','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check3","c","CHECK (((octet_length(approved_by) = 32) AND (approved_by = source_author_public_key)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check4','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check4","c","CHECK ((((kind = ''experience''::text) AND (human_public_key IS NULL)) OR ((kind = ''relationship''::text) AND (human_public_key IS NOT NULL) AND (human_public_key = approved_by))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check5','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check5","c","CHECK (((ortak_employee_memory_timestamp(approved_at) IS NOT NULL) AND (ortak_employee_memory_timestamp(expires_at) IS NOT NULL)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check6','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check6","c","CHECK (((expires_at > approved_at) AND (expires_at <= (approved_at + ''2160:00:00''::interval))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_check7','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_check7","c","CHECK ((((version = 1) AND (revoked_at IS NULL) AND (revoked_by IS NULL)) OR ((version = 2) AND (revoked_at IS NOT NULL) AND (revoked_by IS NOT NULL) AND (revoked_by = approved_by) AND (revoked_at >= approved_at))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_community_id_fkey','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_company_id_community_id_id_key','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_company_id_community_id_id_key","u","UNIQUE (company_id, community_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_company_id_employee_id_fkey','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_company_id_employee_id_fkey","f","FOREIGN KEY (company_id, employee_id) REFERENCES employees(company_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_company_id_fkey','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_content_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_content_check","c","CHECK ((((octet_length(content) >= 1) AND (octet_length(content) <= 4096)) AND (btrim(content) <> ''''::text)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_human_public_key_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_human_public_key_check","c","CHECK ((octet_length(human_public_key) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_id_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_id_check","c","CHECK ((id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_kind_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_kind_check","c","CHECK ((kind = ANY (ARRAY[''experience''::text, ''relationship''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_pkey','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_pkey","p","PRIMARY KEY (company_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_provenance_bytes_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_provenance_bytes_check","c","CHECK (((octet_length(provenance_bytes) >= 1) AND (octet_length(provenance_bytes) <= 4096)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_revoked_by_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_revoked_by_check","c","CHECK ((octet_length(revoked_by) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_source_author_public_key_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_source_author_public_key_check","c","CHECK ((octet_length(source_author_public_key) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_source_event_created_at_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_source_event_created_at_check","c","CHECK ((ortak_employee_memory_timestamp(source_event_created_at) IS NOT NULL))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_source_event_id_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_source_event_id_check","c","CHECK ((octet_length(source_event_id) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_source_evidence_hash_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_source_evidence_hash_check","c","CHECK ((octet_length(source_evidence_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_source_hash_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_source_hash_check","c","CHECK ((octet_length(source_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_facts','employee_reviewed_memory_facts_version_check','["employee_reviewed_memory_facts","employee_reviewed_memory_facts_version_check","c","CHECK ((version = ANY (ARRAY[1, 2])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_memory_operation_at_commit','["employee_reviewed_memory_operations","employee_memory_operation_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_oper_company_id_community_id_fact_fkey','["employee_reviewed_memory_operations","employee_reviewed_memory_oper_company_id_community_id_fact_fkey","f","FOREIGN KEY (company_id, community_id, fact_id) REFERENCES employee_reviewed_memory_facts(company_id, community_id, id) DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operatio_company_id_fact_id_action_key','["employee_reviewed_memory_operations","employee_reviewed_memory_operatio_company_id_fact_id_action_key","u","UNIQUE (company_id, fact_id, action)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_action_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_action_check","c","CHECK ((action = ANY (ARRAY[''approve''::text, ''stop''::text])))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_actor_public_key_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_actor_public_key_check","c","CHECK ((octet_length(actor_public_key) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_auth_event_id_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_auth_event_id_check","c","CHECK ((octet_length(auth_event_id) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_check","c","CHECK ((submitted_hash = sha256(submitted_bytes)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_check1','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_check1","c","CHECK ((((action = ''approve''::text) AND (result_version = 1)) OR ((action = ''stop''::text) AND (result_version = 2))))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_community_id_fkey','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_operation_id_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_operation_id_check","c","CHECK ((operation_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_pkey','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_pkey","p","PRIMARY KEY (company_id, actor_public_key, operation_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_submitted_bytes_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_submitted_bytes_check","c","CHECK (((octet_length(submitted_bytes) >= 1) AND (octet_length(submitted_bytes) <= 32768)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_operations','employee_reviewed_memory_operations_valid_before_check','["employee_reviewed_memory_operations","employee_reviewed_memory_operations_valid_before_check","c","CHECK ((ortak_employee_memory_timestamp(valid_before) IS NOT NULL))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targ_company_id_community_id_empl_fkey','["employee_reviewed_memory_targets","employee_reviewed_memory_targ_company_id_community_id_empl_fkey","f","FOREIGN KEY (company_id, community_id, employee_id, destination_channel_id) REFERENCES employee_memory_channel_authorities(company_id, community_id, employee_id, channel_id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targ_company_id_destination_channe_key','["employee_reviewed_memory_targets","employee_reviewed_memory_targ_company_id_destination_channe_key","u","UNIQUE (company_id, destination_channel_id, employee_id, deployment_id, binding_hash)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targ_company_id_employee_id_emplo_fkey','["employee_reviewed_memory_targets","employee_reviewed_memory_targ_company_id_employee_id_emplo_fkey","f","FOREIGN KEY (company_id, employee_id, employee_revision_id) REFERENCES employee_revisions(company_id, employee_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_binding_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_binding_check","c","CHECK (((jsonb_typeof(binding) = ''object''::text) AND (octet_length((binding)::text) <= 8192)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_binding_hash_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_binding_hash_check","c","CHECK ((octet_length(binding_hash) = 32))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_check","c","CHECK ((namespace_hash = sha256(namespace_bytes)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_check1','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_check1","c","CHECK (COALESCE((((creation_receipt ->> ''company_id''::text) = (company_id)::text) AND ((creation_receipt ->> ''employee_id''::text) = employee_id) AND ((creation_receipt ->> ''deployment_id''::text) = (deployment_id)::text) AND ((creation_receipt -> ''binding''::text) = binding) AND ((creation_receipt ->> ''protocol''::text) = protocol) AND ((creation_receipt ->> ''namespace_hash''::text) = encode(namespace_hash, ''hex''::text)) AND ((creation_receipt ->> ''request_hash''::text) ~ ''^[0-9a-f]{64}$''::text) AND (jsonb_typeof((creation_receipt -> ''native_ids''::text)) = ''object''::text)), false))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_community_id_fkey','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_company_id_fkey','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_consumption_epoch_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_consumption_epoch_check","c","CHECK ((consumption_epoch >= 0))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_creation_receipt_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_creation_receipt_check","c","CHECK (((jsonb_typeof(creation_receipt) = ''object''::text) AND (octet_length((creation_receipt)::text) <= 16384)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_deployment_id_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_deployment_id_check","c","CHECK ((deployment_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_employee_lifecycle_epoch_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_employee_lifecycle_epoch_check","c","CHECK ((employee_lifecycle_epoch >= 0))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_id_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_id_check","c","CHECK ((id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_namespace_bytes_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_namespace_bytes_check","c","CHECK (((octet_length(namespace_bytes) >= 1) AND (octet_length(namespace_bytes) <= 2048)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_pkey','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_pkey","p","PRIMARY KEY (company_id, id)",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_protocol_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_protocol_check","c","CHECK ((protocol = ''reviewed-employee/1''::text))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_registration_receipt_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_registration_receipt_check","c","CHECK (((jsonb_typeof(registration_receipt) = ''object''::text) AND (octet_length((registration_receipt)::text) <= 4096)))",true,false,false]'::jsonb),
        ('employee_reviewed_memory_targets','employee_reviewed_memory_targets_valid_until_check','["employee_reviewed_memory_targets","employee_reviewed_memory_targets_valid_until_check","c","CHECK ((ortak_employee_memory_timestamp(valid_until) IS NOT NULL))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_attempts_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_attempts_check","c","CHECK (((attempts >= 0) AND (attempts <= 3)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check","c","CHECK ((isfinite(deadline) AND isfinite(valid_before) AND (deadline > source_received_at) AND (deadline <= (source_received_at + ''00:02:00''::interval)) AND (valid_before <= deadline)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check1','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check1","c","CHECK ((isfinite(next_attempt_at) AND (next_attempt_at <= (deadline + ''00:00:05''::interval))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check10','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check10","c","CHECK (((state <> ''verified''::text) OR (verified_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check2','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check2","c","CHECK ((claim_generation = attempts))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check3','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check3","c","CHECK (((state = ANY (ARRAY[''claimed''::text, ''verified''::text])) = (claim_token IS NOT NULL)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check4','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check4","c","CHECK ((((claim_token IS NULL) = (worker_id IS NULL)) AND ((claim_token IS NULL) = (claimed_at IS NULL)) AND ((claim_token IS NULL) = (claim_expires_at IS NULL)) AND ((claim_token IS NULL) = (crypto_deadline IS NULL))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check5','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check5","c","CHECK (((claim_token IS NULL) OR ((claim_token <> ''00000000-0000-0000-0000-000000000000''::uuid) AND (worker_id <> ''00000000-0000-0000-0000-000000000000''::uuid))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check6','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check6","c","CHECK (((claim_token IS NULL) OR ((claimed_at < crypto_deadline) AND (crypto_deadline <= (claimed_at + ''00:00:05''::interval)) AND (crypto_deadline <= claim_expires_at) AND (claim_expires_at <= (claimed_at + ''00:00:30''::interval)) AND (claim_expires_at <= valid_before))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check7','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check7","c","CHECK (((state = ANY (ARRAY[''failed''::text, ''cancelled''::text])) = (terminal_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check8','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check8","c","CHECK ((((verified_at IS NULL) = (rumor_id IS NULL)) AND ((verified_at IS NULL) = (seal_id IS NULL)) AND ((verified_at IS NULL) = (seal_created_at IS NULL)) AND ((verified_at IS NULL) = (rumor_created_at IS NULL)) AND ((verified_at IS NULL) = (rumor_hash IS NULL))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_check9','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_check9","c","CHECK (((verified_at IS NOT NULL) OR (reply_to IS NULL)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_claim_generation_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_claim_generation_check","c","CHECK (((claim_generation >= 0) AND (claim_generation <= 3)))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_company_id_fkey','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_company_id_selection_id_fkey','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_company_id_selection_id_fkey","f","FOREIGN KEY (company_id, selection_id) REFERENCES encrypted_dm_selections(company_id, selection_id)",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_employee_lifecycle_epoch_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_employee_lifecycle_epoch_check","c","CHECK ((employee_lifecycle_epoch >= 0))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_error_code_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_error_code_check","c","CHECK ((error_code = ANY (ARRAY[''material_unavailable''::text, ''crypto_invalid''::text, ''authority_changed''::text, ''source_unavailable''::text, ''deadline_exceeded''::text, ''attempts_exhausted''::text, ''cancelled''::text])))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_office_generation_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_office_generation_check","c","CHECK ((office_generation >= 0))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_pkey','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_pkey","p","PRIMARY KEY (company_id, source_id)",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_reply_to_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_reply_to_check","c","CHECK ((octet_length(reply_to) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_rumor_created_at_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_rumor_created_at_check","c","CHECK (((rumor_created_at IS NULL) OR ((rumor_created_at >= ''1970-01-01 00:00:00+00''::timestamp with time zone) AND (rumor_created_at < ''10000-01-01 00:00:00+00''::timestamp with time zone) AND (date_trunc(''second''::text, rumor_created_at) = rumor_created_at))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_rumor_hash_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_rumor_hash_check","c","CHECK ((octet_length(rumor_hash) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_rumor_id_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_rumor_id_check","c","CHECK ((octet_length(rumor_id) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_seal_created_at_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_seal_created_at_check","c","CHECK (((seal_created_at IS NULL) OR ((seal_created_at >= ''1970-01-01 00:00:00+00''::timestamp with time zone) AND (seal_created_at < ''10000-01-01 00:00:00+00''::timestamp with time zone) AND (date_trunc(''second''::text, seal_created_at) = seal_created_at))))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_seal_id_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_seal_id_check","c","CHECK ((octet_length(seal_id) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_selection_generation_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_selection_generation_check","c","CHECK ((selection_generation > 0))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_source_author_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_source_author_check","c","CHECK ((octet_length(source_author) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_source_hash_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_source_hash_check","c","CHECK ((octet_length(source_hash) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_source_id_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_source_id_check","c","CHECK ((octet_length(source_id) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_decrypt_jobs_state_check','["encrypted_dm_decrypt_jobs","encrypted_dm_decrypt_jobs_state_check","c","CHECK ((state = ANY (ARRAY[''pending''::text, ''claimed''::text, ''verified''::text, ''failed''::text, ''cancelled''::text])))",true,false,false]'::jsonb),
        ('encrypted_dm_decrypt_jobs','encrypted_dm_job_current_at_commit','["encrypted_dm_decrypt_jobs","encrypted_dm_job_current_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selection_current_at_commit','["encrypted_dm_selections","encrypted_dm_selection_current_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_check','["encrypted_dm_selections","encrypted_dm_selections_check","c","CHECK ((human_public_key <> employee_public_key))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_check1','["encrypted_dm_selections","encrypted_dm_selections_check1","c","CHECK (((NOT enabled) OR (enabled_at IS NOT NULL)))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_company_id_fkey','["encrypted_dm_selections","encrypted_dm_selections_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_decrypt_ref_check','["encrypted_dm_selections","encrypted_dm_selections_decrypt_ref_check","c","CHECK (ortak_is_credential_ref(decrypt_ref))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_employee_public_key_check','["encrypted_dm_selections","encrypted_dm_selections_employee_public_key_check","c","CHECK ((octet_length(employee_public_key) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_generation_check','["encrypted_dm_selections","encrypted_dm_selections_generation_check","c","CHECK ((generation > 0))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_human_public_key_check','["encrypted_dm_selections","encrypted_dm_selections_human_public_key_check","c","CHECK ((octet_length(human_public_key) = 32))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_key_version_check','["encrypted_dm_selections","encrypted_dm_selections_key_version_check","c","CHECK ((key_version >= 0))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_pkey','["encrypted_dm_selections","encrypted_dm_selections_pkey","p","PRIMARY KEY (company_id, selection_id)",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_purpose_check','["encrypted_dm_selections","encrypted_dm_selections_purpose_check","c","CHECK ((purpose = ''dm_decrypt''::text))",true,false,false]'::jsonb),
        ('encrypted_dm_selections','encrypted_dm_selections_selection_id_check','["encrypted_dm_selections","encrypted_dm_selections_selection_id_check","c","CHECK ((selection_id <> ''00000000-0000-0000-0000-000000000000''::uuid))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','employee_memory_snapshot_at_commit','["run_employee_reviewed_memory_uses","employee_memory_snapshot_at_commit","t","TRIGGER DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory__destination_authority_epoch_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory__destination_authority_epoch_check","c","CHECK ((destination_authority_epoch >= 0))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_approved_by_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_approved_by_check","c","CHECK ((approved_by ~ ''^[0-9a-f]{64}$''::text))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_audience_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_audience_hash_check","c","CHECK ((octet_length(audience_hash) = 32))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_binding_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_binding_hash_check","c","CHECK ((octet_length(binding_hash) = 32))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_community_id_fkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_community_id_fkey","f","FOREIGN KEY (community_id) REFERENCES communities(id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_fact_id_fkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_fact_id_fkey","f","FOREIGN KEY (company_id, fact_id) REFERENCES employee_reviewed_memory_facts(company_id, id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_fact_id_fkey1','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_fact_id_fkey1","f","FOREIGN KEY (company_id, fact_id) REFERENCES employee_reviewed_memory_exports(company_id, fact_id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_fkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_fkey","f","FOREIGN KEY (company_id) REFERENCES companies(id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_run_id_fact_id_key','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_run_id_fact_id_key","u","UNIQUE (company_id, run_id, fact_id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_run_id_fkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_run_id_fkey","f","FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_run_id_fkey1','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_run_id_fkey1","f","FOREIGN KEY (company_id, run_id) REFERENCES run_context_snapshots(company_id, run_id) DEFERRABLE INITIALLY DEFERRED",true,true,true]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_company_id_target_id_fkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_company_id_target_id_fkey","f","FOREIGN KEY (company_id, target_id) REFERENCES employee_reviewed_memory_targets(company_id, id)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_consumption_epoch_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_consumption_epoch_check","c","CHECK ((consumption_epoch >= 0))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_content_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_content_hash_check","c","CHECK ((octet_length(content_hash) = 32))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_fact_version_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_fact_version_check","c","CHECK ((fact_version = 1))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_namespace_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_namespace_hash_check","c","CHECK ((octet_length(namespace_hash) = 32))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_ordinal_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_ordinal_check","c","CHECK (((ordinal >= 0) AND (ordinal <= 7)))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_pkey','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_pkey","p","PRIMARY KEY (company_id, run_id, ordinal)",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_sharing_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_sharing_hash_check","c","CHECK ((octet_length(sharing_hash) = 32))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_source_authority_epoch_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_source_authority_epoch_check","c","CHECK ((source_authority_epoch >= 0))",true,false,false]'::jsonb),
        ('run_employee_reviewed_memory_uses','run_employee_reviewed_memory_uses_source_hash_check','["run_employee_reviewed_memory_uses","run_employee_reviewed_memory_uses_source_hash_check","c","CHECK ((octet_length(source_hash) = 32))",true,false,false]'::jsonb),
        ('runs','runs_payload_mode_check','["runs","runs_payload_mode_check","c","CHECK ((payload_mode = ANY (ARRAY[''ordinary''::text, ''confidential_dm_v1''::text])))",true,false,false]'::jsonb)
    ) AS required(relation,name,metadata) LOOP
        -- This original BETWEEN expands to a nested left-hand AND during
        -- analysis. Reparsing its deparsed AND text flattens that node again;
        -- use the exact migration syntax to restore the original catalog tree.
        definition := CASE
            WHEN item.relation='employee_reviewed_memory_facts'
                AND item.name='employee_reviewed_memory_facts_content_check'
            THEN $content77$CHECK(octet_length(content) BETWEEN 1 AND 4096 AND btrim(content)<>'')$content77$
            ELSE item.metadata->>3
        END;
        SELECT jsonb_build_array(c.relname,k.conname,k.contype,pg_get_constraintdef(k.oid,false),
            k.convalidated,k.condeferrable,k.condeferred) INTO observed
        FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND c.relname=item.relation AND k.conname=item.name;
        IF observed IS NULL AND item.metadata->>2='c' THEN
            EXECUTE format('ALTER TABLE public.%I ADD CONSTRAINT %I %s',item.relation,item.name,definition);
        ELSIF observed IS DISTINCT FROM item.metadata AND item.metadata->>2='c' THEN
            -- Exact resume03 bootstrap catalog SHA256 28ed4a2aefb310aeba18abab296da1641e49d55cf0e1dee9753dc535e51b8084.
            -- Across all 280 reviewed constraints, only these three present
            -- CHECKs differ: pgschema flattened nested associative AND nodes.
            -- Admit the observed full metadata only; no expression rewriting or
            -- pretty-deparse equivalence is an authorization to drop a guard.
            bootstrap := CASE item.relation||'.'||item.name
                WHEN 'confidential_run_payloads.confidential_run_payloads_check' THEN
                    '["confidential_run_payloads","confidential_run_payloads_check","c","CHECK ((((purpose = ANY (ARRAY[''snapshot''::text, ''reply_draft''::text])) AND (ordinal = 0)) OR ((purpose = ''runtime_event''::text) AND (ordinal >= 1) AND (ordinal <= 512))))",true,false,false]'::jsonb
                WHEN 'employee_reviewed_memory_export_commands.employee_reviewed_memory_export_commands_check' THEN
                    '["employee_reviewed_memory_export_commands","employee_reviewed_memory_export_commands_check","c","CHECK ((((action = ''publish''::text) AND (result_version = 0)) OR ((action = ANY (ARRAY[''retry_publish''::text, ''retry_withdraw''::text])) AND (result_version >= 1) AND (result_version <= 8))))",true,false,false]'::jsonb
                WHEN 'employee_reviewed_memory_facts.employee_reviewed_memory_facts_content_check' THEN
                    '["employee_reviewed_memory_facts","employee_reviewed_memory_facts_content_check","c","CHECK (((octet_length(content) >= 1) AND (octet_length(content) <= 4096) AND (btrim(content) <> ''''::text)))",true,false,false]'::jsonb
                ELSE NULL
            END;
            IF observed = bootstrap THEN
                EXECUTE format('ALTER TABLE public.%I DROP CONSTRAINT %I',item.relation,item.name);
                EXECUTE format('ALTER TABLE public.%I ADD CONSTRAINT %I %s',item.relation,item.name,definition);
            END IF;
        END IF;
        SELECT jsonb_build_array(c.relname,k.conname,k.contype,pg_get_constraintdef(k.oid,false),
            k.convalidated,k.condeferrable,k.condeferred) INTO observed
        FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND c.relname=item.relation AND k.conname=item.name;
        IF observed IS DISTINCT FROM item.metadata THEN
            RAISE EXCEPTION 'unexpected77 constraint %.%',item.relation,item.name;
        END IF;
    END LOOP;
END $reconcile77_constraints$;
