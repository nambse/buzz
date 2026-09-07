"""Pinned79 conversation snapshot guard catalog, independent of compared databases."""
import hashlib

FUNCTIONS = {'ortak_conversation_plaintext79': ['value text',
                                    'sql',
                                    'i',
                                    True,
                                    False,
                                    False,
                                    's',
                                    None,
                                    'text'],
 'ortak_conversation_snapshot_admission79': ['',
                                             'plpgsql',
                                             'v',
                                             False,
                                             False,
                                             False,
                                             'u',
                                             None,
                                             'trigger'],
 'ortak_run_conversation_context_current': ['company uuid, run uuid',
                                            'plpgsql',
                                            's',
                                            False,
                                            False,
                                            False,
                                            'u',
                                            None,
                                            'boolean']}
BODY_SHA256 = {'ortak_conversation_plaintext79': '2875b4f04c551c1a38afdb858814026b1e5ca3291c52bc7669d8ce4ce0af9d2d',
 'ortak_conversation_snapshot_admission79': 'f65e257f82f413d506e795de583dbfe7d742cc66a8e990129ab9d3f7e799fec9',
 'ortak_run_conversation_context_current': '942ac91d78398224e1eb2d791523a31f5c839ef832eee30a664f5264bd7525e2'}
TRIGGERS = [['run_context_snapshots', 'ortak_conversation_snapshot_admission79',
             'O', 5, True, True, 'public', 'ortak_conversation_snapshot_admission79',
             '', 0, [], True, False, None]]
CLOSED_BODY = "\nBEGIN\n    RAISE EXCEPTION 'ortak: schema79 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';\nEND\n"


def check(value, refused):
    """Reject missing guards or equal catalogs containing altered authority code."""
    functions = {row[0]: row for row in value.get('functions', [])}
    for name, metadata in FUNCTIONS.items():
        row = functions.get(name)
        if row is None or len(row) != 11 or row[1:10] != metadata:
            raise refused('context79_function_metadata_invalid')
        if not isinstance(row[10], str) or hashlib.sha256(row[10].encode()).hexdigest() != BODY_SHA256[name]:
            raise refused('context79_function_body_invalid')
    if value.get('context79_triggers') != TRIGGERS:
        raise refused('context79_trigger_invalid')
