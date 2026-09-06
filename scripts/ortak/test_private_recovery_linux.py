#!/usr/bin/env python3
"""Explicit opt-in Linux flock fixture using only newly generated empty local directories."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from uuid import uuid4

from backup_private_database import Commands, Refused, environment, private_directory
from check_private_recovery_gate import response
from prepare_private_recovery import save
import private_recovery_inventory as inventory
import recovery_lock_holder
import private_recovery_journal as selected_journal

SETUP = r"""
import os,sqlite3,shutil
from pathlib import Path
root=Path('/private/tmp/ortak-hermes-v0-private-20260905')
for p in (root/'state', root/'oauth', root/'oauth/ada-private'):
 p.mkdir(mode=0o700,exist_ok=True)
for p in (root/'state/executor.lock',root/'oauth/ada-private/oauth.lock'):
 p.touch(mode=0o600,exist_ok=False)
db=sqlite3.connect(root/'state/staging.sqlite')
db.execute('PRAGMA journal_mode=WAL')
db.executescript("CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT,sequence INTEGER);INSERT INTO runs VALUES('fixture','completed',1);INSERT INTO events VALUES('fixture',1);")
db.commit()
shutil.copyfile(root/'state/staging.sqlite',root/'state/journal.sqlite')
shutil.copyfile(root/'state/staging.sqlite-wal',root/'state/journal.sqlite-wal')
db.close()
(root/'state/staging.sqlite').unlink()
assert not (root/'state/journal.sqlite-shm').exists()
for p in (root/'state/journal.sqlite',root/'state/journal.sqlite-wal',root/'state/executor.lock',root/'oauth/ada-private/oauth.lock'):
 p.chmod(0o600);os.chown(p,10001,10001)
for p in (root/'state',root/'oauth/ada-private',root/'oauth'):
 p.chmod(0o700);os.chown(p,10001,10001)
os.setgid(10001);os.setuid(10001)
"""


def docker_args(command, output, name, image, script, *, setup=False):
    """No production path is mounted: source directories are fresh fixture-only state/oauth."""
    args = command.docker('run', '--pull', 'never', '--init', '--name', name,
        '--label', 'org.ortak.recovery_fixture=' + output.name,
        '--network', 'none', '--read-only', '--user', '0:0' if setup else '10001:10001',
        '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges', '--memory', '256m', '--pids-limit', '16',
        '--tmpfs', '/recovery-working:rw,noexec,nosuid,nodev,size=150m,mode=700,uid=10001,gid=10001')
    if setup:
        for cap in ['CHOWN', 'DAC_OVERRIDE', 'SETUID', 'SETGID']:
            args += ['--cap-add', cap]
    for relative in ['state', 'oauth']:
        args += ['--mount', 'type=bind,source=' + str(output / relative) + ',target='
                 + str(inventory.RUNTIME / relative) + ('' if setup else ',readonly')]
    return args + ['--entrypoint', '/usr/local/bin/python', '-i', image, '-u', '-c', script]


def execute():
    """Hold, contend and release real Linux locks on fresh fixtures; retain every result/container."""
    inventory.directory(inventory.STATE)
    parent = private_directory(inventory.STATE / 'recovery-linux-fixtures')
    output = private_directory(parent / uuid4().hex, fresh=True)
    for relative in ['state', 'oauth']:
        private_directory(output / relative, fresh=True)
    command = Commands(output)
    image = inventory.SERVICES['controller'][2]
    actual = command.run('image', command.docker('image', 'inspect', '--format', '{{.Id}}', image), ceiling=128).decode().strip()
    inventory.require(actual == image, 'fixture_image_mismatch')
    script = selected_journal.lease_script(recovery_lock_holder).decode()
    fixture_prefix = 'ortak-recovery-fixture-' + output.name
    names = [fixture_prefix + '-' + suffix for suffix in ['holder', 'contender', 'released']]
    manifest = {'format': 'ortak-private-recovery-linux-fixture/1', 'status': 'started',
        'image': image, 'container_names': names, 'fixture_directory': str(output),
        'source_mounts': [str(output / 'state'), str(output / 'oauth')],
        'production_mounts': False, 'provider_credentials': False, 'network': 'none', 'docker_socket': False,
        'script_sha256': hashlib.sha256(script.encode()).hexdigest()}
    save(output / 'intent.json', manifest)
    process = None
    try:
        process = subprocess.Popen(docker_args(command, output, names[0], image, SETUP + '\n' + script, setup=True),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=environment(), start_new_session=True)
        held = response(process, command)
        inventory.require(held['status'] == 'held' and held['journal'] == {'runs': 1, 'nonterminal': 0, 'invalid_cursors': 0},
                          'fixture_lease_not_held')
        # Import the exact script as definitions only: never execute its main
        # and never import the real controller/provider/application module.
        probe = "namespace={'__name__':'fixture'}\nexec(" + repr(script) + ",namespace)\n" + r"""
import json
try:
 with namespace['held_locks'](namespace['RUNTIME']): pass
 print(json.dumps({'blocked':False}))
except BlockingIOError:
 print(json.dumps({'blocked':True}))
"""
        contention = json.loads(command.run('contender', docker_args(command, output, names[1], image, probe), ceiling=1024))
        inventory.require(contention == {'blocked': True}, 'linux_lock_contention_not_proven')
        process.stdin.write(b'release\n'); process.stdin.flush()
        inventory.require(response(process, command) == {'status': 'released'}, 'fixture_release_failed')
        inventory.require(process.wait(timeout=5) == 0, 'fixture_holder_failed')
        released = json.loads(command.run('released', docker_args(command, output, names[2], image, probe), ceiling=1024))
        inventory.require(released == {'blocked': False}, 'linux_lock_release_not_proven')
        inventory.require((output / 'state/journal.sqlite-wal').exists()
            and not (output / 'state/journal.sqlite-shm').exists(), 'source_wal_shm_fixture_changed')
        manifest.update(status='verified', contention_blocked=True, release_reacquired=True,
                        journal_counters=held['journal'], cold_wal_without_source_shm=True,
                        host_linux_interoperability_tested=False)
    except (Refused, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        manifest['status'] = 'failed'
        save(output / 'manifest.json', manifest)
        raise Refused('linux_fixture_failed_retained') from None
    finally:
        if process is not None: command.stop(process)
    save(output / 'manifest.json', manifest)
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-fresh-fixture', action='store_true', required=True)
    parser.parse_args()
    try:
        output = execute()
        print(json.dumps({'status': 'verified', 'manifest': str(output / 'manifest.json'), 'production_mounts': False}))
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Linux lease fixture refused; fresh private artifacts retained. No production state mounted.') from None
