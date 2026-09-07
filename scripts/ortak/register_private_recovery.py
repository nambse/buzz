#!/usr/bin/env python3
"""Freeze current private process/session evidence and prospective resume code, without stopping anything."""

import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
from uuid import uuid4

from backup_private_database import Refused, private_binary, private_directory
from prepare_private_recovery import authority, load_preparation, observe, save, sha
import private_recovery_inventory as inventory

REPOSITORY_HELPERS = Path(__file__).resolve().parent
ROLLOUT = inventory.NATIVE_RESUME
LAUNCH_HELPERS = inventory.CURRENT_LAUNCH_HELPERS
LAUNCHERS = inventory.NATIVE_LAUNCHERS
RECEIPTS = inventory.NATIVE_RECEIPTS
FORMAT = 'ortak-private-recovery-owners/1'
MANAGEMENT_SELECTION = (LAUNCHERS['ortak-management'], RECEIPTS['ortak-management'], LAUNCH_HELPERS)
OPERATOR_FILES = (
    'backup_private_database.py', 'backup_private_honcho.py', 'capture_private_recovery.py',
    'check_private_recovery_gate.py', 'init_private_stack.py', 'prepare_private_recovery.py',
    'private_native_services.py', 'private_recovery_inventory.py', 'private_recovery_payloads.py',
    'private_recovery_scorer.py',
    'private_recovery_deployment76.py',
    'recovery_image_export.py',
    'private_recovery_database_metadata.py', 'private_recovery_schema_lease.py', 'private_recovery_obligations.py',
    'private_recovery_extensions77.py',
    'private_recovery_workspaces.py', 'private_recovery_conversations.py', 'private_recovery_workspace_capture.py',
    'private_recovery_workspace_files.py', 'recovery_workspace_layout.py', 'recovery_workspace_io.py',
    'restore_workspace_files.py', 'private_recovery_journal.py', 'recovery_journal_archive.py',
    'recovery_confidential_journal.py',
    'recovery_native_confidential.py', 'private_recovery_native_confidential.py',
    'private_recovery_offline_stores.py', 'private_restore_credential_functions.py',
    'private_restore_honcho_checks.py',
    'recovery_archive_io.py', 'recovery_lock_holder.py', 'recovery_native_ingress.py', 'register_private_recovery.py',
    'restore_private_recovery.py',
)


def selected_process_sources():
    """A management writer requires a new explicit source/receipt selection before registration."""
    inventory.require(isinstance(MANAGEMENT_SELECTION, tuple) and len(MANAGEMENT_SELECTION) == 3,
        'management_launch_selection_required')
    launcher, receipt, helper_root = MANAGEMENT_SELECTION
    inventory.require(all(isinstance(path, Path) and path.is_relative_to(inventory.STATE)
        for path in MANAGEMENT_SELECTION), 'management_launch_selection_scope')
    launchers = {**LAUNCHERS, 'ortak-management': launcher}
    receipts = {**RECEIPTS, 'ortak-management': receipt}
    imports = {name: LAUNCH_HELPERS for name in LAUNCHERS}
    imports['ortak-management'] = helper_root
    inventory.native_writer_set(receipts)
    return launchers, receipts, imports


def source(path, private=False):
    """Read bounded public source code only, checking ownership/links and stability."""
    before = path.lstat()
    inventory.require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
        and before.st_nlink == 1 and before.st_size <= 65536
        and stat.S_IMODE(before.st_mode) in ({0o600, 0o500} if private else {0o644, 0o755}), 'launcher_source_refused')
    if private:
        relative = path.relative_to(inventory.STATE)
        for count in range(len(relative.parts)):
            inventory.directory(inventory.STATE.joinpath(*relative.parts[:count]))
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as stream:
        metadata = os.fstat(stream.fileno())
        raw = stream.read(65537)
    after = path.lstat()
    inventory.require(len(raw) <= 65536 and (before.st_ino, before.st_mtime_ns, before.st_size)
        == (metadata.st_ino, metadata.st_mtime_ns, metadata.st_size)
        == (after.st_ino, after.st_mtime_ns, after.st_size), 'launcher_source_changed')
    ast.parse(raw)  # Source is never executed during registration.
    return raw


def frozen_source(path, raw):
    """Fsync fresh owner-private immutable code; no secret/config content is copied."""
    with private_binary(path) as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(0o500)
    return {'path': str(path), 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}


def freeze_operator(operation):
    """Freeze the explicit complete local helper closure, including deferred credential-restore imports."""
    directory = private_directory(operation / 'operator-code', fresh=True)
    rows, total = {}, 0
    for name in OPERATOR_FILES:
        original = REPOSITORY_HELPERS / name
        raw = source(original); total += len(raw)
        inventory.require(total <= 1024 * 1024, 'operator_code_bound')
        for node in ast.walk(ast.parse(raw)):
            modules = [alias.name for alias in node.names] if isinstance(node, ast.Import) else \
                [node.module] if isinstance(node, ast.ImportFrom) and node.module else []
            for module in modules:
                local = module.split('.')[0] + '.py'
                inventory.require(not (REPOSITORY_HELPERS / local).is_file() or local in OPERATOR_FILES,
                    'operator_local_import_unreviewed')
        rows[name] = {'original_path': str(original), 'original_sha256': hashlib.sha256(raw).hexdigest(),
            'frozen': frozen_source(directory / name, raw), 'rebase': False}
    for row in rows.values():
        inventory.require(hashlib.sha256(source(Path(row['original_path']))).hexdigest() == row['original_sha256'],
            'operator_source_changed')
    return rows


def rebase_source(raw, destination, original_directory=REPOSITORY_HELPERS):
    """Replace exactly one AST string literal selecting the reviewed helper directory."""
    text = raw.decode()
    matches = [node for node in ast.walk(ast.parse(text)) if isinstance(node, ast.Constant)
               and node.value == str(original_directory)]
    inventory.require(len(matches) == 1, 'launcher_helper_literal_refused')
    node = matches[0]
    inventory.require(node.lineno == node.end_lineno, 'launcher_helper_literal_refused')
    lines = text.splitlines(keepends=True)
    line = lines[node.lineno - 1].encode()
    lines[node.lineno - 1] = (line[:node.col_offset] + repr(str(destination)).encode()
                             + line[node.end_col_offset:]).decode()
    changed = ''.join(lines).encode()
    # Only this constant changes in the AST; all executable logic stays intact.
    original_tree = ast.parse(raw)
    for entry in ast.walk(original_tree):
        if isinstance(entry, ast.Constant) and entry.value == str(original_directory):
            entry.value = str(destination)
    inventory.require(ast.dump(original_tree) == ast.dump(ast.parse(changed)), 'launcher_rebase_changed_logic')
    return changed


def sessions(observation):
    """Bind declared terminal sessions to rediscovered PID/start/loaded-image identities."""
    result = {}
    inventory.native_writer_set(observation['native_processes'])
    launchers, receipts, imports = selected_process_sources()
    for name, path in receipts.items():
        record, metadata = inventory.native_launch_record(name)
        process = observation['native_processes'][name]
        session, pid = record['session'], record['pid']
        inventory.require(record['status'] == 'resumed_verified' and type(session) is int and session > 0
            and pid == process['pid'] and record['binary'] == process['executable']
            and record['sha256'] == process['sha256'] and record['launcher'] == str(launchers[name])
            and record['helper_import_root'] == str(imports[name])
            and record['identity'].split() == [str(pid), str(process['uid']), *process['started_at'].split()],
                          'session_process_receipt_mismatch')
        result[name] = {'session_id': session, 'receipt': metadata,
                        'live_process': observation['native_processes'][name]}
    return result


def approved_launcher(name, raw):
    """Each selected live launch receipt binds the original public code and helper directory."""
    launchers, receipts, imports = selected_process_sources()
    record, _ = inventory.native_launch_record(name)
    inventory.require(record['launcher'] == str(launchers[name])
        and record['launcher_sha256'] == hashlib.sha256(raw).hexdigest()
        and record['helper_import_root'] == str(imports[name]), 'launcher_rollout_mismatch')


def register(preparation, *, observer=observe):
    """Create one retained prospective resume closure; a stale preparation cannot gain authority."""
    prepared = load_preparation(preparation)
    parent = private_directory(inventory.STATE / 'recovery-operations')
    operation = private_directory(parent / uuid4().hex, fresh=True)
    save(operation / 'intent.json', {'format': FORMAT, 'preparation': str(preparation),
                                    'action': 'register_owners', 'source_mutations': False})
    try:
        current = observer(operation)
        inventory.require(authority(current) == authority(prepared['observation']), 'prepared_authority_changed')
        owners = sessions(current)
        launchers, _, import_roots = selected_process_sources()
        closure = private_directory(operation / 'resume-code', fresh=True)
        code = {}
        for name in ['private_native_services.py', 'init_private_stack.py']:
            original = REPOSITORY_HELPERS / name
            raw = source(original)
            code[name] = {'original_path': str(original), 'original_sha256': hashlib.sha256(raw).hexdigest(),
                          'frozen': frozen_source(closure / name, raw), 'rebase': False}
            for helper_root in set(import_roots.values()):
                selected_helper = helper_root / name
                inventory.require(hashlib.sha256(source(selected_helper, private=True)).hexdigest()
                                  == code[name]['original_sha256'], 'selected_frozen_helper_source_differs')
        recipes = {}
        for name, original in launchers.items():
            raw = source(original, private=True)
            approved_launcher(name, raw)
            rebased = rebase_source(raw, closure, import_roots[name])
            saved = frozen_source(closure / (name + '-resume.py'), rebased)
            code[name] = {'original_path': str(original), 'original_sha256': hashlib.sha256(raw).hexdigest(),
                          'frozen': saved, 'rebase': 'only_exact_helper_directory_string_literal'}
            recipes[name] = [sys.executable, saved['path']]
        for name in inventory.NATIVE_WRITERS:
            owners[name]['historical_launcher_hash_attested'] = True
            owners[name]['resume_recipe_status'] = 'prospective_exact_logic_with_frozen_helper_import'
        inventory.require(sessions(current) == {name: {k: v for k, v in row.items()
            if k not in {'historical_launcher_hash_attested', 'resume_recipe_status'}} for name, row in owners.items()},
            'process_receipt_changed')
        for row in code.values():
            original = Path(row['original_path'])
            private = original in launchers.values()
            inventory.require(hashlib.sha256(source(original, private=private)).hexdigest()
                == row['original_sha256'], 'launcher_source_changed')
        recheck = private_directory(operation / 'recheck', fresh=True)
        inventory.require(authority(observer(recheck)) == authority(current), 'registration_authority_changed')
        operator_code = freeze_operator(operation)
        result = {'format': FORMAT, 'status': 'registered', 'operation_id': operation.name,
            'preparation': str(preparation), 'prepared_authority_sha256': sha(authority(current)),
            'owners': owners, 'source_code': code, 'resume_recipes': recipes,
            'operator_code': operator_code,
            'capture_command': [sys.executable, operator_code['capture_private_recovery.py']['frozen']['path']],
            'working_directory': str(inventory.STATE), 'source_mutations': False,
            'python_runtime': {'executable': sys.executable, 'version': sys.version,
                               'complete_runtime_backed_up': False},
            'service_paused': False, 'quiescence_witness': None,
            'limitations': ['terminal_session_is_a_saved_owner_receipt_not_signal_authority',
                'current_pid_start_inode_hash_must_be_revalidated_immediately_before_signal',
                'resume_recipes_require_root_review_and_new_execution_receipts',
                'new_controller_or_worker_generation_requires_new_registry',
                'no_secret_or_oauth_contents_copied']}
        result['registry_sha256'] = sha(result)
        save(operation / 'owners.json', result)
        return operation
    except (Refused, OSError, ValueError, KeyError, TypeError, SyntaxError):
        save(operation / 'failure.json', {'status': 'failed', 'error_code': 'owner_registration_refused',
                                         'source_mutations': False, 'service_paused': False})
        raise Refused('owner_registration_refused_private_evidence_retained') from None


def main():
    """Registration accepts only a selected private preparation, never an arbitrary launcher."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preparation', type=Path, required=True)
    args = parser.parse_args()
    output = register(args.preparation)
    print(json.dumps({'status': 'registered', 'registry': str(output / 'owners.json'), 'service_paused': False}))


if __name__ == '__main__':
    try:
        main()
    except (Refused, OSError, ValueError, KeyError, TypeError):
        raise SystemExit('Owner registration refused; private evidence retained. No service was paused.') from None
