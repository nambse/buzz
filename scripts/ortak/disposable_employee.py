"""Exact public selection and private atomic preparation for a fresh employee."""
from contextlib import contextmanager
from datetime import datetime
import fcntl
import ipaddress
import json
import os
from pathlib import Path
import re
import stat
from urllib.parse import urlsplit
from uuid import UUID

from bootstrap_private_memory import Refused, canonical, require

FORMAT = 'ortak-disposable-employee-prepare/1'
MAX_BYTES = 65536


def exact(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys), 'invalid_selection_fields')


def identifier(value):
    require(isinstance(value, str) and re.fullmatch('[a-z][a-z0-9_-]{0,63}', value), 'invalid_employee_id')


def uuid(value):
    require(isinstance(value, str) and str(UUID(value)) == value and UUID(value).int != 0, 'invalid_uuid')


def reference(value):
    require(isinstance(value, str) and re.fullmatch('[a-z][a-z0-9+.-]*://[A-Za-z0-9][A-Za-z0-9/_.:-]{0,240}', value), 'invalid_reference')


def path(value):
    require(isinstance(value, str) and len(value) <= 1024 and ',' not in value and '\0' not in value, 'invalid_path')
    result = Path(value)
    require(result.is_absolute() and str(result.resolve()) == value and result.name not in ('', '.', '..'), 'noncanonical_path')
    return result


def validate(value):
    """Validate the complete public intent; never inspect credentials or adapters."""
    fields = {'format', 'company_id', 'employee_id', 'output_directory', 'signer_ref', 'signer_env',
              'runtime_binding', 'oauth_directory', 'worker_image', 'key_generator', 'memory'}
    exact(value, fields | ({'oauth_owner'} if isinstance(value, dict) and 'oauth_owner' in value else set()))
    require(value['format'] == FORMAT, 'invalid_selection_format')
    uuid(value['company_id']); identifier(value['employee_id'])
    root, oauth = path(value['output_directory']), path(value['oauth_directory'])
    require(root != oauth and root not in oauth.parents and oauth not in root.parents, 'overlapping_resource_roots')
    reference(value['signer_ref'])
    require(re.fullmatch('[A-Z_][A-Z0-9_]{0,127}', value['signer_env'] or ''), 'invalid_signer_environment')
    require(isinstance(value['worker_image'], str) and re.fullmatch('sha256:[0-9a-f]{64}', value['worker_image']), 'immutable_worker_image_required')
    generator = value['key_generator']; exact(generator, ('path', 'sha256'))
    path(generator['path'])
    require(isinstance(generator['sha256'], str) and re.fullmatch('[0-9a-f]{64}', generator['sha256']), 'invalid_generator_digest')
    binding = value['runtime_binding']
    exact(binding, ('adapter', 'profile_ref', 'workspace_ref', 'model', 'credential_refs', 'options'))
    require(binding['adapter'] == 'hermes' and binding['workspace_ref'] == 'none', 'initial_empty_policy_profile_required')
    require(isinstance(binding['profile_ref'], str) and re.fullmatch('[A-Za-z0-9][A-Za-z0-9._:-]{0,127}', binding['profile_ref']), 'invalid_profile_ref')
    require(isinstance(binding['model'], str) and re.fullmatch('[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}', binding['model']), 'invalid_model')
    exact(binding['options'], ('reasoning_effort',))
    effort = binding['options']['reasoning_effort']
    require(effort in ('low', 'medium', 'high', 'xhigh', 'max') and
            (effort != 'max' or binding['model'] == 'gpt-6-astra' or binding['model'].startswith('gpt-5.6')), 'invalid_effort')
    require(isinstance(binding['credential_refs'], list) and len(binding['credential_refs']) == 1, 'exact_runtime_credential_required')
    reference(binding['credential_refs'][0])
    if 'oauth_owner' in value:
        owner = value['oauth_owner']
        exact(owner, ('format', 'company_id', 'employee_id', 'profile_ref', 'credential_ref'))
        identifier(owner['employee_id']); reference(owner['credential_ref'])
        require(owner['format'] == 'ortak-oauth-identity/1' and owner['company_id'] == value['company_id']
                and owner['employee_id'] != value['employee_id']
                and owner['credential_ref'] == binding['credential_refs'][0]
                and isinstance(owner['profile_ref'], str)
                and re.fullmatch('[A-Za-z0-9][A-Za-z0-9._:-]{0,127}', owner['profile_ref']),
                'invalid_shared_connection_selection')
    memory = value['memory']
    exact(memory, ('deployment_id', 'origin', 'token_ref', 'token_env', 'binding', 'creation_key',
                   'validation_run_id', 'validation_recorded_at'))
    uuid(memory['deployment_id']); uuid(memory['validation_run_id']); reference(memory['token_ref'])
    require(isinstance(memory['token_env'], str) and re.fullmatch('ORTAK_HONCHO_[A-Z0-9_]{1,112}', memory['token_env']), 'invalid_memory_environment')
    require(memory['token_env'] != value['signer_env'] and len({memory['token_ref'], value['signer_ref'], binding['credential_refs'][0]}) == 3, 'credential_owners_overlap')
    origin = urlsplit(memory['origin'])
    require(origin.scheme == 'http' and origin.hostname is not None and ipaddress.ip_address(origin.hostname).is_loopback
            and origin.port is not None and origin.path in ('', '/') and not origin.query and not origin.fragment
            and not origin.username and not origin.password and memory['origin'] == f'http://{origin.netloc}', 'explicit_loopback_memory_required')
    mb = memory['binding']; exact(mb, ('adapter', 'endpoint_ref', 'workspace', 'user_peer', 'employee_peer', 'options'))
    reference(mb['endpoint_ref'])
    require(mb['adapter'] == 'honcho' and mb['options'] == {} and mb['user_peer'] != mb['employee_peer'], 'invalid_memory_binding')
    for field in ('workspace', 'user_peer', 'employee_peer'):
        require(isinstance(mb[field], str) and re.fullmatch('[A-Za-z0-9_-]{1,128}', mb[field]), 'invalid_memory_name')
    require(isinstance(memory['creation_key'], str) and re.fullmatch('[!-~]{1,200}', memory['creation_key']), 'invalid_memory_creation_key')
    stamp = memory['validation_recorded_at']
    require(isinstance(stamp, str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{6})?Z', stamp), 'invalid_diagnostic_time')
    parsed = datetime.fromisoformat(stamp.replace('Z', '+00:00'))
    require(parsed.isoformat(timespec='microseconds' if parsed.microsecond else 'seconds').replace('+00:00', 'Z') == stamp, 'invalid_diagnostic_time')
    require(len(canonical(value).encode()) <= 16384, 'selection_too_large')
    return value


def read(path, *, public=False, staged=False):
    """Single-link bounded files only; secret leaves require exactly0600."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        meta = os.fstat(stream.fileno())
        modes = (0o600, 0o444) if public and staged else (0o444 if public else 0o600,)
        require(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.getuid() and meta.st_nlink == 1
                and stat.S_IMODE(meta.st_mode) in modes, 'private_leaf_changed')
        data = stream.read(MAX_BYTES + 1)
    require(len(data) <= MAX_BYTES, 'private_leaf_too_large')
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate_json_field'); result[key] = value
        return result
    value = json.loads(data, object_pairs_hook=pairs)
    require(not staged or data == (canonical(value) + '\n').encode(), 'incomplete_pending_leaf')
    return value


def pending(path):
    """One target-associated checkpoint; callers hold the selected root lock."""
    return path.with_name('.pending-' + path.name)


def present(path):
    return path.exists() or path.is_symlink()


def save(path, value, *, immutable=False, public=False):
    """Durable atomic replacement; immutable values can only replay exactly."""
    temporary = pending(path)
    if present(path):
        prior = read(path, public=public)
        if immutable:
            require(prior == value, 'prepared_selection_changed')
            if not present(temporary): return
    data = (canonical(value) + '\n').encode()
    require(len(data) <= MAX_BYTES, 'private_leaf_too_large')
    if present(temporary):
        require(read(temporary, public=public, staged=True) == value, 'pending_leaf_changed')
        fd = os.open(temporary, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as stream:
            if public: os.fchmod(stream.fileno(), 0o444)
            os.fsync(stream.fileno())
    else:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data); stream.flush(); os.fsync(stream.fileno())
            if public: os.fchmod(stream.fileno(), 0o444); os.fsync(stream.fileno())
    # Interrupted writes remain scoped checkpoints. Unknown/incomplete bytes
    # are retained and refused, never silently removed or treated as success.
    os.replace(temporary, path)
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try: os.fsync(descriptor)
    finally: os.close(descriptor)


@contextmanager
def selected_root(selection, *, create=False):
    """An exclusive per-employee directory never adopts an unmarked old tree."""
    root = path(selection['output_directory'])
    if create and not root.exists():
        parent = root.parent.stat()
        require(parent.st_uid == os.getuid() and not parent.st_mode & 0o077, 'private_parent_required')
        root.mkdir(mode=0o700)
    meta = root.lstat()
    require(stat.S_ISDIR(meta.st_mode) and meta.st_uid == os.getuid() and stat.S_IMODE(meta.st_mode) == 0o700, 'private_root_changed')
    intent = root / 'selection.json'
    if not intent.exists():
        # Refuse an old tree before even creating our lock within it.
        require(create and {child.name for child in root.iterdir()} <= {'.prepare.lock', pending(intent).name}, 'unmarked_employee_root')
        if present(pending(intent)):
            require(read(pending(intent), staged=True) == selection, 'prepared_selection_changed')
    fd = os.open(root / '.prepare.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        meta = os.fstat(fd)
        require(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.getuid() and meta.st_nlink == 1
                and stat.S_IMODE(meta.st_mode) == 0o600, 'private_lock_changed')
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if not intent.exists():
            require(create and {child.name for child in root.iterdir()} <= {'.prepare.lock', pending(intent).name}, 'unmarked_employee_root')
        save(intent, selection, immutable=True)
        yield root
    finally: os.close(fd)


def enrollment(selection):
    """Exact existing interactive command; no credential access or Docker launch."""
    if 'oauth_owner' in selection:
        return None
    binding = selection['runtime_binding']
    return ['python', '-m', 'ortak_hermes_bridge.oauth_login', '--directory', selection['oauth_directory'],
            '--company', selection['company_id'], '--employee', selection['employee_id'],
            '--profile-ref', binding['profile_ref'], '--credential-ref', binding['credential_refs'][0]]


def shared_connection(selection):
    """Public recipe only; the controller must validate the existing owner/store."""
    return {'format': 'ortak-shared-oauth-connection/1', 'company_id': selection['company_id'],
            'employee_id': selection['employee_id'], 'binding': selection['runtime_binding'],
            'oauth_directory': selection['oauth_directory'], 'oauth_owner': selection['oauth_owner'],
            'ownership_verified': False, 'oauth_enrolled': False, 'employee_activated': False}
