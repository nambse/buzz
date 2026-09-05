-- Reconcile schema details that pgschema does not preserve.
--
-- pgschema reconciles DDL, but it does not execute seed DML or preserve every
-- table storage parameter from schema/schema.sql. It also currently emits
-- partition children as standalone CREATE TABLE statements. Every pgschema
-- apply caller must run this idempotent script so fresh bootstraps converge on
-- the same live database contract as migration-managed databases.

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
SELECT company_id,id FROM runs WHERE status='completed' AND delivery_intent IN ('reply','channel')
ON CONFLICT (company_id,run_id) DO NOTHING;
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM runs r WHERE r.status='completed'
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
