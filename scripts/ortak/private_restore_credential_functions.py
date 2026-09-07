"""Temporary target-only compatibility for migration45's SQL check helper during pg_restore.

pg_restore sets an empty search_path. The historical immutable SQL body calls an
unqualified nested public function, which fails only for a populated JSON array.
The once-created target has no application users; every phase is bounded, failures
remain retained, and the original function definition/config must be restored.
"""

import hashlib
import json
import os

from backup_private_database import Refused
from init_private_stack import create_file

PRIMITIVE = r"""
    SELECT value ~ '^(credential|secret)://[A-Za-z0-9._:@#-]+(/[A-Za-z0-9._:@#-]+)*$'
       AND value !~ '(://|/)\.\.?(/|$)'
       AND length(split_part(value, '://', 2)) <= 512
"""
ARRAY = """
    SELECT jsonb_typeof(refs) = 'array'
       AND NOT EXISTS (
           SELECT 1
             FROM jsonb_array_elements(refs) AS element
            WHERE jsonb_typeof(element) <> 'string'
               OR NOT ortak_is_credential_ref(element #>> '{}')
       )
"""
SIGNATURE = 'public.ortak_all_credential_refs(jsonb)'
EXPECTED = [{'name': name, 'arguments': arguments, 'body_sha256': hashlib.sha256(body.encode()).hexdigest(),
    'language': 'sql', 'owner': 'ortak', 'result': 'boolean', 'volatility': 'i', 'strict': True,
    'parallel': 's', 'security_definer': False, 'leakproof': False, 'config': None}
    for name, arguments, body in [('ortak_all_credential_refs', 'jsonb', ARRAY), ('ortak_is_credential_ref', 'text', PRIMITIVE)]]
CATALOG_SQL = """
SELECT coalesce(jsonb_agg(jsonb_build_object(
 'name',p.proname,'arguments',oidvectortypes(p.proargtypes),
 'body_sha256',encode(sha256(convert_to(p.prosrc,'UTF8')),'hex'),
 'language',l.lanname,'owner',pg_get_userbyid(p.proowner),'result',format_type(p.prorettype,NULL),
 'volatility',p.provolatile,'strict',p.proisstrict,'parallel',p.proparallel,
 'security_definer',p.prosecdef,'leakproof',p.proleakproof,'config',p.proconfig
) ORDER BY p.proname,oidvectortypes(p.proargtypes)),'[]'::jsonb)
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_language l ON l.oid=p.prolang
WHERE n.nspname='public' AND p.proname IN ('ortak_all_credential_refs','ortak_is_credential_ref')
"""


def expected(configured=False):
    """No unknown overload, body, owner, function mode or original setting is admitted."""
    rows = [dict(row) for row in EXPECTED]
    if configured: rows[0]['config'] = ['search_path=pg_catalog, public']
    return rows


def guard_sql(configured=False):
    """Recheck the complete allowlist in the same transaction as each temporary ALTER."""
    document = json.dumps(expected(configured), separators=(',', ':')).replace("'", "''")
    return "DO $restore_guard$ BEGIN IF (" + CATALOG_SQL + ") <> '" + document + "'::jsonb THEN " \
        "RAISE EXCEPTION 'restore credential helper allowlist refused'; END IF; END $restore_guard$;\n"


def restore_sections(command, database, archive):
    """Caller must hold once-created fresh-target authority; never call for a source database."""
    intent = {'format': 'ortak-private-target-credential-restore/1', 'database': database,
        'temporary_function': SIGNATURE, 'original_proconfig': None,
        'temporary_search_path': ['pg_catalog', 'public'], 'source_mutations': False,
        'expected_functions': EXPECTED, 'phases': ['pre-data', 'configure', 'data', 'post-data', 'reset', 'verify']}
    def publish(name, value):
        create_file(command.root / name, json.dumps(value, indent=2) + '\n')
        descriptor = os.open(command.root, os.O_RDONLY)
        try: os.fsync(descriptor)
        finally: os.close(descriptor)

    publish('restore-compatibility-intent.json', intent)

    def record(phase, status):
        publish('restore-' + phase + '-' + status + '.json',
            {'phase': phase, 'status': status, 'database': database})

    def section(name):
        record(name, 'intent')
        command.run('restore-' + name, command.command('pg_restore', '--no-password', '--exit-on-error',
            '--single-transaction', '--section=' + name, '-h', '/var/run/postgresql', '-U', 'ortak', '-d', database), archive=archive)
        record(name, 'complete')

    section('pre-data')
    actual = json.loads(command.run('restore-original-functions', command.psql(database), sql=CATALOG_SQL + ';\n', ceiling=8192))
    if actual != EXPECTED: raise Refused('restore_credential_function_allowlist_refused')
    record('configure', 'intent')
    command.run('restore-configure', command.psql(database), sql='BEGIN;\n' + guard_sql()
        + 'ALTER FUNCTION ' + SIGNATURE + ' SET search_path TO pg_catalog, public;\n'
        + guard_sql(True) + 'COMMIT;\n', ceiling=128)
    record('configure', 'complete')
    section('data')
    section('post-data')
    record('reset', 'intent')
    command.run('restore-reset', command.psql(database), sql='BEGIN;\n' + guard_sql(True)
        + 'ALTER FUNCTION ' + SIGNATURE + ' RESET ALL;\n' + guard_sql() + 'COMMIT;\n', ceiling=128)
    record('reset', 'complete')
    final = json.loads(command.run('restore-final-functions', command.psql(database), sql=CATALOG_SQL + ';\n', ceiling=8192))
    if final != actual: raise Refused('restore_credential_function_not_restored')
    record('verify', 'complete')
    return {'temporary_function': SIGNATURE, 'original_proconfig_restored': True,
        'exact_function_catalog_restored': True, 'source_mutations': False, 'sections': ['pre-data', 'data', 'post-data']}
