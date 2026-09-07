"""Isolated loopback S3 fixture using installed curl's AWS signer, never custom signing or live keys."""

import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from urllib.parse import quote
import xml.etree.ElementTree as XML

BUCKET = 'ortak-offline-recovery-fixture'
OBJECT = 'versioned-fixture'
NAMESPACE = '{http://s3.amazonaws.com/doc/2006-03-01/}'
PHASE = 'starting'
LAST_HTTP = None


def require(value):
    if not value: raise ValueError('fixture_refused')


class Client:
    """The only endpoint is this new fixture's localhost, in its network-none namespace."""

    def __init__(self, working):
        self.working = Path(working)
        user = Path('/run/secrets/fixture-minio-user').read_text().strip()
        password = Path('/run/secrets/fixture-minio-password').read_text().strip()
        require(re.fullmatch(r'[0-9a-f]{32}', user) and re.fullmatch(r'[0-9a-f]{64}', password))
        self.config = ('user = "' + user + ':' + password + '"\n').encode()
        self.sequence = 0

    def request(self, method, path, body=b'', headers=(), authenticated=True):
        """Credential config travels only over stdin; bounded response files are never printed."""
        global LAST_HTTP
        require(method in ('GET', 'PUT', 'DELETE') and path.startswith('/') and len(path) <= 512
            and len(body) <= 4096 and self.sequence < 64)
        self.sequence += 1
        output, header, request = [self.working / (name + str(self.sequence)) for name in ('out', 'headers', 'request')]
        request.write_bytes(body)
        args = ['/usr/bin/curl', '-q', '--silent', '--noproxy', '*', '--proto', '=http',
            '--connect-timeout', '2', '--max-time', '5', '--max-filesize', '131072',
            '--output', str(output), '--dump-header', str(header), '--write-out', '%{http_code}',
            '--request', method, '--header', 'x-amz-content-sha256:' + hashlib.sha256(body).hexdigest()]
        if authenticated: args += ['--aws-sigv4', 'aws:amz:us-east-1:s3', '--config', '-']
        if method == 'PUT': args += ['--data-binary', '@' + str(request)]
        for value in headers: args += ['--header', value]
        args.append('http://127.0.0.1:9000' + path)
        result = subprocess.run(args, input=self.config if authenticated else b'', stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=7, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'HOME': '/nonexistent'})
        require(result.returncode == 0 and re.fullmatch(rb'[0-9]{3}', result.stdout))
        LAST_HTTP = int(result.stdout)
        require(output.stat().st_size <= 131072 and header.stat().st_size <= 16384)
        values = {}
        for line in header.read_text().splitlines():
            if ':' in line:
                key, value = line.split(':', 1); values[key.lower()] = value.strip()
        return LAST_HTTP, output.read_bytes(), values

    def ready(self):
        """Only health reads retry, with a finite total attempt count; S3 writes never retry implicitly."""
        for _ in range(25):
            try:
                if self.request('GET', '/minio/health/ready', authenticated=False)[0] == 200: return
            except (OSError, ValueError, subprocess.SubprocessError): pass
            time.sleep(0.2)
        raise ValueError('fixture_not_ready')


def verify(client, expected):
    """Read exact original version IDs, metadata, body bytes and the retained delete marker."""
    global PHASE
    PHASE = 'verify_versioning'
    code, body, _ = client.request('GET', '/' + BUCKET + '?versioning')
    require(code == 200 and XML.fromstring(body).findtext(NAMESPACE + 'Status') == 'Enabled')
    PHASE = 'verify_versions'
    code, body, _ = client.request('GET', '/' + BUCKET + '?versions')
    require(code == 200)
    parsed = XML.fromstring(body)
    versions = [row.findtext(NAMESPACE + 'VersionId') for row in parsed.findall(NAMESPACE + 'Version')]
    markers = [row.findtext(NAMESPACE + 'VersionId') for row in parsed.findall(NAMESPACE + 'DeleteMarker')]
    require(set(versions) == {row['version_id'] for row in expected['objects']}
        and markers == [expected['delete_marker']] and len(versions) == 2)
    PHASE = 'verify_latest_deleted'
    code, _, headers = client.request('GET', '/' + BUCKET + '/' + OBJECT)
    require(code == 404 and headers.get('x-amz-delete-marker') == 'true')
    PHASE = 'verify_object_bytes'
    for row in expected['objects']:
        code, body, headers = client.request('GET', '/' + BUCKET + '/' + OBJECT + '?versionId=' + quote(row['version_id'], safe=''))
        require(code == 200 and hashlib.sha256(body).hexdigest() == row['sha256'] and len(body) == row['bytes']
            and headers.get('x-amz-meta-fixture-generation') == row['generation']
            and headers.get('x-amz-version-id') == row['version_id'])
    return {'versions': 2, 'delete_markers': 1, 'latest_is_deleted': True,
            'body_and_metadata_equal': True, 'versioning_enabled': True}


def run(mode, expected=None):
    """Seed only an explicitly new fixture, or read only its fresh restored copy."""
    global PHASE
    require(sys.platform == 'linux' and not Path('/var/run/docker.sock').exists() and mode in ('seed', 'verify'))
    with tempfile.TemporaryDirectory(prefix='minio-fixture-') as working:
        client = Client(working); client.ready()
        if mode == 'seed':
            PHASE = 'create_fixture_bucket'
            require(client.request('PUT', '/' + BUCKET)[0] == 200)
            PHASE = 'enable_versioning'
            body = b'<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>Enabled</Status></VersioningConfiguration>'
            md5 = base64.b64encode(hashlib.md5(body).digest()).decode()
            require(client.request('PUT', '/' + BUCKET + '?versioning', body,
                ['Content-Type:application/xml', 'Content-MD5:' + md5])[0] == 200)
            objects = []
            for generation in ('1', '2'):
                PHASE = 'write_fixture_version_' + generation
                body = ('offline fixture version ' + generation).encode()
                code, _, headers = client.request('PUT', '/' + BUCKET + '/' + OBJECT, body,
                    ['Content-Type:text/plain', 'x-amz-meta-fixture-generation:' + generation])
                require(code == 200 and re.fullmatch(r'[A-Za-z0-9._-]{1,128}', headers.get('x-amz-version-id', '')))
                objects.append({'version_id': headers['x-amz-version-id'], 'generation': generation,
                    'sha256': hashlib.sha256(body).hexdigest(), 'bytes': len(body)})
            require(objects[0]['version_id'] != objects[1]['version_id'])
            PHASE = 'write_temporary_version_for_permanent_delete'
            code, _, headers = client.request('PUT', '/' + BUCKET + '/' + OBJECT, b'temporary fixture version')
            version = headers.get('x-amz-version-id', '')
            require(code == 200 and re.fullmatch(r'[A-Za-z0-9._-]{1,128}', version))
            PHASE = 'permanently_delete_exact_temporary_version'
            path = '/' + BUCKET + '/' + OBJECT + '?versionId=' + quote(version, safe='')
            require(client.request('DELETE', path)[0] == 204)
            require(client.request('GET', path)[0] == 404)
            PHASE = 'write_fixture_delete_marker'
            code, _, headers = client.request('DELETE', '/' + BUCKET + '/' + OBJECT)
            require(code == 204 and headers.get('x-amz-delete-marker') == 'true')
            expected = {'objects': objects, 'delete_marker': headers['x-amz-version-id']}
        require(set(expected) == {'objects', 'delete_marker'})
        verified = verify(client, expected)
        return {'status': 'verified', 'expected': expected, 'verification': verified,
                'real_source_keys': False, 'writes_performed': mode == 'seed'}


if __name__ == '__main__':
    try:
        raw = sys.stdin.buffer.read(8193)
        require(len(raw) <= 8192)
        request = json.loads(raw)
        print(json.dumps(run(request['mode'], request.get('expected'))))
    except Exception:
        print(json.dumps({'status': 'refused', 'phase': PHASE, 'http_status': LAST_HTTP}))
        raise SystemExit(3) from None
