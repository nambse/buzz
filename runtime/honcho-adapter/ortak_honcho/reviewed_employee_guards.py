"""Employee-family-only immutable evidence and deferred atomic content guards."""

from sqlalchemy import text

from .reviewed_employee_models import TABLES, contents, diagnostic_content

PIN_ROW = "company_id,deployment_id,namespace_hash,binding_hash,ownership,target_id,destination_channel_id,content_hash,source_hash,sharing_hash"
DIAG_ROW = "company_id,deployment_id,namespace_hash,binding_hash,ownership,employee_revision_id,employee_lifecycle_epoch,challenge_hash"


def fields(alias, names):
    return ",".join(f"{alias}.{name}" for name in names.split(","))


FUNCTIONS = [
    """CREATE OR REPLACE FUNCTION ortak_employee_memory_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN RAISE EXCEPTION 'employee memory evidence is immutable'; END $$""",
    """CREATE OR REPLACE FUNCTION ortak_employee_content_guard() RETURNS trigger LANGUAGE plpgsql AS $$
    BEGIN
      IF TG_OP='UPDATE' THEN RAISE EXCEPTION 'employee content cannot change'; END IF;
      IF TG_TABLE_NAME='ortak_employee_reviewed_content' THEN
        IF TG_OP='INSERT' THEN
          IF EXISTS(SELECT 1 FROM ortak_employee_reviewed_tombstones t WHERE
              (t.workspace_id,t.employee_id,t.record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id))
            OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_records r WHERE
              (r.workspace_id,r.employee_id,r.record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id)
              AND r.xmin::text::bigint=txid_current()%4294967296
              AND r.content_hash=encode(sha256(convert_to(NEW.content,'UTF8')),'hex'))
          THEN RAISE EXCEPTION 'employee content requires fresh matching header without tombstone'; END IF;
        ELSIF NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_tombstones t WHERE
            (t.workspace_id,t.employee_id,t.record_id)=(OLD.workspace_id,OLD.employee_id,OLD.record_id))
        THEN RAISE EXCEPTION 'employee erasure requires tombstone'; END IF;
      ELSE
        IF TG_OP='INSERT' THEN
          IF EXISTS(SELECT 1 FROM ortak_employee_diagnostic_tombstones t WHERE
              (t.workspace_id,t.employee_id,t.operation_id)=(NEW.workspace_id,NEW.employee_id,NEW.operation_id))
            OR NOT EXISTS(SELECT 1 FROM ortak_employee_diagnostics d WHERE
              (d.workspace_id,d.employee_id,d.operation_id)=(NEW.workspace_id,NEW.employee_id,NEW.operation_id)
              AND d.xmin::text::bigint=txid_current()%4294967296
              AND d.challenge_hash=encode(sha256(convert_to(NEW.challenge,'UTF8')),'hex'))
          THEN RAISE EXCEPTION 'diagnostic content requires fresh matching header without tombstone'; END IF;
        ELSIF NOT EXISTS(SELECT 1 FROM ortak_employee_diagnostic_tombstones t WHERE
            (t.workspace_id,t.employee_id,t.operation_id)=(OLD.workspace_id,OLD.employee_id,OLD.operation_id))
        THEN RAISE EXCEPTION 'diagnostic erasure requires tombstone'; END IF;
      END IF;
      IF TG_OP='DELETE' THEN RETURN OLD; END IF; RETURN NEW;
    END $$""",
    f"""CREATE OR REPLACE FUNCTION ortak_employee_record_commit() RETURNS trigger LANGUAGE plpgsql AS $$
    DECLARE r ortak_employee_reviewed_records; t ortak_employee_reviewed_tombstones; has_r boolean; has_t boolean;
    BEGIN
      SELECT * INTO r FROM ortak_employee_reviewed_records WHERE
        (workspace_id,employee_id,record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id); has_r=FOUND;
      SELECT * INTO t FROM ortak_employee_reviewed_tombstones WHERE
        (workspace_id,employee_id,record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id); has_t=FOUND;
      IF has_r AND has_t AND ROW({fields('r', PIN_ROW)}) IS DISTINCT FROM ROW({fields('t', PIN_ROW)})
      THEN RAISE EXCEPTION 'employee tombstone ownership differs'; END IF;
      IF has_r AND NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_operations o WHERE
          (o.workspace_id,o.employee_id,o.record_id)=(r.workspace_id,r.employee_id,r.record_id)
          AND o.action='publish' AND o.idempotency_key=r.publish_key
          AND o.request_hash=r.request_hash AND o.body_hash=r.body_hash
          AND (TG_TABLE_NAME<>'ortak_employee_reviewed_records' OR o.xmin::text::bigint=txid_current()%4294967296))
      THEN RAISE EXCEPTION 'employee publication requires atomic receipt'; END IF;
      IF has_t THEN
        IF EXISTS(SELECT 1 FROM ortak_employee_reviewed_content c WHERE
            (c.workspace_id,c.employee_id,c.record_id)=(t.workspace_id,t.employee_id,t.record_id))
          OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_operations o WHERE
            (o.workspace_id,o.employee_id,o.record_id)=(t.workspace_id,t.employee_id,t.record_id)
            AND o.action='withdraw' AND o.idempotency_key=t.withdraw_key
            AND o.request_hash=t.request_hash AND o.body_hash=t.body_hash
            AND (TG_TABLE_NAME<>'ortak_employee_reviewed_tombstones' OR o.xmin::text::bigint=txid_current()%4294967296))
        THEN RAISE EXCEPTION 'employee withdrawal requires receipt and no text'; END IF;
      ELSIF has_r THEN
        IF NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_content c WHERE
            (c.workspace_id,c.employee_id,c.record_id)=(r.workspace_id,r.employee_id,r.record_id)
            AND r.content_hash=encode(sha256(convert_to(c.content,'UTF8')),'hex'))
          OR TG_TABLE_NAME='ortak_employee_reviewed_records' AND
            (r.expires_at<=clock_timestamp() OR r.expires_at>clock_timestamp()+interval '90 days')
        THEN RAISE EXCEPTION 'employee publication requires live matching text'; END IF;
      END IF;
      IF TG_TABLE_NAME='ortak_employee_reviewed_operations' THEN
        IF NEW.action='publish' THEN
          IF NOT has_r OR NEW.idempotency_key<>r.publish_key OR NEW.request_hash<>r.request_hash OR NEW.body_hash<>r.body_hash
            OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_records x WHERE
              (x.workspace_id,x.employee_id,x.record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id)
              AND x.xmin::text::bigint=txid_current()%4294967296)
          THEN RAISE EXCEPTION 'employee publish receipt lacks atomic header'; END IF;
        ELSIF NOT has_t OR NEW.idempotency_key<>t.withdraw_key OR NEW.request_hash<>t.request_hash OR NEW.body_hash<>t.body_hash
          OR NOT EXISTS(SELECT 1 FROM ortak_employee_reviewed_tombstones x WHERE
            (x.workspace_id,x.employee_id,x.record_id)=(NEW.workspace_id,NEW.employee_id,NEW.record_id)
            AND x.xmin::text::bigint=txid_current()%4294967296)
        THEN RAISE EXCEPTION 'employee withdraw receipt lacks atomic tombstone'; END IF;
      END IF;
      RETURN NULL;
    END $$""",
    f"""CREATE OR REPLACE FUNCTION ortak_employee_diagnostic_commit() RETURNS trigger LANGUAGE plpgsql AS $$
    DECLARE d ortak_employee_diagnostics; t ortak_employee_diagnostic_tombstones; has_d boolean; has_t boolean;
    BEGIN
      SELECT * INTO d FROM ortak_employee_diagnostics WHERE
        (workspace_id,employee_id,operation_id)=(NEW.workspace_id,NEW.employee_id,NEW.operation_id); has_d=FOUND;
      SELECT * INTO t FROM ortak_employee_diagnostic_tombstones WHERE
        (workspace_id,employee_id,operation_id)=(NEW.workspace_id,NEW.employee_id,NEW.operation_id); has_t=FOUND;
      IF has_d AND has_t AND ROW({fields('d', DIAG_ROW)}) IS DISTINCT FROM ROW({fields('t', DIAG_ROW)})
      THEN RAISE EXCEPTION 'diagnostic tombstone ownership differs'; END IF;
      IF has_t THEN
        IF EXISTS(SELECT 1 FROM ortak_employee_diagnostic_content c WHERE
            (c.workspace_id,c.employee_id,c.operation_id)=(t.workspace_id,t.employee_id,t.operation_id))
        THEN RAISE EXCEPTION 'diagnostic erasure left content'; END IF;
      ELSIF has_d AND NOT EXISTS(SELECT 1 FROM ortak_employee_diagnostic_content c WHERE
          (c.workspace_id,c.employee_id,c.operation_id)=(d.workspace_id,d.employee_id,d.operation_id)
          AND d.challenge_hash=encode(sha256(convert_to(c.challenge,'UTF8')),'hex'))
      THEN RAISE EXCEPTION 'diagnostic write requires atomic challenge'; END IF;
      RETURN NULL;
    END $$""",
]


async def install(connection):
    """Explicit initialization only; fixed source identifiers, no request DDL."""
    for statement in FUNCTIONS:
        await connection.execute(text(statement))
    for table in TABLES:
        name = table.name
        triggers = [("retain", "BEFORE TRUNCATE", "STATEMENT", "ortak_employee_memory_immutable")]
        if table is contents or table is diagnostic_content:
            triggers.append(("content_guard", "BEFORE INSERT OR UPDATE OR DELETE", "ROW", "ortak_employee_content_guard"))
        else:
            triggers.append(("immutable", "BEFORE UPDATE OR DELETE", "ROW", "ortak_employee_memory_immutable"))
        for suffix, timing, level, function in triggers:
            trigger = name + "_" + suffix
            await connection.execute(text(f"DROP TRIGGER IF EXISTS {trigger} ON {name}"))
            await connection.execute(text(f"CREATE TRIGGER {trigger} {timing} ON {name} FOR EACH {level} EXECUTE FUNCTION {function}()"))
        if table is not contents and table is not diagnostic_content:
            function = "ortak_employee_diagnostic_commit" if name.startswith("ortak_employee_diagnostic") else "ortak_employee_record_commit"
            trigger = name + "_commit"
            await connection.execute(text(f"DROP TRIGGER IF EXISTS {trigger} ON {name}"))
            await connection.execute(text(f"CREATE CONSTRAINT TRIGGER {trigger} AFTER INSERT ON {name} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION {function}()"))
