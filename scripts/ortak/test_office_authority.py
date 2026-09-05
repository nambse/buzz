#!/usr/bin/env python3
"""Exercise migration 0048's production SQL guards with independent PG sessions.

Requires psql and an ALREADY BOOTSTRAPPED disposable database named explicitly by
ORTAK_TEST_DATABASE_URL. Never consults DATABASE_URL or creates/drops databases.
Fixtures use fresh UUIDs. No runtime, relay, or external service is contacted.
"""
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import time
from urllib.parse import urlparse
import uuid

URL = os.environ.get("ORTAK_TEST_DATABASE_URL", "")
parsed = urlparse(URL)
if parsed.hostname not in ("127.0.0.1", "localhost") or parsed.port != 55432:
    raise SystemExit("ORTAK_TEST_DATABASE_URL must explicitly select disposable localhost:55432")
PSQL = os.environ.get("PSQL", shutil.which("psql") or "")
if not PSQL:
    fallback = Path("/Applications/Postgres.app/Contents/Versions/latest/bin/psql")
    PSQL = str(fallback) if fallback.exists() else ""
if not PSQL:
    raise SystemExit("psql is required (or set PSQL)")
ARGS = [PSQL, URL, "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1", "-v", "VERBOSITY=verbose"]


def sql(statement, error=None):
    result = subprocess.run(ARGS, input=statement, text=True, capture_output=True, timeout=8)
    if error:
        assert result.returncode != 0 and error in result.stderr, result.stderr
        return result.stderr
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


class Session:
    """One bounded psql transaction; no sleeps are needed to arrange races."""
    def __enter__(self):
        self.proc = subprocess.Popen(ARGS, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT, bufsize=0)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.proc.stdout, selectors.EVENT_READ)
        self.query("SET statement_timeout = '4s'; SET lock_timeout = '2s'; BEGIN;")
        return self

    def send(self, statement):
        self.marker = "DONE_" + uuid.uuid4().hex
        self.proc.stdin.write((statement + "\n\\echo " + self.marker + "\n").encode())

    def read(self):
        data = b""
        deadline = time.monotonic() + 6
        while self.marker.encode() not in data:
            if not self.selector.select(max(0, deadline - time.monotonic())):
                raise AssertionError("session exceeded bounded timeout")
            chunk = os.read(self.proc.stdout.fileno(), 65536)
            assert chunk, data.decode()
            data += chunk
            assert len(data) < 1024 * 1024, "session output exceeded bound"
        value = data.decode().split(self.marker)[0].strip()
        assert "ERROR:" not in value, value
        return value

    def query(self, statement):
        self.send(statement)
        return self.read()

    def __exit__(self, *_):
        if self.proc.poll() is None:
            self.proc.stdin.write(b"ROLLBACK;\n\\q\n")
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=3)
        self.selector.close()
        self.proc.stdin.close()
        self.proc.stdout.close()


class Fixture:
    def __init__(self):
        self.company, self.community, self.channel, self.revision, self.run = [str(uuid.uuid4()) for _ in range(5)]
        self.key, self.human, self.event = [uuid.uuid4().hex * 2 for _ in range(3)]
        self.c = "'" + self.company + "'"
        self.q = "'" + self.community + "'"
        self.h = "'" + self.channel + "'"
        sql(f"""
        BEGIN;
        INSERT INTO companies(id,slug,display_name) VALUES ({self.c},'f-{self.company}','Fence');
        INSERT INTO communities(id,host) VALUES ({self.q},'f-{self.community}.local');
        INSERT INTO office_company_bindings(company_id,community_id) VALUES ({self.c},{self.q});
        INSERT INTO channels(id,community_id,name,created_by) VALUES ({self.h},{self.q},'fence',decode('{self.human}','hex'));
        INSERT INTO users(community_id,pubkey) VALUES ({self.q},decode('{self.human}','hex'));
        INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES ({self.q},{self.h},decode('{self.key}','hex'));
        INSERT INTO employees(company_id,id) VALUES ({self.c},'fixture');
        INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode)
        VALUES ({self.c},'{self.revision}','fixture',1,'{{}}',decode(repeat('01',32),'hex'),'create');
        UPDATE employees SET active_revision_id='{self.revision}',status='active' WHERE company_id={self.c};
        INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at)
        VALUES ({self.c},'fixture','{self.revision}','create',decode('{self.key}','hex'),'credential://test/fence',now());
        INSERT INTO runs(company_id,id,employee_id,employee_revision_id,runtime_adapter)
        VALUES ({self.c},'{self.run}','fixture','{self.revision}','test');
        INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
        VALUES ({self.q},decode('{self.event}','hex'),decode('{self.human}','hex'),clock_timestamp(),9,'[]','fixture',decode(repeat('00',64),'hex'),{self.h});
        INSERT INTO thread_metadata(community_id,event_created_at,event_id,channel_id)
        SELECT community_id,created_at,id,channel_id FROM events WHERE community_id={self.q};
        INSERT INTO outbox(company_id,kind,dedup_key,run_id,signed_event_id,signed_event_bytes)
        VALUES ({self.c},'office_publish','fixture','{self.run}',decode('{self.event}','hex'),decode('01','hex'));
        COMMIT;
        """)

    def generation(self):
        return int(sql(f"SELECT ortak_lock_office_authority({self.c});"))

    def decision(self, generation, deadline="NULL"):
        msg = uuid.uuid4().hex * 2
        return f"""INSERT INTO routing_decisions(company_id,message_id,root_message_id,
        inbox_claim_generation,origin_type,origin_id,mode,summary_reason,policy_version,policy_fingerprint,input_hash,
        office_authority_generation,office_authority_valid_before,office_input_hash)
        VALUES ({self.c},decode('{msg}','hex'),decode('{msg}','hex'),0,'human','fixture','silent','fixture','v0',
        'sha256:'||repeat('0',64),decode(repeat('01',32),'hex'),{generation},{deadline},decode(repeat('02',32),'hex'));"""


def test_mutation_coverage(f):
    statements = [
        f"UPDATE channels SET archived_at=now() WHERE community_id={f.q}",
        f"UPDATE channels SET visibility='private' WHERE community_id={f.q}",
        f"UPDATE channel_members SET removed_at=now() WHERE community_id={f.q}",
        f"UPDATE channel_members SET role='bot' WHERE community_id={f.q}",
        f"UPDATE users SET deactivated_at=now() WHERE community_id={f.q}",
        f"UPDATE users SET agent_type='legacy' WHERE community_id={f.q}",
        f"INSERT INTO relay_members(community_id,pubkey,role) VALUES({f.q},'{f.human}','member')",
        f"UPDATE employee_office_bindings SET valid_until=now()+interval '1 second' WHERE company_id={f.c}",
        f"INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,valid_until) VALUES ({f.c},'fixture','{f.revision}','create',decode(repeat('07',32),'hex'),'credential://test/other',now()+interval '1 day')",
        f"UPDATE events SET deleted_at=now() WHERE community_id={f.q}",
        f"DELETE FROM events WHERE community_id={f.q}",
        f"UPDATE thread_metadata SET parent_event_id=decode(repeat('02',32),'hex'),parent_event_created_at=now() WHERE community_id={f.q}",
        f"UPDATE employees SET status='paused' WHERE company_id={f.c}",
        f"UPDATE companies SET routing_policy='{{\"changed\":true}}' WHERE id={f.c}",
        f"UPDATE runs SET message_id=decode(repeat('03',32),'hex') WHERE company_id={f.c}",
        f"DELETE FROM outbox WHERE company_id={f.c}",
        f"DELETE FROM office_company_bindings WHERE company_id={f.c}",
    ]
    initial = f.generation()
    for statement in statements:
        value = sql(f"BEGIN; {statement}; SELECT generation FROM office_authority_generations WHERE company_id={f.c}; ROLLBACK;")
        assert int(value) > initial, statement
    assert f.generation() == initial, "rolled-back mutation must not alter authority"
    print(f"PASS {len(statements)} authoritative mutation paths advance generation atomically")


def test_reader_exclusion(f):
    other = Fixture()
    with Session() as reader:
        generation = int(reader.query(f"SELECT ortak_lock_office_authority({f.c});"))
        for mutation in [
            f"UPDATE channel_members SET removed_at=now() WHERE community_id={f.q}",
            f"UPDATE channels SET visibility='private' WHERE community_id={f.q}",
            f"UPDATE events SET deleted_at=now() WHERE community_id={f.q}",
            f"INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,valid_until) VALUES ({f.c},'fixture','{f.revision}','create',decode(repeat('08',32),'hex'),'credential://test/absent',now()+interval '1 day')",
        ]:
            sql(mutation, error="40001")
        sql(f"UPDATE channels SET visibility='private' WHERE community_id={other.q};")
        reader.query(f.decision(generation))
        reader.query("COMMIT;")
    sql(f"UPDATE channels SET archived_at=now() WHERE community_id={f.q};")
    assert f.generation() > generation
    sql("BEGIN;" + f.decision(generation) + "COMMIT;", error="40001")
    print("PASS after-read mutations fail closed; other company proceeds; stale decision cannot commit")


def test_writer_first_fresh_snapshot(f):
    with Session() as writer, Session() as reader:
        writer.query(f"UPDATE users SET deactivated_at=now() WHERE community_id={f.q};")
        changed = int(writer.query(f"SELECT generation FROM office_authority_generations WHERE company_id={f.c};"))
        reader.send(f"SELECT ortak_lock_office_authority({f.c});")
        # Observe the real backend waiting instead of relying on a timed sleep.
        for _ in range(40):
            waiting = int(sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event='advisory';"))
            if waiting:
                break
        assert waiting, "reader must actually wait for the writer fence"
        writer.query("COMMIT;")
        assert int(reader.read()) == changed
    print("PASS writer-first helper returns post-lock committed generation")


def test_mapping_absence_and_deletion_order(f):
    company, community = str(uuid.uuid4()), str(uuid.uuid4())
    sql(f"INSERT INTO companies(id,slug,display_name) VALUES ('{company}','f-{company}','Unmapped'); INSERT INTO communities(id,host) VALUES ('{community}','f-{community}.local');")
    with Session() as writer:
        writer.query(f"INSERT INTO users(community_id,pubkey) VALUES ('{community}',decode(repeat('04',32),'hex'));")
        sql(f"INSERT INTO office_company_bindings(company_id,community_id) VALUES ('{company}','{community}');", error="40001")
        writer.query("COMMIT;")
    sql(f"INSERT INTO office_company_bindings(company_id,community_id) VALUES ('{company}','{community}');")
    with Session() as deletion:
        deletion.query(f"SELECT pg_advisory_xact_lock(community_deletion_lock_key({f.q}));")
        sql(f"SELECT ortak_lock_office_authority({f.c});", error="40001")
    with Session() as reader:
        reader.query(f"SELECT ortak_lock_office_authority({f.c});")
        assert sql(f"SELECT pg_try_advisory_xact_lock(community_deletion_lock_key({f.q}));") == "f"
    print("PASS absent company mapping cannot race Office inserts; deletion lock ordering fails closed")


def test_noise_and_time(f):
    before = f.generation()
    sql(f"UPDATE channels SET topic='cosmetic' WHERE community_id={f.q}; UPDATE users SET display_name='cosmetic' WHERE community_id={f.q}; UPDATE thread_metadata SET reply_count=reply_count+1 WHERE community_id={f.q}; UPDATE runs SET status='running' WHERE company_id={f.c}; UPDATE outbox SET attempt_count=attempt_count+1 WHERE company_id={f.c};")
    assert f.generation() == before
    sql("BEGIN;" + f.decision(before, "clock_timestamp()+interval '50 milliseconds'") + "SELECT pg_sleep(0.08); COMMIT;", error="40001")
    sql(f"BEGIN; UPDATE runs SET office_admission_token=gen_random_uuid(),office_admission_generation={before},office_admission_valid_before=clock_timestamp()+interval '50 milliseconds' WHERE company_id={f.c}; SELECT pg_sleep(0.08); COMMIT;", error="40001")
    # The old token's same-generation/same-deadline admission may have succeeded
    # earlier. A fresh re-admission token must still check expiry at commit.
    sql(f"UPDATE runs SET office_admission_token=gen_random_uuid(),office_admission_generation={before},office_admission_valid_before=clock_timestamp()+interval '200 milliseconds' WHERE company_id={f.c};")
    sql(f"BEGIN; UPDATE runs SET office_admission_token=gen_random_uuid() WHERE company_id={f.c}; SELECT pg_sleep(0.25); COMMIT;", error="40001")
    sql(f"UPDATE runs SET office_admission_token=NULL WHERE company_id={f.c};", error="23514")
    sql(f"UPDATE runs SET office_admission_token=gen_random_uuid(),office_admission_generation={before},office_admission_valid_before=clock_timestamp()+interval '50 milliseconds' WHERE company_id={f.c}; SELECT pg_sleep(0.08); UPDATE runs SET status='cancelled',cancel_reason='test',finished_at=now() WHERE company_id={f.c};")
    sql(f"UPDATE office_authority_generations SET generation=0 WHERE company_id={f.c};", error="55000")
    sql("TRUNCATE events;", error="55000")
    sql("TRUNCATE ONLY events_p_future;", error="55000")
    with Session() as migration:
        migration.query("SELECT pg_advisory_xact_lock(7094711454081051697::BIGINT);")
        sql(f"SELECT ortak_lock_office_authority({f.c});", error="40001")
    sql(f"BEGIN ISOLATION LEVEL REPEATABLE READ; SELECT ortak_lock_office_authority({f.c});", error="25000")
    print("PASS cosmetic/lifecycle writes do not revoke; deferred decision/admission/re-admission expiry rejects; cancellation survives expiry")


if __name__ == "__main__":
    fixture = Fixture()
    test_mutation_coverage(fixture)
    test_reader_exclusion(fixture)
    test_writer_first_fresh_snapshot(fixture)
    test_mapping_absence_and_deletion_order(fixture)
    test_noise_and_time(fixture)
    print("PASS Office authority PostgreSQL checks completed")
