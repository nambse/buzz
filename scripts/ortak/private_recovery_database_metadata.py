"""Selected database settings and sequences for a quiesced capture, without password discovery."""

import json
import re

from private_recovery_inventory import require
from backup_private_honcho import CONTENT_SQL, MAX_ROWS

# Role passwords are never queried. Exact selected configuration values can be
# sensitive, so the complete result is routed only into the encrypted envelope.
EXTRAS_SQL = r"""
BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SELECT jsonb_build_object(
 'role',(SELECT jsonb_build_object('name',rolname,'superuser',rolsuper,'inherit',rolinherit,
  'create_role',rolcreaterole,'create_database',rolcreatedb,'login',rolcanlogin,
  'replication',rolreplication,'bypass_rls',rolbypassrls,'connection_limit',rolconnlimit,
  'valid_until',rolvaliduntil,'settings',rolconfig) FROM pg_roles WHERE rolname=current_user),
 'database',(SELECT jsonb_build_object('owner',pg_get_userbyid(datdba),'encoding',pg_encoding_to_char(encoding),
  'collation',datcollate,'ctype',datctype,'locale_provider',datlocprovider,'connection_limit',datconnlimit,
  'tablespace',(SELECT spcname FROM pg_tablespace WHERE oid=d.dattablespace)) FROM pg_database d WHERE datname=current_database()),
 'settings',(SELECT jsonb_agg(jsonb_build_object('database_specific',s.setdatabase<>0,
  'role_specific',s.setrole<>0,'values',s.setconfig) ORDER BY s.setdatabase<>0,s.setrole<>0)
  FROM pg_db_role_setting s WHERE s.setdatabase IN (0,(SELECT oid FROM pg_database WHERE datname=current_database()))
  AND s.setrole IN (0,(SELECT oid FROM pg_roles WHERE rolname=current_user))),
 'sequences',(SELECT jsonb_object_agg(name,body) FROM (
  SELECT format('%I.%I',n.nspname,c.relname) name,
   ((xpath('/table/row/body/text()',query_to_xml(format(
    'SELECT jsonb_build_object(''last_value'',last_value,''is_called'',is_called) body FROM %I.%I',
    n.nspname,c.relname),false,false,'')))[1]::text)::jsonb body
  FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='public' AND c.relkind='S'
 ) sequences)
);
ROLLBACK;
"""


def selected_extras(commands, database, label):
    """Only already-guarded selected source/verification connections reach this query."""
    value = json.loads(commands.run(label, commands.psql(database), sql=EXTRAS_SQL, ceiling=65536))
    require(set(value) == {'role', 'database', 'settings', 'sequences'}
        and value['role']['name'] in ('ortak', 'ortak_honcho')
        and value['database']['owner'] == value['role']['name']
        and value['database']['tablespace'] == 'pg_default'
        and len(value['sequences'] or {}) <= 512, 'selected_database_settings_refused')
    require(all(re.fullmatch(r'public\.[A-Za-z0-9_]+', name) and set(row) == {'last_value', 'is_called'}
                and type(row['last_value']) is int and type(row['is_called']) is bool
                for name, row in (value['sequences'] or {}).items()), 'sequence_inventory_refused')
    return value


def selected_content(commands, database, label, expected_counts):
    """Verify complete logical table bytes for the already-quiesced exact source or restore."""
    require(0 < len(expected_counts) <= 2048
        and all(type(n) is int and n >= 0 for n in expected_counts.values())
        and sum(expected_counts.values()) <= MAX_ROWS, 'content_inventory_bound')
    value = json.loads(commands.run(label, commands.psql(database),
        sql='BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n' + CONTENT_SQL + '\nROLLBACK;\n',
        ceiling=512 * 1024))
    require(set(value) == set(expected_counts) and all(isinstance(h, str)
        and re.fullmatch(r'[0-9a-f]{64}', h) for h in value.values()), 'content_inventory_refused')
    return value


def verified_content(commands, source, restored, expected):
    """Counts alone cannot certify preserved receipts: require identical logical bytes in both stores."""
    source_rows = selected_content(commands, source, 'source-content-check', expected['expected']['tables'])
    restored_rows = selected_content(commands, restored, 'restored-content-check', expected['restored']['tables'])
    require(source_rows == restored_rows, 'database_logical_rows_restore_mismatch')
    return source_rows
