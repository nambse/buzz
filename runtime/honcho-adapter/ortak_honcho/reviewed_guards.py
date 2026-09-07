"""Idempotent database guards for retained tombstones and atomic text transitions."""

from sqlalchemy import text

FUNCTIONS = [
    """CREATE OR REPLACE FUNCTION ortak_reviewed_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN RAISE EXCEPTION 'reviewed lifecycle evidence is immutable'; END $$""",
    """CREATE OR REPLACE FUNCTION ortak_reviewed_content_guard() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
      IF TG_OP='UPDATE' THEN RAISE EXCEPTION 'reviewed text cannot be rewritten'; END IF;
      IF TG_OP='INSERT' AND EXISTS(SELECT 1 FROM ortak_reviewed_tombstones
          WHERE workspace_id=NEW.workspace_id AND project_id=NEW.project_id AND record_id=NEW.record_id)
      THEN RAISE EXCEPTION 'reviewed tombstone forbids resurrection'; END IF;
      IF TG_OP='DELETE' AND NOT EXISTS(SELECT 1 FROM ortak_reviewed_tombstones
          WHERE workspace_id=OLD.workspace_id AND project_id=OLD.project_id AND record_id=OLD.record_id)
      THEN RAISE EXCEPTION 'reviewed erasure requires a tombstone'; END IF;
      IF TG_OP='DELETE' THEN RETURN OLD; END IF; RETURN NEW;
    END $$""",
    """CREATE OR REPLACE FUNCTION ortak_reviewed_record_commit() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
      IF NOT EXISTS(SELECT 1 FROM ortak_reviewed_operations o WHERE o.workspace_id=NEW.workspace_id
          AND o.project_id=NEW.project_id AND o.record_id=NEW.record_id AND o.action='publish'
          AND o.idempotency_key=NEW.publish_key AND o.request_hash=NEW.request_hash
          AND o.xmin::text::bigint = (txid_current() % 4294967296))
      THEN RAISE EXCEPTION 'reviewed publication requires its atomic receipt'; END IF;
      IF NOT EXISTS(SELECT 1 FROM ortak_reviewed_tombstones t WHERE t.workspace_id=NEW.workspace_id
          AND t.project_id=NEW.project_id AND t.record_id=NEW.record_id)
      AND (NEW.expires_at <= clock_timestamp() OR NEW.expires_at > clock_timestamp()+interval '90 days'
          OR NOT EXISTS(SELECT 1 FROM ortak_reviewed_record_content c WHERE c.workspace_id=NEW.workspace_id
              AND c.project_id=NEW.project_id AND c.record_id=NEW.record_id))
      THEN RAISE EXCEPTION 'reviewed publication needs live text or a retained tombstone'; END IF;
      RETURN NULL;
    END $$""",
    """CREATE OR REPLACE FUNCTION ortak_reviewed_tombstone_commit() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
      IF EXISTS(SELECT 1 FROM ortak_reviewed_record_content c WHERE c.workspace_id=NEW.workspace_id
          AND c.project_id=NEW.project_id AND c.record_id=NEW.record_id)
      THEN RAISE EXCEPTION 'reviewed erasure left text'; END IF;
      IF NOT EXISTS(SELECT 1 FROM ortak_reviewed_operations o WHERE o.workspace_id=NEW.workspace_id
          AND o.project_id=NEW.project_id AND o.record_id=NEW.record_id
          AND o.action=CASE NEW.reason WHEN 'expired' THEN 'expire' ELSE 'withdraw' END
          AND o.xmin::text::bigint = (txid_current() % 4294967296))
      THEN RAISE EXCEPTION 'reviewed erasure requires its atomic receipt'; END IF;
      IF NEW.reason='expired' AND NOT EXISTS(SELECT 1 FROM ortak_reviewed_records r
          WHERE r.workspace_id=NEW.workspace_id AND r.project_id=NEW.project_id AND r.record_id=NEW.record_id
          AND r.expires_at <= clock_timestamp())
      THEN RAISE EXCEPTION 'reviewed expiry is premature'; END IF;
      RETURN NULL;
    END $$""",
    """CREATE OR REPLACE FUNCTION ortak_reviewed_operation_commit() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
      IF NEW.action='publish' THEN
        IF NOT EXISTS(SELECT 1 FROM ortak_reviewed_records r WHERE r.workspace_id=NEW.workspace_id
            AND r.project_id=NEW.project_id AND r.record_id=NEW.record_id
            AND r.publish_key=NEW.idempotency_key AND r.request_hash=NEW.request_hash
            AND r.xmin::text::bigint = (txid_current() % 4294967296))
        THEN RAISE EXCEPTION 'reviewed receipt requires an atomic record'; END IF;
      ELSIF NOT EXISTS(SELECT 1 FROM ortak_reviewed_tombstones t WHERE t.workspace_id=NEW.workspace_id
          AND t.project_id=NEW.project_id AND t.record_id=NEW.record_id)
        OR EXISTS(SELECT 1 FROM ortak_reviewed_record_content c WHERE c.workspace_id=NEW.workspace_id
          AND c.project_id=NEW.project_id AND c.record_id=NEW.record_id)
      THEN RAISE EXCEPTION 'reviewed receipt requires proven text removal'; END IF;
      RETURN NULL;
    END $$""",
]


async def install(connection):
    """Called by explicit schema initialization/tests, never a request or health read."""
    for statement in FUNCTIONS:
        await connection.execute(text(statement))
    tables = (
        "ortak_reviewed_records",
        "ortak_reviewed_tombstones",
        "ortak_reviewed_operations",
    )
    for table in (*tables, "ortak_reviewed_record_content"):
        # All interpolated identifiers are fixed source literals.
        name = table + "_retain"
        await connection.execute(text(f"DROP TRIGGER IF EXISTS {name} ON {table}"))
        await connection.execute(
            text(
                f"CREATE TRIGGER {name} BEFORE TRUNCATE ON {table} "
                "FOR EACH STATEMENT EXECUTE FUNCTION ortak_reviewed_immutable()"
            )
        )
        if table in tables:
            name = table + "_immutable"
            await connection.execute(text(f"DROP TRIGGER IF EXISTS {name} ON {table}"))
            await connection.execute(
                text(
                    f"CREATE TRIGGER {name} BEFORE UPDATE OR DELETE ON {table} "
                    "FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_immutable()"
                )
            )
    await connection.execute(
        text(
            "DROP TRIGGER IF EXISTS ortak_reviewed_content_guard ON ortak_reviewed_record_content"
        )
    )
    await connection.execute(
        text(
            "CREATE TRIGGER ortak_reviewed_content_guard BEFORE INSERT OR UPDATE OR DELETE "
            "ON ortak_reviewed_record_content FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_content_guard()"
        )
    )
    for table, function in (
        ("ortak_reviewed_records", "ortak_reviewed_record_commit"),
        ("ortak_reviewed_tombstones", "ortak_reviewed_tombstone_commit"),
        ("ortak_reviewed_operations", "ortak_reviewed_operation_commit"),
    ):
        await connection.execute(text(f"DROP TRIGGER IF EXISTS {function} ON {table}"))
        await connection.execute(
            text(
                f"CREATE CONSTRAINT TRIGGER {function} AFTER INSERT ON {table} "
                f"DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION {function}()"
            )
        )
