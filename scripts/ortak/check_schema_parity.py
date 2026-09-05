#!/usr/bin/env python3
"""Probe migration 55 versus real pgschema on disposable port 55432; retain both DBs.

Requires Python >=3.11 with psycopg2, the cached pgschema 1.7.4, and an explicitly
selected compiled buzz-db test binary. Reads only ORTAK_SCHEMA_PARITY_TEST_URL.
Never use live port 55433. Receipt and bounded command logs stay owner-private.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import time
from urllib.parse import unquote, urlsplit
from uuid import uuid4

PGSCHEMA = Path("/Users/nambse/Library/Caches/hermit/pkg/pgschema-1.7.4/pgschema")
URL_ENV = "ORTAK_SCHEMA_PARITY_TEST_URL"
REPO = Path(__file__).resolve().parents[2]
TEST = "runtime::migration::postgres_tests::run_migrations_applies_consolidated_initial_schema_on_fresh_database"
MAX_OUTPUT = 4 * 1024 * 1024
MAX_SECONDS = 300
MAX_SQL_SECONDS = 30
TABLES = ["project_api_bindings", "project_access_grants", "work_api_operations", "office_company_bindings"]
FUNCTIONS = ["ortak_check_routing_claim_expiry", "ortak_check_work_api_receipt", "ortak_project_access_guard",
             "ortak_assert_project_binding_purge", "ortak_guard_project_api_binding", "ortak_project_binding_purge_at_commit"]

# Columns are keyed by name: ALTER versus inline creation can reorder physical
# attnums. Ordered index/constraint keys, sort/null options and all deferred
# flags remain exact. Function bodies and catalog-rendered SQL are not rewritten.
CATALOG = r"""
WITH selected AS (
 SELECT c.oid,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relname=ANY(%s)
), functions AS (
 SELECT p.*,l.lanname FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 JOIN pg_language l ON l.oid=p.prolang
 WHERE n.nspname='public' AND p.proname=ANY(%s)
)
SELECT jsonb_build_object(
 'tables',(SELECT jsonb_agg(relname ORDER BY relname) FROM selected),
 'columns',(SELECT jsonb_agg(jsonb_build_array(c.relname,a.attname,format_type(a.atttypid,a.atttypmod),
   a.attnotnull,a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),coll.collname)
   ORDER BY c.relname,a.attname) FROM selected c JOIN pg_attribute a ON a.attrelid=c.oid
   LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
   LEFT JOIN pg_collation coll ON coll.oid=a.attcollation
   WHERE a.attnum>0 AND NOT a.attisdropped),
 'indexes',(SELECT jsonb_agg(jsonb_build_array(c.relname,ic.relname,pg_get_indexdef(i.indexrelid),
   i.indoption::int2[]::text,i.indisvalid,i.indisready,i.indisunique,i.indisprimary,i.indisexclusion)
   ORDER BY c.relname,ic.relname) FROM selected c JOIN pg_index i ON i.indrelid=c.oid
   JOIN pg_class ic ON ic.oid=i.indexrelid),
 'constraints',(SELECT jsonb_agg(jsonb_build_array(c.relname,k.conname,k.contype,
   pg_get_constraintdef(k.oid,false),k.convalidated,k.condeferrable,k.condeferred)
   ORDER BY c.relname,k.conname) FROM selected c JOIN pg_constraint k ON k.conrelid=c.oid),
 'triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pg_get_triggerdef(t.oid,false)) ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname='public' AND NOT t.tgisinternal AND
    (c.relname=ANY(%s) OR (c.relname='routing_decisions' AND t.tgname='ortak_routing_claim_expiry_at_commit'))),
 'functions',(SELECT jsonb_agg(jsonb_build_array(proname,pg_get_function_identity_arguments(oid),
   lanname,provolatile,proisstrict,prosecdef,proleakproof,proparallel,proconfig,
   pg_get_function_result(oid),prosrc) ORDER BY proname,pg_get_function_identity_arguments(oid)) FROM functions),
 'fence_targets',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,pg_get_triggerdef(t.oid,false)) ORDER BY c.relname,t.tgname)
   FROM pg_trigger t JOIN pg_proc p ON p.oid=t.tgfoid JOIN pg_class c ON c.oid=t.tgrelid
   JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'
   AND NOT t.tgisinternal AND p.proname='enforce_community_write_fence')
);
"""


class Refused(Exception):
    """Only fixed, non-sensitive error codes cross the command boundary."""


def selected_url(value):
    """Reject alternate ports/hosts, libpq parameters and ambient selection."""
    try:
        url = urlsplit(value)
        if (url.scheme not in ("postgres", "postgresql") or url.hostname != "127.0.0.1"
                or url.port != 55432 or url.query or url.fragment
                or not re.fullmatch(r"/[a-zA-Z_][a-zA-Z0-9_]{0,62}", url.path)
                or not re.fullmatch(r"[a-zA-Z_][a-zA-Z0-9_]{0,62}", url.username or "")
                or url.password is None or not url.password or len(value) > 1024
                or any(ord(c) < 32 for c in unquote(url.password))):
            raise ValueError()
        return {"host": "127.0.0.1", "port": 55432, "user": url.username,
                "password": unquote(url.password), "dbname": url.path[1:]}
    except (ValueError, TypeError, AttributeError):
        raise Refused("explicit_disposable_test_url_required") from None


def database_name(value):
    """Only this probe's fresh generated names may receive schema writes."""
    if not re.fullmatch(r"ortak_parity_[0-9a-f]{32}_(desired|migrated)", value):
        raise Refused("generated_database_name_required")
    return value


def executable(path):
    """Require an explicit absolute regular executable, without following links."""
    if not path.is_absolute() or path.resolve() != path:
        raise Refused("absolute_executable_required")
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not os.access(path, os.X_OK):
        raise Refused("regular_executable_required")
    return path


def write_private(path, content):
    """Persist one fresh protected file; existing probe evidence is never overwritten."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def document(path, value):
    write_private(path, (json.dumps(value, sort_keys=True, indent=2) + "\n").encode())


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class Commands:
    """Bound each local process group to 120s, all commands to 300s and logs to 4 MiB."""
    def __init__(self, directory):
        self.directory = directory
        self.deadline = time.monotonic() + MAX_SECONDS

    def run(self, label, args, environment):
        deadline = min(self.deadline, time.monotonic() + 120)
        if deadline <= time.monotonic():
            raise Refused("probe_deadline_exceeded")
        descriptor = os.open(self.directory / (label + ".log"),
                             os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, "wb") as log:
            child = subprocess.Popen(args, cwd=REPO, env=environment, stdin=subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                size = 0
                with selectors.DefaultSelector() as ready:
                    ready.register(child.stdout, selectors.EVENT_READ)
                    while ready.get_map():
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not ready.select(remaining):
                            raise Refused("child_deadline_exceeded")
                        block = os.read(child.stdout.fileno(), min(65536, MAX_OUTPUT - size + 1))
                        if not block:
                            ready.unregister(child.stdout)
                            continue
                        size += len(block)
                        if size > MAX_OUTPUT:
                            raise Refused("child_output_limit_exceeded")
                        log.write(block)
                if child.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
                    raise Refused("child_failed")
                if time.monotonic() >= deadline:
                    raise Refused("child_deadline_exceeded")
                log.flush()
                os.fsync(log.fileno())
            finally:
                # Stop the owned process group even if the leader already exited.
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=3)
                child.stdout.close()


def environment(connection, database, home):
    """Reconstruct both target and plan selection; never inherit libpq/service/proxy settings."""
    database_name(database)
    result = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C", "HOME": str(home),
              "PGHOST": "127.0.0.1", "PGPORT": "55432", "PGUSER": connection["user"],
              "PGPASSWORD": connection["password"], "PGDATABASE": database, "PGCONNECT_TIMEOUT": "3",
              "PGOPTIONS": "-c lock_timeout=2000 -c statement_timeout=110000 -c idle_in_transaction_session_timeout=110000"}
    for key in ("HOST", "PORT", "USER", "PASSWORD", "DB"):
        result["PGSCHEMA_PLAN_" + key] = result["PGDATABASE" if key == "DB" else "PG" + key]
    return result


def query_worker(selected, database, sql, parameters, options, result_path):
    """One owned SQL process; retain bounded diagnostics only in its private result."""
    # Neither native libpq diagnostics nor an unexpected child traceback may
    # escape to the operator's terminal. Explicit results remain available.
    descriptor = os.open(os.devnull, os.O_RDWR)
    try:
        for destination in (0, 1, 2):
            os.dup2(descriptor, destination)
    finally:
        if descriptor > 2:
            os.close(descriptor)
    os.environ.clear()
    os.environ.update({"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"})
    connection = None
    try:
        import psycopg2
        connection = psycopg2.connect(**{**selected, "dbname": database}, connect_timeout=3,
            options=options, application_name="ortak_schema_parity_55432",
            sslmode="disable", gssencmode="disable")
        connection.autocommit = True
        with connection.cursor() as cursor:
            cursor.execute(sql, parameters)
            value = cursor.fetchone()[0] if cursor.description else None
        result = {"status": "ok", "value": value}
    except Exception as error:
        result = {"status": "failed", "error_type": type(error).__name__,
                  "error_message": str(error)[:8192]}
    finally:
        if connection is not None:
            connection.close()
    encoded = (json.dumps(result, sort_keys=True) + "\n").encode()
    if len(encoded) > MAX_OUTPUT:
        encoded = b'{"status":"failed","error_type":"sql_result_limit_exceeded"}\n'
    write_private(result_path, encoded)


class Database:
    """Bound direct SQL by a spawned-process wall clock and server-side deadlines."""
    def __init__(self, selected, deadline, directory, worker=query_worker):
        self.selected, self.deadline, self.directory = selected, deadline, directory
        self.worker = worker

    def query(self, database, sql, parameters=None, *, admin=False):
        if not admin:
            database_name(database)
        elif database != self.selected["dbname"]:
            raise Refused("admin_database_changed")
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise Refused("probe_deadline_exceeded")
        deadline = time.monotonic() + min(MAX_SQL_SECONDS, remaining)
        options = (f"-c lock_timeout=2000 -c statement_timeout={max(1, min(30000, int(remaining * 1000)))} "
                   "-c idle_in_transaction_session_timeout=30000")
        result_path = self.directory / ("sql-" + uuid4().hex + ".json")
        process = multiprocessing.get_context("spawn").Process(target=self.worker,
            args=(self.selected, database, sql, parameters, options, result_path))
        try:
            process.start()
            process.join(timeout=max(0, deadline - time.monotonic()))
            if process.is_alive() or time.monotonic() >= deadline:
                raise Refused("sql_deadline_exceeded")
            if process.exitcode != 0:
                raise Refused("sql_worker_failed")
        finally:
            if process.pid is not None:
                if process.is_alive():
                    process.terminate()
                    process.join(timeout=0.2)
                if process.is_alive():
                    process.kill()
                    process.join(timeout=0.8)
                if process.is_alive():
                    raise Refused("sql_worker_containment_failed")
                process.close()
        # Read only after the writer has exited: a partial pipe or file cannot
        # block the parent. A dead worker never yields a partial success.
        with result_path.open("rb") as stream:
            encoded = stream.read(MAX_OUTPUT + 1)
        if len(encoded) > MAX_OUTPUT:
            raise Refused("sql_result_limit_exceeded")
        result = json.loads(encoded)
        if result.get("status") != "ok":
            raise Refused("sql_query_failed")
        return result["value"]


def checked_catalog(value):
    """Presence matters: equal empty catalogs do not prove the guards exist."""
    if (not isinstance(value, dict) or sorted(value.get("tables") or []) != sorted(TABLES)
            or sorted(row[0] for row in value.get("functions") or []) != sorted(FUNCTIONS)
            or not all(value.get(key) for key in ("columns", "indexes", "constraints", "triggers", "fence_targets"))):
        raise Refused("required_catalog_missing")
    triggers = {row[1]: row for row in value["triggers"]}
    for name, trigger_type in (("ortak_routing_claim_expiry_at_commit", 5),
                               ("work_api_receipt_at_commit", 5),
                               ("project_api_binding_purge_at_commit", 9)):
        if name not in triggers or triggers[name][2:6] != ["O", trigger_type, True, True]:
            raise Refused("deferred_commit_guard_missing")
    if not any(row[0] == "project_api_bindings" for row in value["fence_targets"]):
        raise Refused("work_community_fence_missing")
    return value


def probe(value, binary, receipt_parent, commands_type=Commands, database_type=Database):
    """Create exactly two fresh databases and retain them on every outcome."""
    selected = selected_url(value)  # Before even creating local files/children.
    executable(binary)
    executable(PGSCHEMA)
    if not receipt_parent.is_absolute() or receipt_parent.resolve() != receipt_parent:
        raise Refused("absolute_receipt_parent_required")
    info = receipt_parent.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise Refused("private_receipt_parent_required")
    identifier = uuid4().hex
    directory = receipt_parent / ("schema-parity-" + identifier)
    directory.mkdir(mode=0o700)
    home = directory / "home"
    home.mkdir(mode=0o700)
    desired, migrated = [database_name(f"ortak_parity_{identifier}_{kind}") for kind in ("desired", "migrated")]
    for relative, name in (("schema/schema.sql", "schema.sql"),
                           ("scripts/reconcile-schema-after-pgschema.sql", "reconcile.sql")):
        with (REPO / relative).open("rb") as stream:
            source = stream.read(MAX_OUTPUT + 1)
        if len(source) > MAX_OUTPUT:
            raise Refused("source_size_limit_exceeded")
        write_private(directory / name, source)
    receipt = {"format": "ortak-schema-parity/v1", "status": "started", "host": "127.0.0.1", "port": 55432,
               "desired_database": desired, "migrated_database": migrated, "databases_retained": True,
               "created_at": datetime.now(timezone.utc).isoformat(), "migration_target": 55,
               "migration_test_binary": str(binary), "migration_test_sha256": digest(binary),
               "pgschema_binary": str(PGSCHEMA), "pgschema_sha256": digest(PGSCHEMA),
               "schema_sha256": digest(directory / "schema.sql"),
               "reconciliation_sha256": digest(directory / "reconcile.sql")}
    document(directory / "intent.json", receipt)
    commands = commands_type(directory)
    db = database_type(selected, commands.deadline, directory)
    try:
        for name in (desired, migrated):
            # The generated grammar contains no quotes; never reuse/drop/clean a DB.
            db.query(selected["dbname"], f'CREATE DATABASE "{name}" TEMPLATE template0', admin=True)
        env = environment(selected, desired, home)
        commands.run("pgschema-apply", [str(PGSCHEMA), "apply", "--auto-approve", "--file", str(directory / "schema.sql"),
            "--host", "127.0.0.1", "--port", "55432", "--db", desired,
            "--plan-host", "127.0.0.1", "--plan-port", "55432", "--plan-db", desired], env)
        reconcile = (directory / "reconcile.sql").read_text()
        snapshots = []
        for _ in range(2):
            db.query(desired, reconcile)
            snapshots.append(checked_catalog(db.query(desired, CATALOG, (TABLES, FUNCTIONS, TABLES))))
        if snapshots[0] != snapshots[1]:
            raise Refused("reconciliation_not_idempotent")
        desired_catalog = snapshots[1]
        document(directory / "desired-catalog.json", desired_catalog)
        from urllib.parse import quote
        env = environment(selected, migrated, home)
        env["BUZZ_TEST_DATABASE_URL"] = (f"postgres://{selected['user']}:{quote(selected['password'], safe='')}@127.0.0.1:55432/{migrated}")
        commands.run("migration-test", [str(binary), "--exact", TEST, "--ignored", "--test-threads=1"], env)
        versions = db.query(migrated, "SELECT json_agg(version ORDER BY version) FROM _sqlx_migrations WHERE success")
        if versions != list(range(1, 56)):
            raise Refused("migration55_not_proven")
        migrated_catalog = checked_catalog(db.query(migrated, CATALOG, (TABLES, FUNCTIONS, TABLES)))
        document(directory / "migrated-catalog.json", migrated_catalog)
        different = sorted(key for key in desired_catalog if desired_catalog[key] != migrated_catalog.get(key))
        receipt["different_components"] = different
        if different:
            raise Refused("schema_catalog_mismatch")
        receipt.update(status="verified", migration_versions=versions, reconciliation_passes=2,
                       compared_components=sorted(desired_catalog), provider_calls=0)
    except Exception as error:
        receipt.update(status="failed", error_code=str(error) if isinstance(error, Refused) else "schema_probe_failed")
        raise Refused("schema_probe_failed_databases_retained") from None
    finally:
        receipt["finished_at"] = datetime.now(timezone.utc).isoformat()
        document(directory / "receipt.json", receipt)
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--migration-test-binary", type=Path, required=True)
    parser.add_argument("--receipt-parent", type=Path, required=True, help="existing real owner-private0700 directory")
    args = parser.parse_args()
    try:
        value = os.environ.get(URL_ENV)
        selected_url(value)
        # Dedicated CLI process: libpq must not consult ambient services, SSL
        # credentials, PGOPTIONS or alternate URL selectors during direct SQL.
        os.environ.clear()
        os.environ.update({"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"})
        directory = probe(value, args.migration_test_binary, args.receipt_parent)
        print(json.dumps({"status": "verified", "receipt": str(directory / "receipt.json"), "databases_retained": True}))
    except Exception:
        print(json.dumps({"status": "failed", "code": "schema_parity_failed", "databases_retained": True}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
