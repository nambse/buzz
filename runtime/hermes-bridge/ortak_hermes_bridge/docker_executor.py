"""Candidate Docker containment owner; activation requires explicit image validation.

This module is not selected by the default CLI. Never enable it based solely on
image labels: the exact digest's real guard/cancel/restart smoke is a release gate.
"""
import fcntl
import hashlib
import json
import os
import re
import selectors
import stat
import subprocess
import threading
import tempfile
import time
from pathlib import Path

from . import HERMES_REVISION
from .journal import BridgeError, identity

MAX_ACTIVE = 4
MAX_SECONDS = 180


def container_name(key):
    """Deterministic server-owned name survives lost launch receipts."""
    identity(key)
    return 'ortak-run-' + hashlib.sha256(key.encode()).hexdigest()


class DockerEngine:
    """Narrow fixed-argument Docker CLI port; no shell or model-created arguments."""
    def __init__(self, binary='/usr/bin/docker'):
        self.binary = binary

    def command(self, args):
        """Cap inspect output while reading, with a bounded process deadline."""
        process = None
        try:
            process = subprocess.Popen([self.binary, *args], stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            deadline = time.monotonic() + 5
            output = bytearray()
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not ready.select(remaining):
                        raise BridgeError('container_engine_unavailable', 503)
                    chunk = os.read(process.stdout.fileno(), 1025 - len(output))
                    if not chunk:
                        break
                    output.extend(chunk)
                    if len(output) > 1024:
                        raise BridgeError('container_engine_invalid_response', 503)
            code = process.wait(timeout=max(0.001, deadline - time.monotonic()))
            return code, output.decode('utf-8', errors='strict').strip()
        except (OSError, UnicodeError, subprocess.TimeoutExpired):
            raise BridgeError('container_engine_unavailable', 503) from None
        finally:
            if process is not None:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=2)
                if process.stdout is not None:
                    process.stdout.close()

    def validated_image(self, image):
        """Verify the image is present and carries the selected source revision."""
        code, value = self.command(['image', 'inspect', '--format', '{{index .Config.Labels "org.ortak.hermes.revision"}}', image])
        return code == 0 and value == HERMES_REVISION

    def launch(self, args, payload):
        """Spawn one container; output is discarded by CLI and Docker log driver."""
        try:
            # Prewritten anonymous input avoids a blocking pipe write when the
            # daemon stalls before attaching container stdin. No input goes in argv.
            with tempfile.TemporaryFile() as incoming:
                incoming.write(payload)
                incoming.seek(0)
                return subprocess.Popen([self.binary, *args], stdin=incoming,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except (OSError, BrokenPipeError):
            raise BridgeError('container_launch_failed', 503) from None

    def stopped(self, name):
        """Absence must be established by a successful daemon list, not inspect404."""
        code, value = self.command(['container', 'ls', '--all', '--filter', f'name=^/{name}$', '--format', '{{.Names}}'])
        if code != 0:
            return False
        if not value:
            return True
        if value != name:
            return False
        code, running = self.command(['container', 'inspect', '--format', '{{.State.Running}}', name])
        return code == 0 and running == 'false'

    def owned_keys(self, company):
        """Bounded inventory of this company's explicitly labeled execution containers."""
        code, value = self.command(['container', 'ls', '--all', '--filter',
                                    f'label=org.ortak.company={company}', '--filter',
                                    'label=org.ortak.start_key', '--format',
                                    '{{.Label "org.ortak.start_key"}}'])
        if code != 0:
            raise BridgeError('container_inventory_unavailable', 503)
        keys = value.splitlines() if value else []
        if len(keys) > 8 or any(identity(key)[0] != company for key in keys):
            raise BridgeError('invalid_container_inventory', 503)
        return keys

    def stop(self, key, image):
        """Verify exact ownership and image BEFORE any destructive engine operation."""
        company, _ = identity(key)
        name = container_name(key)
        code, listed = self.command(['container', 'ls', '--all', '--filter', f'name=^/{name}$', '--format', '{{.Names}}'])
        if code != 0:
            return False
        if not listed:
            return True
        if listed != name:
            return False
        template = '{{.Name}}|{{index .Config.Labels "org.ortak.company"}}|{{index .Config.Labels "org.ortak.start_key"}}|{{.Config.Image}}'
        code, metadata = self.command(['container', 'inspect', '--format', template, name])
        if code != 0:
            return self.stopped(name)
        if metadata != f'/{name}|{company}|{key}|{image}':
            return False
        self.command(['container', 'rm', '--force', name])
        return self.stopped(name)


class DockerExecutor:
    """Bounded process owner for a dedicated journal and disposable profiles only."""
    def __init__(self, journal, company_id, profiles, image, network, engine=None, *, validated_digest=None):
        self.available = False
        if not re.fullmatch(r'(?:[^\s@]+@)?sha256:[0-9a-f]{64}', image):
            raise BridgeError('image_digest_required')
        if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]{0,62}', network) or network in {'host', 'bridge', 'none'}:
            raise BridgeError('private_network_required')
        # Explicit deployment gate refers to the exact immutable image, not a
        # version string. The default service never sets this value.
        if validated_digest != image:
            raise BridgeError('executor_validation_required', 503)
        self.journal, self.company_id = journal, company_id
        self.profiles, self.image, self.network = profiles, image, network
        self.engine = engine if engine is not None else DockerEngine()
        self.lock = threading.RLock()
        self.running = {}
        self.shutdown = threading.Event()
        self.state_dir = Path(journal.path).resolve().parent
        if ',' in str(self.state_dir):
            raise BridgeError('invalid_state_directory')
        self.owner_file = open(self.state_dir / 'executor.lock', 'a+b')
        try:
            fcntl.flock(self.owner_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            self.owner_file.close()
            raise BridgeError('executor_already_owned', 503) from None
        try:
            if not self.engine.validated_image(image):
                raise BridgeError('image_revision_mismatch', 503)
            for profile in self.profiles:
                self.validate_profile(profile)
            # Include terminal journals: completion or launch failure is not
            # proof that the container process tree has stopped.
            for key in self.engine.owned_keys(company_id):
                if not self.journal.has_start(key):
                    raise BridgeError('orphan_container_without_registry', 503)
                self.journal.request_cancel(key)
                if not self.engine.stop(key, self.image):
                    raise BridgeError('execution_owner_not_stopped', 503)
                self.journal.finish_cancel(key)
            # A prior container may exist even if its start receipt was lost.
            # Tombstones are committed BEFORE attempting external containment.
            for key, status in self.journal.unsettled():
                if identity(key)[0] != company_id:
                    raise BridgeError('journal_company_mismatch', 503)
                self.journal.request_cancel(key)
                if not self.engine.stop(key, self.image):
                    raise BridgeError('execution_owner_not_stopped', 503)
                # Recovery records cancellation, never blindly restarts work.
                self.journal.finish_cancel(key)
            self.available = True
        except BaseException:
            self.owner_file.close()
            raise
        self.monitor = threading.Thread(target=self._monitor, daemon=True)
        self.monitor.start()

    def validate_profile(self, profile):
        """Require an explicit disposable marker and exact employee/company ownership."""
        root = Path(profile['directory'])
        if not root.is_absolute() or root.is_symlink() or str(root.resolve()) != str(root) or ',' in str(root):
            raise BridgeError('invalid_profile_directory')
        required = {'ORTAK_DISPOSABLE_PROFILE.json', 'ORTAK_RUNTIME_BINDING.json',
                    'ORTAK_PROVIDER.json', 'provider-token'}
        try:
            children = list(root.iterdir())
        except OSError:
            raise BridgeError('disposable_profile_required', 503) from None
        if {p.name for p in children} != required or any(p.is_symlink() or not p.is_file() or p.stat().st_size > 8192 for p in children):
            raise BridgeError('unexpected_profile_contents', 503)
        def read(name, maximum=8192):
            fd = os.open(root / name, os.O_RDONLY | os.O_NOFOLLOW)
            with os.fdopen(fd, 'rb') as file:
                metadata = os.fstat(file.fileno())
                if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != root.stat().st_uid:
                    raise BridgeError('profile_file_ownership_mismatch', 503)
                value = file.read(maximum + 1)
            if len(value) > maximum:
                raise BridgeError('profile_file_too_large', 503)
            return value
        try:
            data = json.loads(read('ORTAK_DISPOSABLE_PROFILE.json'))
            binding = json.loads(read('ORTAK_RUNTIME_BINDING.json'))
            provider = json.loads(read('ORTAK_PROVIDER.json'))
            token = read('provider-token', 4096).decode().strip()
        except (OSError, ValueError):
            raise BridgeError('disposable_profile_required', 503) from None
        if data != {'company_id': self.company_id, 'employee_id': profile['employee_id'], 'profile_ref': profile['binding']['profile_ref']}:
            raise BridgeError('profile_ownership_mismatch', 503)
        if binding != profile['binding'] or not isinstance(provider, dict) or set(provider) != {'provider', 'credential_ref'} or provider['provider'] not in {'openai', 'openrouter'} or binding['credential_refs'] != [provider['credential_ref']]:
            raise BridgeError('profile_configuration_mismatch', 503)
        if not token or any(c.isspace() for c in token):
            raise BridgeError('invalid_provider_credential', 503)
        return root

    def inspect(self, binding):
        """Validate local profile contents; not an OAuth/remote-provider health proof."""
        if not self.available:
            return False
        matches = [p for p in self.profiles if p['binding'] == binding]
        if len(matches) != 1:
            return False
        self.validate_profile(matches[0])
        return True

    def start(self, spec, journal):
        """Start after durable reservation; child gates again before Hermes import."""
        key = spec['idempotency_key']
        if identity(key)[0] != self.company_id or journal.path != self.journal.path:
            raise BridgeError('run_not_found', 404)
        with self.lock:
            if not self.available or len(self.running) >= MAX_ACTIVE:
                raise BridgeError('executor_capacity', 503)
            profile = next((p for p in self.profiles if p['employee_id'] == spec['employee_id'] and p['binding'] == spec['binding']), None)
            if profile is None:
                raise BridgeError('profile_not_found', 404)
            root = self.validate_profile(profile)
            state_name = Path(journal.path).name
            if not re.fullmatch(r'[a-zA-Z0-9_.-]+', state_name):
                raise BridgeError('invalid_journal_name')
            args = ['run', '--rm', '--init', '--interactive', '--name', container_name(key),
                    '--label', f'org.ortak.company={self.company_id}',
                    '--label', f'org.ortak.start_key={key}',
                    '--network', self.network, '--entrypoint', 'python', '--read-only', '--cap-drop', 'ALL',
                    '--security-opt', 'no-new-privileges', '--pids-limit', '64',
                    '--memory', '1g', '--cpus', '1', '--log-driver', 'none',
                    '--user', '10001:10001', '--tmpfs', '/tmp:rw,noexec,nosuid,size=134217728',
                    '--workdir', '/tmp', '--env', 'HOME=/tmp/hermes-home', '--env', 'HERMES_HOME=/tmp/hermes-home',
                    '--mount', f'type=bind,src={root},dst=/profile,readonly',
                    '--mount', f'type=bind,src={self.state_dir},dst=/ortak-state',
                    self.image, '-m', 'ortak_hermes_bridge.worker',
                    '--journal', f'/ortak-state/{state_name}']
            payload = json.dumps({'company_id': self.company_id, 'spec': spec}, separators=(',', ':')).encode()
            if len(payload) > 256 * 1024:
                raise BridgeError('body_too_large', 413)
            process = self.engine.launch(args, payload)
            self.running[key] = (process, time.monotonic())

    def stop(self, key):
        """Report terminal acknowledgement only after whole-container stop and CLI reap."""
        with self.lock:
            if not self.engine.stop(key, self.image):
                return False
            owned = self.running.pop(key, None)
            if owned:
                process = owned[0]
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
            return True

    def _monitor(self):
        while not self.shutdown.wait(0.25):
            try:
                with self.lock:
                    for key, (process, started) in list(self.running.items()):
                        expired = time.monotonic() - started >= MAX_SECONDS
                        if process.poll() is None and not expired:
                            continue
                        # Even successful child completion is checked for leaked
                        # containment before relinquishing this owner's inventory.
                        if not self.stop(key):
                            self.available = False
                            continue
                        if expired:
                            self.journal.fail(key, 'deadline_exceeded')
                        else:
                            self.journal.fail(key, 'executor_interrupted')
            except Exception:
                # Durable records remain unsettled for recovery; never acknowledge
                # failure as success or keep accepting work with a broken owner.
                self.available = False
                return

    def close(self):
        """Tombstone then stop all owned work before releasing exclusive ownership."""
        self.available = False
        self.shutdown.set()
        self.monitor.join(timeout=2)
        with self.lock:
            for key in list(self.running):
                self.journal.request_cancel(key)
                if not self.stop(key):
                    raise BridgeError('execution_owner_not_stopped', 503)
                self.journal.finish_cancel(key)
        self.owner_file.close()
