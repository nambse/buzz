"""Exact, bounded server-owned workspace wire. No filesystem resolution here."""
import hashlib
import json
import re
from uuid import UUID

from .journal import BridgeError

FORMAT = 'ortak-workspace-read/v1'
TOOL = 'read_workspace_text'
FAILURES = {'authority_changed', 'workspace_unavailable', 'file_unavailable',
            'input_changed', 'deadline_exceeded', 'cancelled'}
SCHEMA = {'type': 'function', 'function': {
    'name': TOOL, 'description': 'Read one explicitly selected immutable workspace text input by file ID.',
    'parameters': {'type': 'object', 'properties': {'file_id': {'type': 'string'}},
                   'required': ['file_id'], 'additionalProperties': False}}}


def canonical(value):
    """Canonical ASCII metadata and exact UTF-8 content serialization."""
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False)


def digest(value):
    """SHA-256 of canonical JSON, shared with the typed Rust port."""
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def uuid(value):
    """Only nonzero canonical UUIDs are accepted."""
    try:
        parsed = UUID(value)
        return bool(parsed.int) and str(parsed) == value
    except (ValueError, TypeError, AttributeError):
        return False


def sha(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None


def validate_workspace(workspace, spec, company):
    """Validate exact grant/policy before credential resolution or provider I/O."""
    from .service import EMPTY_POLICY
    if workspace is None:
        if spec['permissions'] != EMPTY_POLICY:
            raise BridgeError('unsupported_permission_policy', 422)
        return
    fields = {'format', 'company_id', 'project_id', 'employee_id', 'workspace_ref',
              'revision', 'manifest_hash', 'files'}
    if not isinstance(workspace, dict) or set(workspace) != fields:
        raise BridgeError('invalid_workspace', 422)
    ref = workspace['workspace_ref']
    if (workspace['format'] != FORMAT or workspace['company_id'] != company
            or workspace['employee_id'] != spec['employee_id']
            or not uuid(workspace['company_id']) or not uuid(workspace['project_id'])
            or not uuid(workspace['revision'])
            or not isinstance(ref, str) or not re.fullmatch('[A-Za-z0-9][A-Za-z0-9._:-]{0,127}', ref)
            or spec['binding'].get('workspace_ref') != ref
            or not uuid(spec['context'].get('work_item_id'))
            or spec['context'].get('conversation_ref') is not None
            or spec['context'].get('reply_to_message_id') is not None):
        raise BridgeError('invalid_workspace', 422)
    expected = {**EMPTY_POLICY, 'allowed_tools': ['files'], 'allowed_workspaces': [ref]}
    if spec['permissions'] != expected:
        raise BridgeError('unsupported_permission_policy', 422)
    files = workspace['files']
    if not isinstance(files, list) or not 1 <= len(files) <= 8:
        raise BridgeError('invalid_workspace', 422)
    ids, names, total = [], set(), 0
    for file in files:
        if not isinstance(file, dict) or set(file) != {'file_id', 'name', 'media_type', 'bytes', 'sha256'}:
            raise BridgeError('invalid_workspace', 422)
        name = file['name']
        if (not uuid(file['file_id']) or not isinstance(name, str) or len(name) > 256
                or not re.fullmatch('[A-Za-z0-9][A-Za-z0-9._/-]*', name)
                or any(part in {'', '.', '..'} for part in name.split('/'))
                or name in names or file['media_type'] != 'text/plain'
                or type(file['bytes']) is not int or not 0 <= file['bytes'] <= 16384
                or not sha(file['sha256'])):
            raise BridgeError('invalid_workspace', 422)
        ids.append(file['file_id'])
        names.add(name)
        total += file['bytes']
    if (ids != sorted(set(ids)) or total > 65536 or not sha(workspace['manifest_hash'])
            or workspace['manifest_hash'] != digest({k: v for k, v in workspace.items() if k != 'manifest_hash'})):
        raise BridgeError('invalid_workspace', 422)


def arguments(raw):
    """Reject additional/duplicate argument keys and return canonical arguments."""
    def pairs(values):
        result = dict(values)
        if len(result) != len(values):
            raise ValueError()
        return result
    try:
        if not isinstance(raw, str) or len(raw.encode()) > 128:
            raise ValueError()
        value = json.loads(raw, object_pairs_hook=pairs)
        if not isinstance(value, dict) or set(value) != {'file_id'} or not uuid(value['file_id']):
            raise ValueError()
        return value, digest(value)
    except (ValueError, TypeError, UnicodeError):
        raise BridgeError('invalid_tool_arguments', 422) from None


def validate_request(request):
    """Keep provider call identities finite and payload-free."""
    if (not isinstance(request, dict) or set(request) != {'call_id', 'file_id', 'arguments_hash', 'ordinal'}
            or not isinstance(request['call_id'], str)
            or not re.fullmatch('[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}', request['call_id'])
            or not uuid(request['file_id']) or type(request['ordinal']) is not int
            or not 1 <= request['ordinal'] <= 4
            or request['arguments_hash'] != digest({'file_id': request['file_id']})):
        raise BridgeError('invalid_tool_request', 422)


def validate_result(result, file):
    """No content enters the child unless the immutable hash, size and name agree."""
    if not isinstance(result, dict):
        raise BridgeError('invalid_tool_result', 422)
    if result.get('status') == 'failed':
        if set(result) == {'status', 'code'} and result['code'] in FAILURES:
            return
    elif (set(result) == {'status', 'content', 'sha256', 'bytes', 'name'}
          and result['status'] == 'completed' and isinstance(result['content'], str)):
        try:
            data = result['content'].encode()
        except UnicodeError:
            raise BridgeError('invalid_tool_result', 422) from None
        if (len(data) <= 16384 and '\0' not in result['content']
                and type(result['bytes']) is int and result['bytes'] == len(data) == file['bytes']
                and result['sha256'] == hashlib.sha256(data).hexdigest() == file['sha256']
                and result['name'] == file['name']):
            return
    raise BridgeError('invalid_tool_result', 422)
