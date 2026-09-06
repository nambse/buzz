#!/usr/bin/env python3
"""Installed capture-reader xattr guard fixture; one fresh volume, no source mounts or credentials."""

import argparse
import json
import tarfile
from uuid import uuid4

from backup_private_database import Commands, Refused, private_directory
from prepare_private_recovery import save
import private_recovery_inventory as inventory
from private_recovery_offline_stores import fresh_volume, image_present, LABEL
import private_recovery_payloads as payload
import recovery_archive_io as archive_io


def execute():
    """Preserve every positive/negative archive and exact stopped reader identity."""
    operation = uuid4().hex
    root = private_directory(inventory.EVIDENCE / ('g-volume-reader-guards-' + operation), fresh=True)
    command = Commands(root); image = inventory.SERVICES['controller'][2]
    image_present(command, image, 'reader-image')
    volume = fresh_volume(command, operation, 'minio')
    report = {'status': 'started', 'source_access': False, 'fixture_only': True,
        'image': image, 'volume': volume, 'cases': [], 'network': 'none', 'credentials': False}
    def args(name, code, readonly):
        return command.docker('run', '--pull', 'never', '--name', name,
            '--label', LABEL + '=' + operation, '--network', 'none', '--read-only',
            '--user', '0:0', '--cap-drop', 'ALL', '--cap-add', 'DAC_OVERRIDE',
            '--security-opt', 'no-new-privileges', '--pids-limit', '16', '--memory', '64m',
            '--mount', 'type=volume,source=' + volume['name'] + ',target=/capture-source,'
                + ('readonly,' if readonly else '') + 'volume-nocopy',
            '--entrypoint', '/usr/local/bin/python', image, '-u', '-c', code, '1024')
    try:
        for index, (label, attribute, size, expected) in enumerate([
            ('both-eight', 'user.total_deletes', 8, None),
            ('unknown-name', 'user.fixture_unreviewed', 8, 'unreviewed_xattr'),
            ('delete-seven', 'user.total_deletes', 7, 'xattr_bound'),
            ('delete-nine', 'user.total_deletes', 9, 'xattr_bound'),
            ('write-seven', 'user.total_writes', 7, 'xattr_bound'),
            ('write-nine', 'user.total_writes', 9, 'xattr_bound'),
        ]):
            setup = "import os;from pathlib import Path;p=Path('/capture-source/format.json');p.write_bytes(b'fixture');p.chmod(0o600);" \
                + "[os.removexattr(p,key) for key in os.listxattr(p)];" \
                + "os.setxattr(p,'user.total_writes',b'1'*8);os.setxattr(p,'user.total_deletes',b'2'*8);" \
                + 'os.setxattr(p,' + repr(attribute) + ',b\'3\'*' + str(size) + ')'
            command.run('setup-' + label, args('ortak-reader-guard-' + operation + '-setup-' + str(index), setup, False), ceiling=128)
            name = 'ortak-reader-guard-' + operation + '-' + str(index)
            archive = root / (label + '.tar')
            try:
                command.run(label, args(name, payload.VOLUME_READER, True), output=archive, ceiling=65536)
                inventory.require(expected is None, 'reader_guard_did_not_refuse')
            except Refused:
                inventory.require(expected is not None, 'reader_guard_positive_refused')
                diagnostic = payload.volume_reader_failure(root / (label + '.stderr'), 'minio')
                inventory.require(diagnostic is not None and diagnostic['code'] == expected, 'reader_guard_diagnostic_changed')
            fmt = '{"id":{{json .Id}},"image":{{json .Image}},"pid":{{json .State.Pid}},"running":{{json .State.Running}},"exit":{{json .State.ExitCode}}}'
            owner = json.loads(command.run(label + '-owner', command.docker('inspect', '--format', fmt, name)))
            inventory.require(owner['image'] == image and owner['pid'] == 0 and owner['running'] is False
                and owner['exit'] == (3 if expected else 0), 'reader_guard_containment_failed')
            if expected is None:
                with tarfile.open(archive) as reader:
                    attrs = archive_io.archive_xattrs(reader.getmember('format.json').pax_headers)
                    inventory.require(set(attrs) == set(archive_io.XATTRS), 'reader_guard_xattrs_missing')
            report['cases'].append({'case': label, 'expected_refusal': expected, 'reader': owner, 'passed': True})
        report['status'] = 'passed'
    except Exception as error:
        report.update(status='failed', error_type=type(error).__name__)
        raise
    finally:
        save(root / 'receipt.json', report)
    return root


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-owned-fixture', action='store_true', required=True)
    parser.parse_args()
    try:
        root = execute()
        print(json.dumps({'status': 'passed', 'receipt': str(root / 'receipt.json'), 'source_access': False}))
    except Exception:
        raise SystemExit('Installed reader fixture failed; generated artifacts retained; no source access.') from None
