"""Single-run child entry for the reviewed-image containment candidate.

Never run this on the owner's desktop. Its files and dependencies are resolved
only from fixed container paths, and no Hermes source is downloaded at runtime.
"""
import argparse
import inspect
import json
import logging
import os
import signal
import sys
from pathlib import Path
from . import HERMES_REVISION
from .journal import BridgeError, Journal, identity
from .service import Bridge
from .hermes_candidate import execute_candidate
from .verify_source import verify_source
from .oauth_credentials import token_expiry



def isolate_environment(home):
    """Replace ambient configuration before any Hermes import, including lazy deps."""
    logging.disable(logging.CRITICAL)
    os.environ.clear()
    os.environ.update(PATH='/usr/local/bin:/usr/bin:/bin', HOME=str(home),
                      HERMES_HOME=str(home), LANG='C.UTF-8',
                      PYTHONDONTWRITEBYTECODE='1', LD_LIBRARY_PATH='/opt/sqlite-fixed/lib',
                      HERMES_DISABLE_LAZY_INSTALLS='1')
    # HERMES_LAZY_INSTALL_TARGET intentionally remains absent: upstream treats
    # that variable as an exception to the sealed-venv installation switch.



def prepare_home(home):
    """Create a new, fixed bootstrap config; never load a selected user's config."""
    isolate_environment(home)
    root = Path(home)
    root.mkdir(mode=0o700, exist_ok=False)
    with (root / 'config.yaml').open('x') as file:
        file.write('agent:\n  environment_probe: false\nsecurity:\n  allow_lazy_installs: false\n')
    (root / 'config.yaml').chmod(0o600)


def arm_deadline(seconds=180):
    """Let the kernel end this child even with a blocked interpreter/controller."""
    # Linux namespace PID1 ignores ordinary default-fatal signals. Docker's
    # --init owns PID1; this worker must be its child for SIGALRM to be reliable.
    if os.getpid() == 1:
        raise BridgeError('worker_init_required', 503)
    signal.signal(signal.SIGALRM, signal.SIG_DFL)
    signal.alarm(seconds)


def bounded_json(path, maximum=8192):
    """Read a bounded operator-owned configuration file."""
    with open(path, 'rb') as file:
        value = file.read(maximum + 1)
    if len(value) > maximum:
        raise BridgeError('configuration_too_large')
    return json.loads(value)


def load_hermes():
    """Import only the immutable image's expected source after journal admission."""
    source = Path('/opt/hermes')
    verify_source(source)
    if (source / '.env').exists():
        raise BridgeError('source_environment_forbidden', 503)
    if (source / 'ORTAK_SOURCE_REVISION').read_text().strip() != HERMES_REVISION:
        raise BridgeError('image_revision_mismatch', 503)
    sys.path.insert(0, str(source))
    from run_agent import AIAgent
    if not Path(inspect.getfile(AIAgent)).resolve().is_relative_to(source):
        raise BridgeError('unexpected_hermes_source', 503)
    return AIAgent


def selected_provider_token(spec, provider, oauth_token):
    """Gate exact credentials again inside the child without loading refresh state."""
    if (not isinstance(provider, dict) or set(provider) != {'provider', 'credential_ref'}
            or spec['binding']['credential_refs'] != [provider['credential_ref']]
            or provider['provider'] not in {'openai', 'openrouter', 'openai-codex'}):
        raise BridgeError('credential_binding_mismatch')
    if provider['provider'] == 'openai-codex':
        import time
        if token_expiry(oauth_token) <= time.time() + 180:
            raise BridgeError('oauth_relogin_required', 503)
        return oauth_token
    if oauth_token is not None:
        raise BridgeError('credential_binding_mismatch')
    with open('/profile/provider-token', 'r') as token_file:
        token = token_file.read(4097).strip()
    if not token or len(token) > 4096 or any(c.isspace() for c in token):
        raise BridgeError('invalid_provider_credential')
    return token


def main():
    """Validate fixed profile ownership and journal before invoking the real agent."""
    parser = argparse.ArgumentParser()
    parser.add_argument('--journal', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    # No provider, proxy, gateway, MCP or profile configuration is inherited
    # from the container base environment. The run home is empty and private.
    arm_deadline()
    prepare_home('/tmp/hermes-home')
    path = Path(args.journal)
    if path.parent != Path('/ortak-state') or not path.is_file():
        raise BridgeError('invalid_journal_path')
    raw = sys.stdin.buffer.read(256 * 1024 + 16384 + 1)
    if len(raw) > 256 * 1024 + 16384:
        raise BridgeError('body_too_large')
    request = json.loads(raw)
    oauth_token = request.pop('oauth_access_token', None)
    spec = request['spec']
    key = spec['idempotency_key']
    company, _ = identity(key)
    marker = bounded_json('/profile/ORTAK_DISPOSABLE_PROFILE.json')
    if marker != {'company_id': company, 'employee_id': spec['employee_id'], 'profile_ref': spec['binding']['profile_ref']}:
        raise BridgeError('profile_ownership_mismatch')
    profile = bounded_json('/profile/ORTAK_RUNTIME_BINDING.json')
    journal = Journal(path)
    bridge = Bridge(journal, company, [{'employee_id': marker['employee_id'], 'binding': profile}])
    bridge.validate(request)
    workspace = request.get('workspace')
    # A delayed container receives only a tombstone and never loads Hermes.
    receipt = journal.lookup(key)
    if receipt is None or receipt['status'] != 'accepted':
        return
    # Recheck the full durable start fingerprint before selecting credentials.
    journal.reserve(spec, workspace=workspace)
    if workspace is not None:
        # The filesystem workflow has a hard provider-child ceiling as well as
        # the pinned model-loop budget; no hung SDK stream can extend it.
        arm_deadline(120)
    provider = bounded_json('/profile/ORTAK_PROVIDER.json')
    token = selected_provider_token(spec, provider, oauth_token)
    execute_candidate(spec, journal, None, provider['provider'], token, load_base=load_hermes, workspace=workspace)

if __name__ == '__main__':
    try:
        main()
    except BaseException:
        # The containing supervisor seals failures from process exit. Never print
        # provider exception text, token contents, input or model output to logs.
        sys.exit(1)
