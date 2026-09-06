"""Restore explicit reviewed Honcho CHECK parse trees on an owned fresh target.

PostgreSQL17 pg_dump deparses the original VARCHAR array cast into SQL whose
reparse distributes casts over individual elements. Replaying the original
reviewed IN expression restores the exact source catalog; comparison is never
normalized and unknown source/target shapes refuse.
"""

import json
import re

from backup_private_database import Refused
from prepare_private_recovery import save

SPECS = (
    ('ortak_reviewed_operations', 'action', 'ck_ortak_reviewed_operations_ortak_reviewed_operation_action',
     ('publish', 'withdraw', 'expire')),
    ('ortak_reviewed_tombstones', 'reason', 'ck_ortak_reviewed_tombstones_ortak_reviewed_tombstone_reason',
     ('withdrawn', 'expired')),
    ('ortak_employee_reviewed_operations', 'action', 'ck_ortak_employee_reviewed_operations_action_kind',
     ('publish', 'withdraw')),
)
CATALOG_SQL = r"""
WITH wanted(relation,column_name) AS (VALUES ('ortak_reviewed_operations','action'),('ortak_reviewed_tombstones','reason'),
 ('ortak_employee_reviewed_operations','action')),
 targets AS (SELECT w.*,c.oid,c.relkind,n.nspname,pg_get_userbyid(c.relowner) owner,a.attnum,
   format_type(a.atttypid,a.atttypmod) data_type,a.attnotnull,a.attidentity,a.attgenerated
   FROM wanted w JOIN pg_namespace n ON n.nspname='public' JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=w.relation
   LEFT JOIN pg_attribute a ON a.attrelid=c.oid AND a.attname=w.column_name AND a.attnum>0 AND NOT a.attisdropped)
SELECT jsonb_build_object(
 'tables',coalesce((SELECT jsonb_agg(jsonb_build_object('schema',nspname,'name',relation,'owner',owner,
  'kind',relkind,'column',column_name,'type',data_type,'not_null',attnotnull,'identity',attidentity,'generated',attgenerated)
  ORDER BY relation) FROM targets),'[]'::jsonb),
 'checks',coalesce((SELECT jsonb_agg(jsonb_build_object('table',t.relation,'name',k.conname,
  'definition',pg_get_constraintdef(k.oid,true),'validated',k.convalidated,'local',k.conislocal,
  'inheritance_count',k.coninhcount,'no_inherit',k.connoinherit,'deferrable',k.condeferrable,'deferred',k.condeferred,
  'columns',(SELECT jsonb_agg(a.attname ORDER BY u.ordinality) FROM unnest(k.conkey) WITH ORDINALITY u(attnum,ordinality)
   JOIN pg_attribute a ON a.attrelid=k.conrelid AND a.attnum=u.attnum)) ORDER BY t.relation,k.conname)
  FROM targets t JOIN pg_constraint k ON k.conrelid=t.oid AND k.contype='c' AND t.attnum=ANY(k.conkey)),'[]'::jsonb))
"""


def expected(*, restored=False, employee=False):
    """Keep the historical pair exact; employee=True selects the reviewed third CHECK."""
    result = {'tables': [], 'checks': []}
    for table, column, name, values in (SPECS if employee else SPECS[:2]):
        array = ', '.join("'" + value + "'::character varying" + ('::text' if restored else '') for value in values)
        definition = 'CHECK (' + column + '::text = ANY (ARRAY[' + array + ']' + ('' if restored else '::text[]') + '))'
        result['tables'].append({'schema':'public','name':table,'owner':'ortak_honcho','kind':'r',
            'column':column,'type':'character varying(16)','not_null':True,'identity':'','generated':''})
        result['checks'].append({'table':table,'name':name,'definition':definition,'validated':True,'local':True,
            'inheritance_count':0,'no_inherit':False,'deferrable':False,'deferred':False,'columns':[column]})
    result['tables'].sort(key=lambda row: row['name'])
    result['checks'].sort(key=lambda row: (row['table'], row['name']))
    return result


def source_checks(command, database, snapshot=None):
    """Read the exact source CHECK family from the same read-only dump snapshot."""
    start = 'BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n'
    if snapshot:
        if not re.fullmatch(r'[0-9A-F]{8}-[0-9A-F]{8}-[0-9]+', snapshot):
            raise Refused('honcho_check_snapshot_refused')
        start += "SET TRANSACTION SNAPSHOT '" + snapshot + "';\n"
    row = json.loads(command.run('honcho-check-source',command.psql(database),sql=start+CATALOG_SQL+';\nROLLBACK;\n',ceiling=16384))
    if row not in (expected(), expected(employee=True), {'tables':[],'checks':[]}):
        raise Refused('honcho_check_source_allowlist_refused')
    return row


def admitted_target(source, target):
    """Legacy absence is explicit; partial families or any unrelated change refuse."""
    if source == {'tables':[],'checks':[]}:
        if target != source: raise Refused('honcho_check_legacy_target_changed')
        return []
    employee = source == expected(employee=True)
    if not employee and source != expected(): raise Refused('honcho_check_source_allowlist_refused')
    if target.get('tables') != source['tables'] or set(target) != {'tables','checks'}:
        raise Refused('honcho_check_target_allowlist_refused')
    if not isinstance(target['checks'],list) or len(target['checks'])!=len(source['checks']):
        raise Refused('honcho_check_target_allowlist_refused')
    restored = expected(restored=True, employee=employee)
    changed=[]
    for index,row in enumerate(target['checks']):
        if row==source['checks'][index]: continue
        if row!=restored['checks'][index]:
            raise Refused('honcho_check_target_allowlist_refused')
        # Catalog order puts the new employee table first, while the retained
        # spec indexes preserve the two historical repair identities.
        changed.append(next(i for i, spec in enumerate(SPECS) if (spec[0], spec[2]) == (row['table'], row['name'])))
    return changed


def guard_sql(document):
    """The complete allowlist is checked again inside the same DDL transaction."""
    literal=json.dumps(document,separators=(',',':')).replace("'","''")
    return "DO $honcho_restore$ BEGIN IF ("+CATALOG_SQL+") <> '"+literal+"'::jsonb THEN " \
        "RAISE EXCEPTION 'Honcho restore CHECK allowlist refused'; END IF; END $honcho_restore$;\n"


def repair_checks(command, database, source):
    """Consume caller's once-created target capability; no source/occupied target repair exists."""
    if getattr(command,'honcho_check_restore_authority',None)!=database:
        raise Refused('honcho_check_fresh_target_required')
    command.honcho_check_restore_authority=None
    generated = re.fullmatch(r'ortak_honcho_verify_[0-9a-f]{32}',database)
    offline = (database=='ortak_honcho_adapter_test' and getattr(command,'kind',None)=='honcho'
        and re.fullmatch(r'[0-9a-f]{32}',getattr(command,'operation',''))
        and getattr(command,'name',None)=='ortak-offline-'+command.operation+'-honcho')
    if not generated and not offline: raise Refused('honcho_check_target_scope_refused')
    # Caller has validated the isolated container; revalidate at the repair seam.
    if offline: command.inspect()
    target=json.loads(command.run('honcho-check-target',command.psql(database),
        sql='BEGIN READ ONLY;\n'+CATALOG_SQL+';\nROLLBACK;\n',ceiling=16384))
    if source is None:
        # Historical pre-D2a bundles have no selected CHECK witness. They cannot
        # acquire repair authority for a newly discovered reviewed table family.
        source={'tables':[],'checks':[]}
    changed=admitted_target(source,target)
    intent={'format':'ortak-private-honcho-check-restore/1','database':database,'source':source,
        'before':target,'changed_checks':[SPECS[index][2] for index in changed],
        'source_mutations':False,'schema_comparison_normalized':False}
    save(command.root/'honcho-check-repair-intent.json',intent)
    if changed:
        sql='BEGIN;\nSET LOCAL lock_timeout=\'2s\';\n'+guard_sql(target)
        for index in changed:
            table,column,name,values=SPECS[index]
            values=', '.join("'"+value+"'" for value in values)
            sql+='ALTER TABLE public.'+table+' DROP CONSTRAINT '+name+';\n'
            sql+='ALTER TABLE public.'+table+' ADD CONSTRAINT '+name+' CHECK ('+column+' IN ('+values+'));\n'
        sql+=guard_sql(source)+'COMMIT;\n'
        command.run('honcho-check-repair',command.psql(database),sql=sql,ceiling=128)
    after=json.loads(command.run('honcho-check-final',command.psql(database),
        sql='BEGIN READ ONLY;\n'+CATALOG_SQL+';\nROLLBACK;\n',ceiling=16384))
    if after!=source: raise Refused('honcho_check_source_shape_not_restored')
    result={'changed_checks':intent['changed_checks'],'exact_original_catalog_restored':True,
        'full_schema_verification_still_required':True,'source_mutations':False,'schema_comparison_normalized':False}
    save(command.root/'honcho-check-repair-complete.json',result)
    return result
