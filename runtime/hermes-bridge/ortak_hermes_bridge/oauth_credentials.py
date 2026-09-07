"""One explicitly enrolled OAuth store; never discover or borrow host credentials."""
import base64
from contextlib import contextmanager
import fcntl
import json
import hashlib
import math
import os
from pathlib import Path
import selectors
import stat
import subprocess
import sys
import tempfile
import time
from uuid import UUID, uuid4

from .journal import BridgeError

MAX_TOKEN = 8192
MAX_STATE = 32768
MARKER = 'ORTAK_OAUTH_IDENTITY.json'
STATE = 'oauth-state.json'
LOCK = 'oauth.lock'


def token_text(value):
    """Reject malformed credentials without including their value in errors."""
    if (not isinstance(value, str) or not 16 <= len(value) <= MAX_TOKEN
            or any(c.isspace() or ord(c) < 32 for c in value)):
        raise BridgeError('invalid_oauth_credential', 503)
    return value


def token_expiry(value):
    """Read only JWT expiry metadata; this is not signature or provider validation."""
    token_text(value)
    try:
        parts = value.split('.')
        if len(parts) != 3:
            raise ValueError()
        claims = json.loads(base64.urlsafe_b64decode(parts[1] + '=' * (-len(parts[1]) % 4)))
        expiry = claims.get('exp')
        account = claims.get('https://api.openai.com/auth', {}).get('chatgpt_account_id')
        if type(expiry) is not int or not 0 < expiry < 100_000_000_000 or not isinstance(account, str) or not account:
            raise ValueError()
        return expiry
    except (ValueError, TypeError, AttributeError, UnicodeError):
        raise BridgeError('invalid_oauth_credential', 503) from None


def oauth_identity(company, employee, binding):
    """Freeze company, employee, profile and opaque credential ownership together."""
    refs = binding.get('credential_refs')
    if (str(UUID(company)) != company or not isinstance(refs, list) or len(refs) != 1
            or any(not isinstance(x, str) or not 1 <= len(x) <= 256 or '\0' in x
                   for x in (employee, binding.get('profile_ref'), refs[0]))):
        raise BridgeError('invalid_oauth_identity')
    return {'format': 'ortak-oauth-identity/1', 'company_id': company,
            'employee_id': employee, 'profile_ref': binding['profile_ref'],
            'credential_ref': refs[0]}


def private_read(path, maximum=MAX_STATE):
    """Bound reads and require one owner-private nonsymlink regular file."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as file:
            meta = os.fstat(file.fileno())
            if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                    or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1):
                raise BridgeError('oauth_file_permissions', 503)
            data = file.read(maximum + 1)
        if len(data) > maximum:
            raise BridgeError('oauth_state_too_large', 503)
        return json.loads(data)
    except (OSError, ValueError, UnicodeError):
        raise BridgeError('oauth_state_unavailable', 503) from None


def atomic_write(path, value):
    """Publish one private durable snapshot; never truncate the original state."""
    data = json.dumps(value, separators=(',', ':')).encode()
    if len(data) > MAX_STATE:
        raise BridgeError('oauth_state_too_large', 503)
    temporary = path.with_name('.oauth-pending-' + uuid4().hex)
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


class OAuthProcess:
    """Bounded private IPC to the pinned official Hermes refresh implementation."""
    def call(self, action, payload):
        if action != 'refresh':
            raise BridgeError('invalid_oauth_operation')
        environment = {'PATH': '/usr/local/bin:/usr/bin:/bin', 'HOME': '/tmp',
                       'PYTHONPATH': '/opt/bridge:/opt/hermes', 'PYTHONDONTWRITEBYTECODE': '1',
                       'LD_LIBRARY_PATH': '/opt/sqlite-fixed/lib', 'LANG': 'C.UTF-8'}
        process = None
        try:
            with tempfile.TemporaryFile() as incoming:
                data = json.dumps(payload, separators=(',', ':')).encode()
                if len(data) > MAX_STATE:
                    raise BridgeError('oauth_state_too_large', 503)
                incoming.write(data)
                incoming.seek(0)
                process = subprocess.Popen(
                    [sys.executable, '-m', 'ortak_hermes_bridge.oauth_flow', action],
                    env=environment, cwd='/tmp', stdin=incoming, stdout=subprocess.PIPE,
                    stderr=subprocess.DEVNULL)
                deadline = time.monotonic() + 35
                output = bytearray()
                with selectors.DefaultSelector() as ready:
                    ready.register(process.stdout, selectors.EVENT_READ)
                    while True:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not ready.select(remaining):
                            raise BridgeError('oauth_request_uncertain', 503)
                        chunk = os.read(process.stdout.fileno(), MAX_STATE + 1 - len(output))
                        if not chunk:
                            break
                        output.extend(chunk)
                        if len(output) > MAX_STATE:
                            raise BridgeError('oauth_request_uncertain', 503)
                if process.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
                    raise BridgeError('oauth_request_uncertain', 503)
                result = json.loads(output)
                if not isinstance(result, dict):
                    raise BridgeError('oauth_request_uncertain', 503)
                error = result.get('error')
                if error is not None:
                    if error not in {'oauth_relogin_required', 'oauth_retry_later', 'oauth_request_uncertain',
                                     'oauth_provider_unavailable', 'oauth_model_unavailable'}:
                        raise BridgeError('oauth_request_uncertain', 503)
                    raise BridgeError(error, 503)
                return result
        except (OSError, ValueError, UnicodeError, subprocess.TimeoutExpired):
            raise BridgeError('oauth_request_uncertain', 503) from None
        finally:
            if process is not None:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=2)
                process.stdout.close()


class OAuthStore:
    """Keep single-use refresh ownership separate from the readonly worker profile."""
    def __init__(self, directory, identity, driver=None):
        self.directory = Path(directory)
        self.identity = identity
        self.driver = driver if driver is not None else OAuthProcess()
        self.validate_directory()
        if private_read(self.directory / MARKER) != identity:
            raise BridgeError('oauth_identity_mismatch', 503)

    def validate_directory(self):
        """Accept only an exact canonical, private path selected by the controller."""
        root = self.directory
        try:
            meta = root.lstat()
            if (not root.is_absolute() or str(root.resolve()) != str(root) or ',' in str(root)
                    or not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.geteuid()
                    or stat.S_IMODE(meta.st_mode) != 0o700):
                raise BridgeError('oauth_directory_permissions', 503)
        except OSError:
            raise BridgeError('oauth_directory_unavailable', 503) from None

    @classmethod
    def create(cls, directory, identity):
        """Create only a fresh directory; existing directories need our exact marker."""
        root = Path(directory)
        if not root.is_absolute() or str(root.parent.resolve() / root.name) != str(root):
            raise BridgeError('oauth_directory_permissions', 503)
        try:
            root.mkdir(mode=0o700)
        except FileExistsError:
            return cls(root, identity)
        atomic_write(root / MARKER, identity)
        # A failed initial write leaves our marker and a recoverable login target.
        return cls(root, identity)

    @contextmanager
    def locked(self, timeout=3, *, readonly=False):
        """Serialize read-refresh-persist across processes with a bounded wait."""
        self.validate_directory()
        if private_read(self.directory / MARKER) != self.identity:
            raise BridgeError('oauth_identity_mismatch', 503)
        flags = os.O_RDONLY | os.O_NOFOLLOW if readonly else os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW
        try:
            descriptor = os.open(self.directory / LOCK, flags, 0o600)
        except OSError:
            raise BridgeError('oauth_state_unavailable', 503) from None
        try:
            meta = os.fstat(descriptor)
            if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                    or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1):
                raise BridgeError('oauth_file_permissions', 503)
            deadline = time.monotonic() + timeout
            while True:
                try:
                    fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    if time.monotonic() >= deadline:
                        raise BridgeError('oauth_store_busy', 503) from None
                    time.sleep(0.025)
            current = (self.directory / LOCK).lstat()
            if (current.st_ino, current.st_dev) != (meta.st_ino, meta.st_dev):
                raise BridgeError('oauth_lock_changed', 503)
            yield
        finally:
            os.close(descriptor)

    def read(self):
        """Validate stored acquisition and retry state without returning it to HTTP."""
        state = private_read(self.directory / STATE)
        if (not isinstance(state, dict) or set(state) != {'format', 'generation', 'phase', 'tokens', 'retry_at'}
                or state['format'] != 'ortak-oauth/1' or type(state['generation']) is not int
                or state['generation'] < 1 or state['phase'] not in
                {'ready', 'refreshing', 'refresh_uncertain', 'login_required'}
                or type(state['retry_at']) not in {int, float}
                or not math.isfinite(state['retry_at'])):
            raise BridgeError('invalid_oauth_state', 503)
        tokens = state['tokens']
        if not isinstance(tokens, dict) or set(tokens) != {'access_token', 'refresh_token'}:
            raise BridgeError('invalid_oauth_state', 503)
        token_expiry(tokens['access_token'])
        token_text(tokens['refresh_token'])
        return state

    def enroll(self, login):
        """Explicit fresh browser enrollment replaces only this owned store's session."""
        with self.locked():
            previous = self.read() if (self.directory / STATE).exists() else None
            # The caller invokes the official device flow directly, never the
            # ambient-resolving login wrapper or credential pool.
            result = login()
            tokens = result.get('tokens') if isinstance(result, dict) else None
            if not isinstance(tokens, dict) or set(tokens) != {'access_token', 'refresh_token'}:
                raise BridgeError('invalid_oauth_credential', 503)
            if token_expiry(tokens['access_token']) <= time.time() + 180:
                raise BridgeError('oauth_relogin_required', 503)
            token_text(tokens['refresh_token'])
            atomic_write(self.directory / STATE, {'format': 'ortak-oauth/1',
                         'generation': previous['generation'] + 1 if previous else 1,
                         'phase': 'ready', 'tokens': tokens, 'retry_at': 0})

    def access_token(self):
        """Resolve a current selected token; an uncertain rotation requires fresh login."""
        with self.locked():
            state = self.read()
            if state['phase'] != 'ready':
                raise BridgeError('oauth_relogin_required', 503)
            if state['retry_at'] > time.time():
                raise BridgeError('oauth_retry_later', 503)
            if token_expiry(state['tokens']['access_token']) > time.time() + 240:
                return state['tokens']['access_token']
            # Commit the uncertainty fence BEFORE a single-use refresh can leave
            # this machine. A crash cannot silently replay the consumed token.
            state['phase'] = 'refreshing'
            atomic_write(self.directory / STATE, state)
            try:
                updated = self.driver.call('refresh', state['tokens'])
                tokens = {k: updated[k] for k in ('access_token', 'refresh_token')}
                if token_expiry(tokens['access_token']) <= time.time() + 240:
                    raise BridgeError('oauth_request_uncertain', 503)
                token_text(tokens['refresh_token'])
            except Exception as error:
                code = error.code if isinstance(error, BridgeError) else 'oauth_request_uncertain'
                state['phase'] = 'ready' if code == 'oauth_retry_later' else (
                    'login_required' if code == 'oauth_relogin_required' else 'refresh_uncertain')
                state['retry_at'] = time.time() + 60 if code == 'oauth_retry_later' else 0
                atomic_write(self.directory / STATE, state)
                raise BridgeError(code if code in {'oauth_retry_later', 'oauth_relogin_required'}
                                  else 'oauth_request_uncertain', 503) from None
            state.update(tokens=tokens, phase='ready', generation=state['generation'] + 1, retry_at=0)
            atomic_write(self.directory / STATE, state)
            return tokens['access_token']

    def enrolled(self):
        """Check readable selected enrollment, even if access needs explicit refresh."""
        with self.locked(readonly=True):
            return self.read()['phase'] == 'ready'

    def snapshot(self):
        """Read current credential fingerprints only; never refresh or call a provider."""
        with self.locked(readonly=True):
            state = self.read()
            token = state['tokens']['access_token']
            if state['phase'] != 'ready' or token_expiry(token) <= time.time() + 60:
                raise BridgeError('oauth_probe_required', 503)
            middle = token.split('.')[1]
            claims = json.loads(base64.urlsafe_b64decode(middle + '=' * (-len(middle) % 4)))
            account = claims['https://api.openai.com/auth']['chatgpt_account_id']
            return {'oauth_generation': state['generation'],
                    'access_sha256': hashlib.sha256(token.encode()).hexdigest(),
                    'account_sha256': hashlib.sha256(account.encode()).hexdigest()}
