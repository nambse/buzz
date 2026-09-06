"""Exact C2 filesystem layout derived from a bounded, same-snapshot DB projection."""

from datetime import datetime
import hashlib
import json
import os
import re
from uuid import UUID

from recovery_workspace_io import MAX_BINARY, absolute, require
from private_recovery_workspaces import TABLE_KEYS


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=False, allow_nan=False).encode('utf-8')


def digest(raw): return hashlib.sha256(raw).hexdigest()


def uuid(value):
    require(isinstance(value, str))
    parsed = UUID(value)
    require(parsed.int != 0 and str(parsed) == value)
    return value


def sha(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value))
    return value


def selection(value):
    require(isinstance(value, dict) and set(value) == {
        'company_id', 'input_root', 'run_root', 'reader_binary', 'reader_sha256', 'reader_uid'})
    uuid(value['company_id']); sha(value['reader_sha256'])
    require(type(value['reader_uid']) is int and value['reader_uid'] == os.getuid())
    roots = [absolute(value[key]) for key in ('input_root', 'run_root', 'reader_binary')]
    require(all(a != b and a not in b.parents and b not in a.parents
                for i, a in enumerate(roots) for b in roots[i + 1:]))
    return value


def grant(raw, company, revision):
    require(isinstance(raw, str) and len(raw.encode()) <= 16384)
    value = json.loads(raw)
    require(canonical(value) == raw.encode() and isinstance(value, dict) and set(value) == {
        'format', 'company_id', 'project_id', 'employee_id', 'workspace_ref',
        'revision', 'manifest_hash', 'files'})
    require(value['format'] == 'ortak-workspace-read/v1'
            and value['company_id'] == company and value['revision'] == revision)
    uuid(company); uuid(revision); uuid(value['project_id'])
    require(isinstance(value['employee_id'], str)
            and re.fullmatch('[a-z0-9][a-z0-9_-]{0,63}', value['employee_id'])
            and isinstance(value['workspace_ref'], str)
            and re.fullmatch('[A-Za-z0-9][A-Za-z0-9._:-]{0,127}', value['workspace_ref']))
    sha(value['manifest_hash'])
    require(value['manifest_hash'] == digest(canonical({k: v for k, v in value.items()
                                                       if k != 'manifest_hash'})))
    files = value['files']
    require(isinstance(files, list) and 1 <= len(files) <= 8)
    previous, names, total = '', set(), 0
    for file in files:
        require(isinstance(file, dict) and set(file) == {
            'file_id', 'name', 'media_type', 'bytes', 'sha256'})
        identifier = uuid(file['file_id']); sha(file['sha256'])
        name = file['name']
        require(identifier > previous and isinstance(name, str) and len(name) <= 256
                and re.fullmatch('[A-Za-z0-9][A-Za-z0-9._/-]*', name)
                and not any(part in ('', '.', '..') for part in name.split('/'))
                and name not in names and file['media_type'] == 'text/plain'
                and type(file['bytes']) is int and 0 <= file['bytes'] <= 16384)
        previous = identifier; names.add(name); total += file['bytes']
    require(total <= 65536)
    return value


def observation(value, selected):
    """Validate raw layout; only the held barrier supplies authoritative observations.

    The callback must query layout and database_evidence in ONE read-only
    REPEATABLE READ transaction and independently prove writers/OS readers and
    the selected cold journal contained. Hashes are bindings, not stop authority.
    """
    require(isinstance(value, dict) and set(value) == {
        'database_evidence', 'workspace_layout', 'closure_evidence'})
    require(len(canonical(value)) <= 1024**2, 'workspace_files_layout_bound')
    evidence, closure = value['database_evidence'], value['closure_evidence']
    require(isinstance(evidence, dict) and set(evidence) == {'schema_version', 'company_id', 'tables'}
            and type(evidence['schema_version']) is int and evidence['schema_version'] in (74, 75, 76, 77, 78)
            and evidence['company_id'] == selected['company_id']
            and isinstance(evidence['tables'], dict) and set(TABLE_KEYS) <= set(evidence['tables']))
    require(isinstance(closure, dict) and set(closure) == {'format', 'barrier_id',
            'selection_sha256', 'database_evidence_sha256', 'journal_sha256',
            'process_observation_sha256', 'workspace_journal_pending', 'live_reader_count',
            'live_writer_count'} and closure['format'] == 'ortak-workspace-files-closure/v1',
            'workspace_files_closure_required')
    uuid(closure['barrier_id'])
    for key in ('selection_sha256', 'database_evidence_sha256', 'journal_sha256',
                'process_observation_sha256'): sha(closure[key])
    require(closure['selection_sha256'] == digest(canonical(selected))
            and closure['database_evidence_sha256'] == digest(canonical(evidence))
            and all(type(closure[key]) is int and closure[key] == 0 for key in
                    ('workspace_journal_pending', 'live_reader_count', 'live_writer_count')),
            'workspace_files_closure_required')
    layout = value['workspace_layout']
    require(isinstance(layout, dict) and set(layout) == {'company_id', 'bindings', 'runs', 'readers'}
            and layout['company_id'] == selected['company_id'])
    bindings, runs, readers = layout['bindings'], layout['runs'], layout['readers']
    require(isinstance(bindings, list) and 1 <= len(bindings) <= 32
            and isinstance(runs, list) and len(runs) <= 64
            and isinstance(readers, list) and len(readers) <= 128, 'workspace_files_layout_bound')
    grants, by_run, ids = {}, {}, set()
    for row in bindings:
        require(isinstance(row, dict) and set(row) == {'revision', 'grant_bytes'})
        revision = uuid(row['revision'])
        require(revision not in grants)
        grants[revision] = grant(row['grant_bytes'], layout['company_id'], revision)
    for row in runs:
        require(isinstance(row, dict) and set(row) == {
            'run_id', 'revision', 'manifest_hash', 'store_ref', 'status'})
        run = uuid(row['run_id'])
        require(run not in by_run, 'workspace_files_ambiguous_run_revision')
        require(row['revision'] in grants and row['manifest_hash'] == grants[row['revision']]['manifest_hash']
                and row['status'] in ('completed', 'failed', 'cancelled'), 'workspace_files_runs_not_terminal')
        require(row['store_ref'] in (None, f'workspace-run:{layout["company_id"]}:{run}'))
        by_run[run] = row
    for row in readers:
        require(isinstance(row, dict) and set(row) == {'id', 'run_id', 'revision', 'executable',
            'executable_hash', 'operating_uid', 'state', 'stop_proof',
            'created_at', 'owner_deadline', 'stopped_at'})
        identifier = uuid(row['id'])
        require(identifier not in ids and row['run_id'] in by_run
                and row['revision'] == by_run[row['run_id']]['revision'])
        ids.add(identifier)
        require(row['state'] == 'stopped' and row['stop_proof'] in (
            'reaped', 'confirmed_absence', 'in_process_returned'), 'workspace_files_readers_not_contained')
        require(all(isinstance(row[key], str) and 1 <= len(row[key]) <= 64
                    for key in ('created_at', 'owner_deadline', 'stopped_at')),
                'workspace_files_readers_not_contained')
        dates = [datetime.fromisoformat(row[key].replace('Z', '+00:00'))
                 for key in ('created_at', 'owner_deadline', 'stopped_at')]
        require(all(v.tzinfo is not None for v in dates) and dates[0] <= dates[2]
                and (row['stop_proof'] != 'confirmed_absence' or dates[1] <= dates[2]),
                'workspace_files_readers_not_contained')
        if row['executable'] is None:
            require(row['executable_hash'] is None and row['operating_uid'] is None
                    and row['stop_proof'] == 'in_process_returned')
        else:
            require(row['executable'] == selected['reader_binary']
                    and row['executable_hash'] == selected['reader_sha256']
                    and type(row['operating_uid']) is int
                    and row['operating_uid'] == selected['reader_uid']
                    and row['stop_proof'] != 'in_process_returned', 'workspace_files_reader_identity')
    require(set(by_run) == {row['run_id'] for row in readers}, 'workspace_files_reader_history_missing')
    observed_keys = {}
    for table, keys in TABLE_KEYS.items():
        rows = evidence['tables'][table]
        require(isinstance(rows, list) and len(rows) <= 1024)
        seen = set()
        for row in rows:
            require(isinstance(row, dict) and set(row) == {'key', 'row_sha256'}
                    and isinstance(row['key'], list) and len(row['key']) == len(keys)
                    and row['key'][0] == selected['company_id'])
            sha(row['row_sha256'])
            key = tuple(row['key']); require(key not in seen); seen.add(key)
        observed_keys[table] = seen
    company = selected['company_id']
    require(observed_keys['workspace_bindings'] == {(company, revision) for revision in grants}
            and observed_keys['workspace_files'] == {(company, revision, file['file_id'])
                for revision, item in grants.items() for file in item['files']}
            and observed_keys['run_workspace_uses'] == {(company, run) for run, row in by_run.items() if row['store_ref']}
            and observed_keys['workspace_reader_executions'] == {(company, identifier) for identifier in ids},
            'workspace_files_projection_incomplete')
    return grants, by_run


def build(source, selected, grants, runs):
    """Enumerate only the exact retained cohort; unknown names refuse before read."""
    binary = absolute(selected['reader_binary'])
    parent = source.root(str(binary.parent), private=False)
    _, row = source.file(parent, binary.name, 'reader', MAX_BINARY,
                         modes=(0o500, 0o700, 0o555, 0o755))
    require(row['sha256'] == selected['reader_sha256'], 'workspace_files_reader_identity')
    roots = {}
    for label, key, marker in [('inputs', 'input_root', '.ortak-workspace-inputs-v1'),
                               ('runs', 'run_root', '.ortak-workspace-runs-v1')]:
        fd = source.root(selected[key]); roots[label] = fd
        source.add(label, fd, os.fstat(fd), 'directory')
        data, _ = source.file(fd, marker, f'{label}/{marker}', 128)
        require(data == f'ortak-workspace/v1:{selected["company_id"]}\n'.encode(),
                'workspace_files_marker_differs')
    require(source.names(roots['inputs']) == sorted(['.ortak-workspace-inputs-v1', *grants]),
            'workspace_files_inventory_differs')
    contents = {}
    for revision, value in sorted(grants.items()):
        folder = source.descend(roots['inputs'], revision, f'inputs/{revision}')
        require(source.names(folder) == [file['file_id'] for file in value['files']],
                'workspace_files_inventory_differs')
        data = {}
        for file in value['files']:
            raw, record = source.file(folder, file['file_id'], f'inputs/{revision}/{file["file_id"]}', 16384)
            require(record['bytes'] == file['bytes'] and record['sha256'] == file['sha256']
                    and b'\0' not in raw, 'workspace_files_input_changed')
            raw.decode('utf-8'); data[file['file_id']] = raw
        data['manifest.json'] = canonical(value); contents[revision] = data
    company = selected['company_id']
    names = source.names(roots['runs'])
    require(names in (['.ortak-workspace-runs-v1'], sorted(['.ortak-workspace-runs-v1', company])),
            'workspace_files_inventory_differs')
    if company not in names:
        require(not any(row['store_ref'] for row in runs.values()), 'workspace_files_run_copy_missing')
        return
    folder = source.descend(roots['runs'], company, f'runs/{company}', (0o700,))
    names = set(source.names(folder))
    require(names <= {name for run in runs for name in (run, run + '.lock', run + '.preparing')},
            'workspace_files_inventory_differs')
    for run, row in sorted(runs.items()):
        final, stage, lock = run in names, run + '.preparing' in names, run + '.lock' in names
        require(not (final and stage) and (not (final or stage) or lock)
                and (not row['store_ref'] or final and lock), 'workspace_files_run_copy_missing')
        if lock:
            source.file(folder, run + '.lock', f'runs/{company}/{run}.lock', 0, (0o600,), lock=True)
        if not (final or stage): continue
        name = run if final else run + '.preparing'
        prefix = f'runs/{company}/{name}'
        child = source.descend(folder, name, prefix, (0o500,) if row['store_ref'] else (0o500, 0o700))
        expected = contents[row['revision']]
        actual = source.names(child)
        require(set(actual) == set(expected) if final else
                set(actual) <= set(expected) | {key + '.partial' for key in expected},
                'workspace_files_inventory_differs')
        for key in actual:
            partial = key.endswith('.partial')
            target = expected[key.removesuffix('.partial')]
            raw, _ = source.file(child, key, f'{prefix}/{key}', len(target),
                                 (0o400, 0o600) if partial else (0o400,))
            require(target.startswith(raw) if partial else target == raw,
                    'workspace_files_run_copy_changed')
