"""Reviewed80 Work reference authority catalog, independent of compared databases."""
import hashlib

FUNCTIONS = {'ortak_run_work_context_current': ['company uuid, run uuid',
                                    'plpgsql',
                                    's',
                                    False,
                                    False,
                                    False,
                                    'u',
                                    None,
                                    'boolean'],
 'ortak_work_context_request80': ['',
                                  'plpgsql',
                                  'v',
                                  False,
                                  False,
                                  False,
                                  'u',
                                  None,
                                  'trigger'],
 'ortak_work_snapshot_admission80': ['',
                                     'plpgsql',
                                     'v',
                                     False,
                                     False,
                                     False,
                                     'u',
                                     None,
                                     'trigger']}
BODY_SHA256 = {'ortak_run_work_context_current': 'b1709b2d2556fa374cc97c62a17ee6dc6fd5fc565c56652c4fcd933197a179fa',
 'ortak_work_context_request80': '1b9618f25662de3683aa791e9c166e3bf714925ce5a847cdac19d2ad8dd644f6',
 'ortak_work_snapshot_admission80': '843bb29459c057f9aa73c2942e491005decc6bcb393ebfee03033bcf8bdf61c8'}
TRIGGERS = [['run_context_snapshots',
  'work_snapshot_admission80',
  'O',
  5,
  True,
  True,
  'public',
  'ortak_work_snapshot_admission80',
  '',
  0,
  [],
  True,
  False,
  None],
 ['work_executions',
  'work_context_request80',
  'O',
  5,
  True,
  True,
  'public',
  'ortak_work_context_request80',
  '',
  0,
  [],
  True,
  False,
  None]]
CLOSED_BODY = "\nBEGIN\n    RAISE EXCEPTION 'ortak: schema80 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';\nEND\n"

def check(value, refused):
    """Reject equal-but-weakened Work authority bodies and deferred triggers."""
    functions = {row[0]: row for row in value.get('functions', [])}
    for name, metadata in FUNCTIONS.items():
        row = functions.get(name)
        if row is None or len(row) != 11 or row[1:10] != metadata:
            raise refused('context80_function_metadata_invalid')
        if not isinstance(row[10], str) or hashlib.sha256(row[10].encode()).hexdigest() != BODY_SHA256[name]:
            raise refused('context80_function_body_invalid')
    for key, expected in [('columns', COLUMNS), ('constraints', CONSTRAINTS)]:
        selected = {tuple(row[:2]) for row in expected}
        if [row for row in value.get(key, []) if tuple(row[:2]) in selected] != expected:
            raise refused('context80_' + key + '_invalid')
    if value.get('context80_triggers') != TRIGGERS:
        raise refused('context80_trigger_invalid')

COLUMNS = [['work_executions', 'context_version', 'smallint', True, '', '', '0', None],
 ['work_executions',
  'reference_artifact_id',
  'uuid',
  False,
  '',
  '',
  None,
  None]]
CONSTRAINTS = [['run_context_snapshots',
  'work_snapshot_admission80',
  't',
  'TRIGGER DEFERRABLE INITIALLY DEFERRED',
  True,
  True,
  True],
 ['work_executions',
  'work_context_request80',
  't',
  'TRIGGER DEFERRABLE INITIALLY DEFERRED',
  True,
  True,
  True],
 ['work_executions',
  'work_execution_reference_artifact_fk',
  'f',
  'FOREIGN KEY (company_id, work_item_id, reference_artifact_id) REFERENCES '
  'artifacts(company_id, work_item_id, id)',
  True,
  False,
  False],
 ['work_executions',
  'work_execution_reference_shape',
  'c',
  'CHECK (((context_version = 1) OR (reference_artifact_id IS NULL)))',
  True,
  False,
  False],
 ['work_executions',
  'work_executions_context_version_check',
  'c',
  'CHECK ((context_version = ANY (ARRAY[0, 1])))',
  True,
  False,
  False]]
