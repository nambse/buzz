#!/usr/bin/env python3
"""Back up and verify only the dated private PostgreSQL database, never replace it."""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import signal
import shutil
import stat
import struct
import subprocess
import time
import zlib
from uuid import uuid4

from init_private_stack import PROJECT, create_file
from private_native_services import selected_root

DOCKER = "/usr/local/bin/docker"
SOCKET = "unix:///Users/nambse/.docker/run/docker.sock"
CONTAINER = PROJECT + "-postgres-1"
IMAGE = "postgres:17.6-alpine@sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94"
DATABASE = "ortak"
MAX_DUMP = 256 * 1024 * 1024
MAX_OUTPUT = 1024 * 1024
MAX_ERRORS = 64 * 1024
MAX_SECONDS = 120

# Count actual rows, including partition parents/children, in the imported
# snapshot. Identifiers come from the catalog and are quoted with format(%I).
# No row content, credential, manifest body or function body leaves this query.
# Canonical live-column order survives pg_dump removing physical attnum holes.
# Keep the order itself: a restored table with reordered columns must differ.
COLUMN_ROWS_SQL = r"""
SELECT c.relname AS relation,
 row_number() OVER (PARTITION BY c.oid ORDER BY a.attnum) AS ordinal,
 a.attname AS name,format_type(a.atttypid,a.atttypmod) AS data_type,
 a.attnotnull AS not_null,a.attidentity AS identity_kind,a.attgenerated AS generated_kind,
 pg_get_expr(d.adbin,d.adrelid) AS default_value
FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid
JOIN pg_namespace n ON n.oid=c.relnamespace
LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
WHERE n.nspname='public' AND a.attnum>0 AND NOT a.attisdropped
 AND c.relkind IN ('r','p','v','m')
"""
SCHEMA_SQL = r"""
   SELECT jsonb_build_object(
    'relations',(SELECT jsonb_agg(jsonb_build_array(c.relname,c.relkind,c.relpersistence,c.reloptions,c.relrowsecurity,c.relforcerowsecurity,c.relreplident,pg_get_expr(c.relpartbound,c.oid)) ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S')),
    'columns',(SELECT jsonb_agg(jsonb_build_array(relation,ordinal,name,data_type,not_null,identity_kind,generated_kind,default_value) ORDER BY relation,ordinal) FROM live_columns),
    'constraints',(SELECT jsonb_agg(jsonb_build_array(c.relname,k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY c.relname,k.conname) FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'),
    'indexes',(SELECT jsonb_agg(jsonb_build_array(tablename,indexname,indexdef) ORDER BY tablename,indexname) FROM pg_indexes WHERE schemaname='public'),
    'triggers',(SELECT jsonb_agg(jsonb_build_array(c.relname,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid,true)) ORDER BY c.relname,t.tgname) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND NOT t.tgisinternal),
    'functions',(SELECT jsonb_agg(pg_get_functiondef(p.oid) ORDER BY p.proname,pg_get_function_identity_arguments(p.oid)) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public' AND p.prokind IN ('f','p')),
    'views',(SELECT jsonb_agg(jsonb_build_array(c.relname,pg_get_viewdef(c.oid,true)) ORDER BY c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('v','m')),
    'sequences',(SELECT jsonb_agg(jsonb_build_array(c.relname,s.seqstart,s.seqincrement,s.seqmax,s.seqmin,s.seqcache,s.seqcycle) ORDER BY c.relname) FROM pg_sequence s JOIN pg_class c ON c.oid=s.seqrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public'),
    'policies',(SELECT jsonb_agg(jsonb_build_array(tablename,policyname,permissive,roles,cmd,qual,with_check) ORDER BY tablename,policyname) FROM pg_policies WHERE schemaname='public'),
    'extensions',(SELECT jsonb_agg(jsonb_build_array(extname,extversion) ORDER BY extname) FROM pg_extension)
   ) AS document
"""
METADATA_SQL = "WITH live_columns AS (" + COLUMN_ROWS_SQL + "), schema_catalog AS (" + SCHEMA_SQL + ") " + r"""
SELECT jsonb_build_object(
 'server_version',current_setting('server_version'),
 'migration_checksums',(SELECT jsonb_agg(jsonb_build_array(version,encode(checksum,'hex'),success) ORDER BY version) FROM _sqlx_migrations),
 'private_company',(SELECT count(*) FROM companies WHERE slug='ortak-private-20260905'),
 'employee_states',(SELECT jsonb_object_agg(status,n) FROM (SELECT status,count(*) n FROM employees GROUP BY status) s),
 'tables',(SELECT jsonb_object_agg(name,n) FROM (
   SELECT format('%I.%I',ns.nspname,c.relname) name,
    ((xpath('/table/row/n/text()',query_to_xml(format('SELECT count(*) n FROM %I.%I',ns.nspname,c.relname),false,false,'')))[1]::text)::bigint n
   FROM pg_class c JOIN pg_namespace ns ON ns.oid=c.relnamespace
   WHERE ns.nspname='public' AND c.relkind IN ('r','p')
 ) counts),
 'schema_sha256',(SELECT encode(sha256(convert_to(document::text,'UTF8')),'hex') FROM schema_catalog),
 'schema_components',(SELECT jsonb_object_agg(key,encode(sha256(convert_to(value::text,'UTF8')),'hex')) FROM schema_catalog,jsonb_each(document))
);
"""


class Refused(Exception):
    """A bounded, non-sensitive failure code suitable for the private manifest."""

    def __init__(self, code, *, receipt_path=None):
        super().__init__(code)
        self.receipt_path = receipt_path


def environment():
    """No ambient Docker endpoint, context, credential, PG or proxy overrides."""
    return {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"}


def verification_name(value):
    """Accept only generated verification names, never the source or a test DB."""
    if not re.fullmatch(r"ortak_verify_[0-9a-f]{32}", value):
        raise Refused("verification_database_name_refused")
    return value


def private_directory(path, *, fresh=False):
    """Create/validate an owner-only real directory without adopting a link."""
    if fresh:
        path.mkdir(mode=0o700)
    elif not path.exists():
        path.mkdir(mode=0o700)
    metadata = path.lstat()
    if (not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o700):
        raise Refused("backup_directory_permissions_refused")
    return path


def private_binary(path):
    """Create a new regular output file mode0600, never follow an existing link."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    return os.fdopen(descriptor, "wb")


def digest(path):
    """Hash an archive without loading it into memory."""
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            result.update(block)
    return result.hexdigest()


class GzipOutput:
    """One level1 gzip member: zero mtime, no filename, bounded physical writes."""

    def __init__(self, sink, ceiling, remaining):
        self.sink, self.ceiling, self.remaining = sink, ceiling, remaining
        self.compressor = zlib.compressobj(level=1, wbits=-15)
        self.physical = self.uncompressed = self.crc = 0
        self.hashed = hashlib.sha256()
        # RFC1952: deflate, no flags/name, mtime0, fastest compression, unknown OS.
        self.emit(b'\x1f\x8b\x08\x00\x00\x00\x00\x00\x04\xff')

    def emit(self, data):
        """Check deadline and physical allowance before every write, including the footer."""
        self.remaining()
        if self.physical + len(data) > self.ceiling:
            raise Refused('command_output_limit_exceeded')
        self.sink.write(data)
        self.physical += len(data)

    def write(self, data):
        """Compress one already-bounded command stdout block without a raw temporary file."""
        self.uncompressed += len(data)
        self.hashed.update(data)
        self.crc = zlib.crc32(data, self.crc)
        self.emit(self.compressor.compress(data))

    def finish(self):
        """Write the complete footer only after successful child exit and empty stderr."""
        self.emit(self.compressor.flush(zlib.Z_FINISH))
        self.emit(struct.pack('<II', self.crc & 0xffffffff, self.uncompressed & 0xffffffff))
        return {'bytes': self.physical, 'uncompressed_bytes': self.uncompressed,
            'uncompressed_sha256': self.hashed.hexdigest()}


class Commands:
    """Bound command time, each output stream, and the local process group."""

    def __init__(self, root):
        self.root = root
        self.deadline = time.monotonic() + MAX_SECONDS
        self.container = None

    def remaining(self):
        value = self.deadline - time.monotonic()
        if value <= 0:
            raise Refused("operation_deadline_exceeded")
        return value

    def docker(self, *args):
        return [DOCKER, "--host", SOCKET, *args]

    def command(self, program, *args):
        if self.container is None:
            raise Refused("container_not_verified")
        # Container-side timeout remains effective even if Docker CLI transport
        # is interrupted. Serial pg_dump/pg_restore create no worker children.
        return self.docker("exec", "-i", "--user", "postgres", self.container,
            "timeout", "-s", "KILL", str(math.ceil(self.remaining())),
            "env", "-i", "PATH=/usr/local/bin:/usr/bin:/bin", "LC_ALL=C",
            "PGOPTIONS=-c lock_timeout=2000 -c statement_timeout=110000 -c idle_in_transaction_session_timeout=110000",
            program, *args)

    def psql(self, database):
        if database != DATABASE:
            verification_name(database)
        return self.command("psql", "--no-psqlrc", "--quiet", "--no-align", "--tuples-only",
            "--no-password", "--set", "ON_ERROR_STOP=1", "-h", "/var/run/postgresql",
            "-U", "ortak", "-d", database)

    @staticmethod
    def stop(process):
        # The leader may already have exited while a child still holds the
        # inherited output pipes. The owned group must be stopped regardless.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=3)
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None and not stream.closed:
                stream.close()

    def run(self, label, args, *, sql=None, archive=None, output=None, ceiling=MAX_OUTPUT,
            gzip_output=False, output_ceiling=None):
        """Default returns bytes; explicit gzip output returns counts/hash after footer and fsync.

        ``ceiling`` still bounds uncompressed stdout. ``output_ceiling`` bounds
        physical gzip bytes independently; gzip is available only with an output path.
        """
        if (type(gzip_output) is not bool or (not gzip_output and output_ceiling is not None)
                or (gzip_output and (output is None or type(ceiling) is not int or ceiling <= 0
                    or type(output_ceiling) is not int or output_ceiling <= 0))):
            raise Refused('command_gzip_options_refused')
        source = None
        if sql is not None:
            source = self.root / (label + ".sql")
            create_file(source, sql)
        if archive is not None:
            source = archive
        result = bytearray()
        stderr = self.root / (label + ".stderr")
        with (source.open("rb") if source else open(os.devnull, "rb")) as incoming, \
                private_binary(stderr) as errors:
            process = subprocess.Popen(args, stdin=incoming, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, env=environment(), start_new_session=True)
            sink = None
            compressed = None
            compressed_result = None
            try:
                if output:
                    sink = private_binary(output)
                    if gzip_output:
                        compressed = GzipOutput(sink, output_ceiling, self.remaining)
                sizes = {"out": 0, "err": 0}
                with selectors.DefaultSelector() as ready:
                    ready.register(process.stdout, selectors.EVENT_READ, "out")
                    ready.register(process.stderr, selectors.EVENT_READ, "err")
                    while ready.get_map():
                        events = ready.select(self.remaining())
                        if not events:
                            raise Refused("command_deadline_exceeded")
                        for key, _ in events:
                            block = os.read(key.fileobj.fileno(), 65536)
                            if not block:
                                ready.unregister(key.fileobj)
                                continue
                            kind = key.data
                            sizes[kind] += len(block)
                            if sizes[kind] > (ceiling if kind == "out" else MAX_ERRORS):
                                raise Refused("command_output_limit_exceeded")
                            if kind == "err":
                                errors.write(block)
                            elif sink:
                                (compressed or sink).write(block)
                            else:
                                result.extend(block)
                if process.wait(timeout=self.remaining()) != 0:
                    raise Refused("command_failed")
                if sizes["err"]:
                    # Warnings must not silently become verified backup evidence.
                    raise Refused("command_reported_diagnostics")
                if sink:
                    if compressed:
                        compressed_result = compressed.finish()
                    sink.flush()
                    os.fsync(sink.fileno())
            finally:
                try:
                    self.stop(process)
                finally:
                    if sink:
                        sink.close()
        return compressed_result if gzip_output else bytes(result)

    def inspect(self):
        # Only selected public identity fields, never docker inspect's Env array.
        template = ('{{json .Id}}\\n{{json .Config.Image}}\\n'
            '{{json (index .Config.Labels "com.docker.compose.project")}}\\n'
            '{{json (index .Config.Labels "com.docker.compose.service")}}\\n'
            '{{json .State.Running}}\\n{{json .Mounts}}')
        data = self.run("container", self.docker("inspect", "--format", template, CONTAINER))
        fields = [json.loads(line) for line in data.decode().replace("\\n", "\n").splitlines()]
        if (len(fields) != 6 or not re.fullmatch(r"[0-9a-f]{64}", fields[0])
                or fields[1:5] != [IMAGE, PROJECT, "postgres", True]
                or not any(m.get("Type") == "volume" and m.get("Name") == PROJECT + "_postgres_data"
                    and m.get("Destination") == "/var/lib/postgresql/data" and m.get("RW") is True
                    for m in fields[5])):
            raise Refused("private_container_identity_refused")
        volume = json.loads(self.run("volume", self.docker("volume", "inspect", "--format",
            "{{json .}}", PROJECT + "_postgres_data")))
        labels = volume.get("Labels") or {}
        if (labels.get("com.docker.compose.project") != PROJECT
                or labels.get("com.docker.compose.volume") != "postgres_data"
                or volume.get("Driver") != "local"
                or not any(m.get("Name") == PROJECT + "_postgres_data"
                    and m.get("Source") == volume.get("Mountpoint") for m in fields[5])):
            raise Refused("private_volume_ownership_refused")
        self.container = fields[0]
        return {"container_id": fields[0], "image": IMAGE, "volume": PROJECT + "_postgres_data"}

    @contextmanager
    def snapshot(self):
        process = subprocess.Popen(self.psql(DATABASE), stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment(), start_new_session=True)
        try:
            process.stdin.write(b"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\nDO $$BEGIN IF NOT pg_try_advisory_xact_lock_shared(7094711454081051697) THEN RAISE EXCEPTION 'schema fence busy'; END IF; END$$;\nSELECT pg_export_snapshot();\n")
            process.stdin.flush()
            value = bytearray()
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                ready.register(process.stderr, selectors.EVENT_READ)
                while b"\n" not in value:
                    events = ready.select(min(5, self.remaining()))
                    if not events:
                        raise Refused("snapshot_start_deadline_exceeded")
                    for key, _ in events:
                        block = os.read(key.fileobj.fileno(), 1024)
                        if key.fileobj is process.stderr or not block:
                            raise Refused("snapshot_start_failed")
                        value.extend(block)
                        if len(value) > 128:
                            raise Refused("snapshot_response_refused")
            snapshot = value.decode().strip()
            if not re.fullmatch(r"[0-9A-F]{8}-[0-9A-F]{8}-[0-9]+", snapshot):
                raise Refused("snapshot_identifier_refused")
            yield snapshot
            process.stdin.write(b"ROLLBACK;\n\\q\n")
            process.stdin.flush()
            if process.wait(timeout=min(3, self.remaining())) != 0:
                raise Refused("snapshot_holder_failed")
        finally:
            self.stop(process)

    def metadata(self, database, label, snapshot=None):
        transaction = "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n"
        if snapshot:
            if not re.fullmatch(r"[0-9A-F]{8}-[0-9A-F]{8}-[0-9]+", snapshot):
                raise Refused("snapshot_identifier_refused")
            transaction += f"SET TRANSACTION SNAPSHOT '{snapshot}';\n"
        # Reject an unexpectedly enlarged/private foreign schema before counts.
        transaction += "DO $$BEGIN IF current_user<>'ortak' OR (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p'))>2048 THEN RAISE EXCEPTION 'database bounds refused'; END IF; END$$;\n"
        value = json.loads(self.run(label, self.psql(database), sql=transaction + METADATA_SQL + "\nROLLBACK;\n"))
        if (not re.fullmatch(r"[0-9a-f]{64}", value["schema_sha256"])
                or not value["tables"] or len(value["tables"]) > 2048
                or not all(type(n) is int and n >= 0 for n in value["tables"].values())
                or value["private_company"] != 1 or value["tables"].get("public.companies") != 1
                or not value["migration_checksums"] or value["migration_checksums"][-1][0] < 52
                or not all(row[2] for row in value["migration_checksums"])):
            raise Refused("private_database_schema_refused")
        return value


def backup(root, commands_type=Commands):
    """Always make a new archive and a new verification database; never restore in place."""
    os.umask(0o077)
    backups = private_directory(root / "backups")
    destination = private_directory(backups / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "_" + uuid4().hex), fresh=True)
    command = commands_type(destination)
    verification = verification_name("ortak_verify_" + uuid4().hex)
    manifest = {"format": "ortak-private-database-backup/1", "project": PROJECT,
        "source_database": DATABASE, "verification_database": verification,
        "database_only": True, "status": "started"}
    create_file(destination / "intent.json", json.dumps(manifest, indent=2) + "\n")
    try:
        manifest["source"] = command.inspect()
        size = int(command.run("database-size", command.psql(DATABASE),
            sql="SELECT pg_database_size(current_database());\n", ceiling=128))
        manifest["source_database_bytes"] = size
        if size <= 0 or shutil.disk_usage(destination).free < MAX_DUMP + 2 * size:
            raise Refused("insufficient_backup_and_restore_space")
        with command.snapshot() as snapshot:
            manifest["snapshot"] = snapshot
            manifest["expected"] = command.metadata(DATABASE, "source", snapshot)
            archive = destination / "database.dump"
            command.run("dump", command.command("pg_dump", "--format=custom", "--no-password",
                "--lock-wait-timeout=2s", "--snapshot=" + snapshot, "-h", "/var/run/postgresql",
                "-U", "ortak", "-d", DATABASE), output=archive, ceiling=MAX_DUMP)
        manifest["archive_bytes"] = archive.stat().st_size
        manifest["archive_sha256"] = digest(archive)
        command.run("create-verification", command.command("createdb", "--no-password", "-h",
            "/var/run/postgresql", "-U", "ortak", "--maintenance-db=ortak", "--template=template0",
            "--owner=ortak", verification))
        manifest["verification_created"] = True
        from private_restore_credential_functions import restore_sections, Refused as RestoreRefused
        try:
            manifest["restore_compatibility"] = restore_sections(command, verification, archive)
        except RestoreRefused as error:
            # A directly executed CLI is __main__, while the shared helper
            # imports this module by name. Normalize that exception identity so
            # an allowlist refusal still writes the private failed manifest.
            raise Refused(str(error)) from None
        restored = command.metadata(verification, "restored")
        manifest["restored"] = restored
        if restored != manifest["expected"]:
            expected = manifest["expected"]
            manifest["different_fields"] = sorted(key for key in expected.keys() | restored.keys()
                if expected.get(key) != restored.get(key))
            manifest["different_schema_components"] = sorted(key for key in
                expected.get("schema_components", {}).keys() | restored.get("schema_components", {}).keys()
                if expected.get("schema_components", {}).get(key) != restored.get("schema_components", {}).get(key))
            raise Refused("restore_metadata_mismatch")
        manifest["status"] = "verified"
    except (Refused, OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        manifest["status"] = "failed"
        manifest["error_code"] = str(error) if isinstance(error, Refused) else "backup_operation_failed"
        create_file(destination / "manifest.json", json.dumps(manifest, indent=2) + "\n")
        raise Refused("backup_failed_private_manifest_retained", receipt_path=destination / 'manifest.json') from None
    create_file(destination / "manifest.json", json.dumps(manifest, indent=2) + "\n")
    return destination


def main():
    """Operate only on the initializer's completed fixed private stack marker."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    args = parser.parse_args()
    destination = backup(selected_root(args.state_dir))
    print(f"Database-only backup and fresh restore verified: {destination}")
    print("Private archive retained mode0600; verification database retained. No employee activation occurred.")


if __name__ == "__main__":
    try:
        main()
    except (Refused, OSError, ValueError):
        raise SystemExit("Database backup/verification failed; private artifacts retained. No source database was replaced.") from None
