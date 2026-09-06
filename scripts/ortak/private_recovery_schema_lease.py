"""Bounded main PostgreSQL schema fence held for the complete recovery capture interval."""

from contextlib import contextmanager
import json
import os
import re
import selectors
import subprocess
import time

from backup_private_database import Commands, environment
from private_recovery_inventory import require


def response(process, command):
    """Read one bounded structured lease reply without exposing child diagnostics."""
    value = bytearray()
    with selectors.DefaultSelector() as ready:
        ready.register(process.stdout, selectors.EVENT_READ)
        while b'\n' not in value:
            require(ready.select(min(5, command.remaining())), 'lease_reply_timeout')
            block = os.read(process.stdout.fileno(), 8193 - len(value))
            require(block and len(value) + len(block) <= 8192, 'lease_reply_refused')
            value.extend(block)
    return json.loads(value)


class SchemaCommands(Commands):
    """Use the existing fixed main connection with a900-second outer lease, finite SQL limits."""

    def __init__(self, root):
        super().__init__(root)
        self.deadline = time.monotonic() + 900

    def command(self, program, *args):
        command = super().command(program, *args)
        return ['PGOPTIONS=-c lock_timeout=2000 -c statement_timeout=60000 -c idle_in_transaction_session_timeout=900000'
                if part.startswith('PGOPTIONS=') else part for part in command]


@contextmanager
def held_schema(root):
    """Export one source snapshot while holding the same advisory shared fence used by migrations."""
    command = SchemaCommands(root)
    source = command.inspect()
    process = subprocess.Popen(command.psql('ortak'), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, env=environment(), start_new_session=True)
    try:
        process.stdin.write(b"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\nDO $$BEGIN IF NOT pg_try_advisory_xact_lock_shared(7094711454081051697) THEN RAISE EXCEPTION 'schema fence busy'; END IF; END$$;\nSELECT jsonb_build_object('snapshot',pg_export_snapshot(),'backend_pid',pg_backend_pid(),'backend_start',(SELECT backend_start FROM pg_stat_activity WHERE pid=pg_backend_pid()));\n")
        process.stdin.flush()
        witness = response(process, command)
        require(set(witness) == {'snapshot', 'backend_pid', 'backend_start'}
            and type(witness['backend_pid']) is int and witness['backend_pid'] > 0
            and isinstance(witness['backend_start'], str) and len(witness['backend_start']) <= 64
            and re.fullmatch(r'[0-9A-F]{8}-[0-9A-F]{8}-[0-9]+', witness['snapshot']), 'schema_lease_identity_refused')
        yield {**witness, 'container_id': source['container_id']}
        require(process.poll() is None, 'schema_lease_lost')
        process.stdin.write(b'ROLLBACK;\n\\q\n'); process.stdin.flush()
        require(process.wait(timeout=min(3, command.remaining())) == 0, 'schema_lease_release_failed')
    finally:
        command.stop(process)
