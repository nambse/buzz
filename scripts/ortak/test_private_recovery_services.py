"""Production-seam regressions for fixture isolation, retained ownership, expiry and S3 semantics."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from backup_private_database import Refused
import rehearse_private_recovery_services as subject
import recovery_minio_client_fixture as s3


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.root.chmod(0o700)
        self.operation = 'a' * 32

    def service(self, kind='redis'):
        volume = {'name': 'ortak_offline_' + self.operation + '_' + kind,
            'labels': {subject.LABEL: self.operation, 'org.ortak.offline_store': kind}}
        secrets = {key: self.root / key for key in ('user', 'password')} if kind == 'minio' else None
        return subject.Service(self.root, self.operation, kind, volume, secrets)

    def test_existing_or_foreign_volume_cannot_gain_fixture_authority(self):
        for volume in [{'name': 'source', 'labels': {}}, {'name': 'ortak_offline_' + self.operation + '_redis',
            'labels': {subject.LABEL: 'b' * 32, 'org.ortak.offline_store': 'redis'}}]:
            with self.assertRaises(Refused): subject.Service(self.root, self.operation, 'redis', volume)

    def test_launch_is_offline_bounded_and_redis_cannot_repair_truncated_aof(self):
        for kind in ('redis', 'minio'):
            args = self.service(kind).launch_args()
            self.assertEqual(args[args.index('--network') + 1], 'none')
            self.assertEqual(args[args.index('--pull') + 1], 'never')
            self.assertIn('--read-only', args); self.assertIn('--memory', args); self.assertIn('--pids-limit', args)
            self.assertNotIn('--publish', args); self.assertNotIn('--privileged', args)
            mounts = [args[index + 1] for index, item in enumerate(args) if item == '--mount']
            self.assertFalse(any('docker.sock' in value for value in mounts))
            self.assertFalse(any(str(subject.inventory.RUNTIME) in value for value in args))
            if kind == 'redis':
                self.assertEqual(args[args.index('--aof-load-truncated') + 1], 'no')
                self.assertEqual(args[args.index('--appendfsync') + 1], 'always')
            else:
                values = [args[index + 1] for index, item in enumerate(args) if item == '--env']
                self.assertTrue(all(value.endswith('=off') or '_FILE=/run/secrets/fixture-minio-' in value for value in values))

    def test_owner_checks_identity_mounts_network_and_exit_before_stop(self):
        service = self.service(); service.container = 'b' * 64
        row = {'id': service.container, 'name': '/' + service.name, 'image': service.image, 'running': True,
            'exit': 0, 'oom': False, 'network': 'none', 'ports': {}, 'readonly': True, 'privileged': False,
            'labels': service.volume['labels'], 'mounts': [{'Type': 'volume', 'Name': service.volume['name'],
                'Destination': '/data', 'RW': True}]}
        with patch.object(service, 'run', return_value=json.dumps(row).encode()): self.assertTrue(service.inspect()['running'])
        for key, value in [('id', 'c' * 64), ('image', 'sha256:' + 'd' * 64), ('network', 'bridge'), ('ports', {'6379': []}),
            ('readonly', False), ('privileged', True), ('mounts', []), ('labels', {})]:
            with self.subTest(key=key), patch.object(service, 'run', return_value=json.dumps({**row, key: value}).encode()):
                with self.assertRaises(Refused): service.inspect()
        with patch.object(service, 'inspect', side_effect=Refused('changed')), patch.object(service, 'run') as run:
            with self.assertRaises(Refused): service.stop_retained()
            run.assert_not_called()

    def test_cold_capture_cannot_read_running_or_unverified_source(self):
        service = self.service()
        with patch.object(service, 'inspect', side_effect=Refused('running')), patch.object(subject.Commands, 'run') as run:
            with self.assertRaises(Refused): subject.cold_archive(service, self.root)
            run.assert_not_called()

    def test_aof_verifier_checks_missing_expired_value_counter_and_absolute_ttl(self):
        expected = {'short_expiry_ms': 1000, 'long_expiry_ms': 300000}
        values = {('PING',): 'PONG', ('TIME',): '2\n0', ('EXISTS', subject.KEYS[1]): '0',
            ('GET', subject.KEYS[0]): 'fixture-value', ('GET', subject.KEYS[2]): 'fixture-value',
            ('GET', subject.KEYS[3]): '2', ('HGET', subject.KEYS[4], 'generation'): '2',
            ('HGET', subject.KEYS[4], 'source'): 'synthetic', ('DBSIZE',): '4',
            ('PTTL', subject.KEYS[2]): '297999', ('PEXPIRETIME', subject.KEYS[2]): '300000',
            ('INFO', 'persistence'): 'aof_enabled:1\naof_last_write_status:ok\naof_last_bgrewrite_status:ok'}
        service = self.service()
        with patch.object(service, 'redis', side_effect=lambda *args: values[args]):
            self.assertTrue(subject.redis_verify(service, expected)['absolute_expiry_preserved'])
        for key, altered in [(('EXISTS', subject.KEYS[1]), '1'), (('GET', subject.KEYS[3]), '4'),
            (('PEXPIRETIME', subject.KEYS[2]), '302000'), (('PTTL', subject.KEYS[2]), '-1')]:
            changed = {**values, key: altered}
            with self.subTest(key=key), patch.object(service, 'redis', side_effect=lambda *args: changed[args]):
                with self.assertRaises(Refused): subject.redis_verify(service, expected)

    def test_curl_secret_config_uses_stdin_only_and_cannot_reach_external_endpoint(self):
        client = s3.Client.__new__(s3.Client); client.working = self.root
        client.config = b'user = "generated:fixture-secret"\n'; client.sequence = 0
        def curl(args, **kwargs):
            self.assertNotIn('fixture-secret', ' '.join(args)); self.assertEqual(kwargs['input'], client.config)
            self.assertEqual(args[-1], 'http://127.0.0.1:9000/fixture')
            self.assertIn('--aws-sigv4', args); self.assertEqual(args[1], '-q')
            self.assertNotIn('MINIO_ROOT_PASSWORD', kwargs['env'])
            Path(args[args.index('--output') + 1]).write_bytes(b'ok')
            Path(args[args.index('--dump-header') + 1]).write_text('HTTP/1.1 200 OK\n')
            return type('Result', (), {'returncode': 0, 'stdout': b'200'})()
        with patch.object(s3.subprocess, 'run', side_effect=curl):
            self.assertEqual(client.request('GET', '/fixture')[0], 200)
            with self.assertRaises(ValueError): client.request('GET', 'http://external/')

    def test_minio_verifier_checks_versions_delete_marker_metadata_and_body(self):
        body = b'fixture'
        expected = {'objects': [{'version_id': version, 'sha256': s3.hashlib.sha256(body).hexdigest(),
            'bytes': len(body), 'generation': version} for version in ('1', '2')], 'delete_marker': '3'}
        prefix = '<Result xmlns="http://s3.amazonaws.com/doc/2006-03-01/">'
        rows = '<Version><VersionId>1</VersionId></Version><Version><VersionId>2</VersionId></Version><DeleteMarker><VersionId>3</VersionId></DeleteMarker>'
        def response(method, path):
            if path.endswith('?versioning'): return 200, (prefix + '<Status>Enabled</Status></Result>').encode(), {}
            if path.endswith('?versions'): return 200, (prefix + rows + '</Result>').encode(), {}
            if '?versionId=' not in path: return 404, b'', {'x-amz-delete-marker': 'true'}
            version = path[-1]
            return 200, body, {'x-amz-meta-fixture-generation': version, 'x-amz-version-id': version}
        client = type('Client', (), {'request': staticmethod(response)})()
        self.assertTrue(s3.verify(client, expected)['body_and_metadata_equal'])
        expected['objects'][0]['sha256'] = '0' * 64
        with self.assertRaises(ValueError): s3.verify(client, expected)


if __name__ == '__main__': unittest.main()
