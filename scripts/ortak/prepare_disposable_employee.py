#!/usr/bin/env python3
"""Plan or explicitly prepare one new disposable employee; never activate it."""
import argparse
import hashlib
import os
from pathlib import Path
import signal
import stat

from disposable_employee import (canonical, enrollment, pending, present, read, require, save, selected_root,
                                 shared_connection, validate)
from disposable_employee_memory import Http, prepare_memory
from private_native_services import identity


def plan(selection):
    """Public-only plan; no key, token, output directory or external service I/O."""
    validate(selection)
    result = {'action': 'plan', 'company_id': selection['company_id'], 'employee_id': selection['employee_id'],
            'output_directory': selection['output_directory'], 'worker_image': selection['worker_image'],
            'oauth_enrollment_argv': enrollment(selection), 'oauth_enrolled': False,
            'next_actions': ['prepare', 'root_interactive_oauth_enrollment', 'memory', 'root_membership_and_catalog_adopt'],
            'employee_activated': False, 'worker_started': False}
    if 'oauth_owner' in selection:
        result['shared_connection'] = shared_connection(selection)
        result['next_actions'] = ['prepare', 'root_register_shared_connection', 'memory', 'root_membership_and_catalog_adopt']
    return result


def generator(selection):
    """Verify a selected immutable key generator before bounded private capture."""
    source = selection['key_generator']; path = Path(source['path'])
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
                and not metadata.st_mode & 0o022 and metadata.st_mode & 0o111
                and 0 < metadata.st_size <= 256 * 1024 * 1024, 'key_generator_not_pinned')
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    require(digest == source['sha256'], 'key_generator_digest_changed')
    return path


def prepare(selection, generate=None):
    """Persist signer once, then derive immutable public profile/config artifacts."""
    validate(selection)
    with selected_root(selection, create=True) as root:
        signer = root / 'signer.json'
        if present(signer) or present(pending(signer)):
            keys = read(signer) if present(signer) else read(pending(signer), staged=True)
            validate_keys(keys)
            save(signer, keys, immutable=True)
        else:
            if 'oauth_owner' not in selection:
                require(not Path(selection['oauth_directory']).exists(), 'fresh_oauth_directory_required')
            binary = generator(selection)
            for name in ('home', 'tmp'):
                child = root / name
                if not child.exists(): child.mkdir(mode=0o700)
                meta = child.lstat()
                require(stat.S_ISDIR(meta.st_mode) and meta.st_uid == os.getuid()
                        and stat.S_IMODE(meta.st_mode) == 0o700, 'private_execution_directory_changed')
            keys = (generate or identity)(binary, root)
            validate_keys(keys)
            save(signer, keys, immutable=True)
        validate_keys(keys)
        binding = selection['runtime_binding']
        profile = root / 'profile'
        if not profile.exists(): profile.mkdir(mode=0o700)
        meta = profile.lstat()
        require(stat.S_ISDIR(meta.st_mode) and meta.st_uid == os.getuid()
                and stat.S_IMODE(meta.st_mode) in (0o700, 0o555), 'public_profile_directory_changed')
        files = {'ORTAK_DISPOSABLE_PROFILE.json': {'company_id': selection['company_id'],
                    'employee_id': selection['employee_id'], 'profile_ref': binding['profile_ref']},
                 'ORTAK_RUNTIME_BINDING.json': binding,
                 'ORTAK_PROVIDER.json': {'provider': 'openai-codex', 'credential_ref': binding['credential_refs'][0]}}
        known = set(files) | {pending(profile / name).name for name in files}
        require({child.name for child in profile.iterdir()} <= known, 'unexpected_profile_file')
        for name, data in files.items(): save(profile / name, data, immutable=True, public=True)
        profile.chmod(0o555)
        entry = {'employee_id': selection['employee_id'], 'binding': binding,
                 'directory': str(profile), 'oauth_directory': selection['oauth_directory']}
        if 'oauth_owner' in selection:
            entry['oauth_owner'] = selection['oauth_owner']
        save(root / 'controller-profile.json', entry, immutable=True)
        save(root / 'office-signer.json', {'company_id': selection['company_id'], 'employee_id': selection['employee_id'],
             'public_key': keys['public_key'], 'signer_ref': selection['signer_ref'], 'secret_env': selection['signer_env']}, immutable=True)
        if 'oauth_owner' in selection:
            save(root / 'oauth-connection.json', shared_connection(selection), immutable=True)
        else:
            save(root / 'oauth-enrollment.json', {'worker_image': selection['worker_image'],
                 'argv': enrollment(selection), 'required_uid': 10001, 'interactive_tty': True,
                 'oauth_enrolled': False, 'parent_requires_mode': '0700'}, immutable=True)
        return {'result': 'public_profile_prepared', 'employee_id': selection['employee_id'],
                'public_key': keys['public_key'], 'oauth_enrolled': False,
                'employee_activated': False, 'worker_started': False}


def validate_keys(keys):
    """Only private leaf validation; callers never serialize this object publicly."""
    import re
    require(isinstance(keys, dict) and set(keys) == {'public_key', 'secret_key'}
            and all(isinstance(v, str) and re.fullmatch('[0-9a-f]{64}', v) for v in keys.values())
            and int(keys['secret_key'], 16) != 0, 'invalid_generated_identity')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selection', type=Path, required=True)
    parser.add_argument('--action', choices=('plan', 'prepare', 'memory', 'export'), default='plan')
    args = parser.parse_args(argv)
    os.umask(0o077)
    selection = validate(read(args.selection))
    if args.action == 'plan': result = plan(selection)
    elif args.action == 'prepare': result = prepare(selection)
    else:
        def deadline(_signal, _frame): raise TimeoutError('employee_memory_deadline')
        previous = signal.signal(signal.SIGALRM, deadline)
        signal.setitimer(signal.ITIMER_REAL, 20)
        try:
            memory = selection['memory']
            result = prepare_memory(selection, lambda: Http(memory['origin'], os.environ.get(memory['token_env'])),
                                    export_only=args.action == 'export')
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0); signal.signal(signal.SIGALRM, previous)
    print(canonical(result))


if __name__ == '__main__':
    try: main()
    except Exception:
        raise SystemExit('Employee preparation refused; retained state preserved. No credentials were displayed.') from None
