#!/usr/bin/env python3
"""Read-only exact source-resume validation; no start/stop/provider/auth discovery."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import sys
import urllib.error
import urllib.request
from uuid import uuid4

PAUSE = Path('/private/tmp/ortak-pause-for-g69-checks-v3.py')
PAUSE_SHA = '7661a94bcbccfdd0e640c77694ba48541cc95b449fcd2ce6bfd2fac781fafa13'
SESSIONS = None
COMPANY = 'a4013353-a84d-49a1-8d2b-10a1caf896fe'
WORK = '4419e4fc-58a3-4570-8f9d-11767a9ff1c5'
PROJECT = '3a06f7cf-ff9c-4deb-bb8c-7ef422eb9b6e'
REVISION = '2f21babf-92ef-4231-afdb-31a1bc8b013d'
RUN = 'e1209baf-9c50-4f74-a2e1-3acf73006aad'
ARTIFACT = '5c2a86f0-d6f3-44f7-ba21-718f8804f016'
FACT = '853d06ef-b81e-4e68-be60-fb985c988d68'
BACKUP = '20260905T224140Z_bbb3a310c90e40cd8ee65cf28c12ff1b'
VERIFY = 'ortak_verify_889d1819c49545abbb38ead92df62261'
SELECTORS = {
    'employees': "id='ada-private'",
    'employee_revisions': "id='" + REVISION + "'",
    'employee_runtime_bindings': "revision_id='" + REVISION + "'",
    'work_items': "id='" + WORK + "'",
    **{name: "work_item_id='" + WORK + "'" for name in
       ['work_item_history', 'work_acceptance_criteria', 'work_approvals', 'work_attachments']},
    'runs': "id='" + RUN + "'", 'artifacts': "id='" + ARTIFACT + "'",
    'runtime_work_outputs': "run_id='" + RUN + "'",
    'reviewed_memory_facts': "id='" + FACT + "'",
    **{name: "fact_id='" + FACT + "'" for name in ['reviewed_memory_exports',
        'reviewed_memory_export_jobs', 'reviewed_memory_export_receipts', 'reviewed_memory_export_commands']},
}
SEMANTIC = """SELECT jsonb_build_object(
 'employee',(SELECT jsonb_build_array(e.status,e.active_revision_id,e.lifecycle_epoch,b.model,b.options->>'reasoning_effort')
   FROM employees e JOIN employee_runtime_bindings b ON b.company_id=e.company_id AND b.revision_id=e.active_revision_id
   WHERE e.company_id='{company}' AND e.id='ada-private'),
 'work',(SELECT jsonb_build_array(state,version,project_id,completed_at IS NOT NULL) FROM work_items WHERE company_id='{company}' AND id='{work}'),
 'run',(SELECT jsonb_build_array(status,employee_revision_id) FROM runs WHERE company_id='{company}' AND id='{run}'),
 'artifact',(SELECT jsonb_build_array(encode(content_hash,'hex'),size_bytes,content_hash=sha256(content_bytes)) FROM artifacts WHERE company_id='{company}' AND id='{artifact}'),
 'fact',(SELECT jsonb_build_array(version,revoked_at IS NOT NULL,source_artifact_id,encode(sha256(convert_to(content,'UTF8')),'hex')) FROM reviewed_memory_facts WHERE company_id='{company}' AND id='{fact}'));
""".format(company=COMPANY,work=WORK,run=RUN,artifact=ARTIFACT,fact=FACT)


def load(owners, owners_sha256):
    row = PAUSE.lstat()
    if not (stat.S_ISREG(row.st_mode) and row.st_uid == os.getuid() and row.st_nlink == 1
            and stat.S_IMODE(row.st_mode) == 0o500 and row.st_size == 20292
            and hashlib.sha256(PAUSE.read_bytes()).hexdigest() == PAUSE_SHA):
        raise ValueError('selected_loader_refused')
    spec = importlib.util.spec_from_file_location('selected_pause', PAUSE)
    selected = importlib.util.module_from_spec(spec); spec.loader.exec_module(selected)
    gate, registry = selected.load_selected(owners, owners_sha256)
    return selected, gate, registry


def scoped_hash_sql():
    parts = []
    for table, predicate in SELECTORS.items():
        where = "company_id='" + COMPANY + "' AND " + predicate
        parts.append("'" + table + "',(SELECT jsonb_build_object('count',count(*),'sha256',"
            "encode(sha256(convert_to(COALESCE(string_agg(h,'' ORDER BY h),''),'UTF8')),'hex')) FROM "
            "(SELECT encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex') h FROM " + table
            + ' t WHERE ' + where + ') selected)')
    return 'SELECT jsonb_build_object(' + ','.join(parts) + ');'


def native(inspector, gate, expected):
    artifact = gate.native_ingress.bundle(inspector)
    gate.inventory.require(artifact == expected['artifact'], 'native_bundle_changed')
    ids = gate.native_ingress.candidates(inspector)
    gate.inventory.require(len(ids) == 1, 'native_current_owner_required')
    pid = ids[0]
    uid = inspector.run(['/bin/ps','-p',pid,'-o','uid='],limit=128).decode().strip()
    binary = inspector.run(['/bin/ps','-p',pid,'-o','comm='],limit=4096).decode().strip()
    started = inspector.run(['/bin/ps','-p',pid,'-o','lstart='],limit=128).decode().strip()
    inode = next(row['inode'] for row in artifact['entries'] if row['path'] == artifact['binary'])
    loaded = inspector.run(['/usr/sbin/lsof','-a','-p',pid,'-d','txt','-Fni'],limit=16384).decode().splitlines()
    gate.inventory.require(uid == str(os.getuid()) and binary == artifact['binary'] and any(
        loaded[i] == 'i'+str(inode) and loaded[i+1] == 'n'+binary for i in range(len(loaded)-1)), 'native_loaded_owner_refused')
    return {'pid':int(pid),'uid':int(uid),'started_at':started,'cwd':str(gate.inventory.STATE),
        'executable':binary,'inode':inode,'sha256':artifact['binary_sha256']}


def status(url):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    request = urllib.request.Request(url,method='GET')
    try:
        with opener.open(request,timeout=4) as response:
            body=response.read(4097)
            if len(body)>4096: raise ValueError('health_body_bound')
            return response.status
    except urllib.error.HTTPError as error:
        error.close()
        return error.code


def record(selected, gate, registry, output):
    expected = gate.load_preparation(Path(registry['preparation']))['observation']
    inspector = gate.inventory.Inventory(output)
    result = {'status':'started','at':datetime.now(timezone.utc).isoformat(),'source_actions':False,
        'provider_calls':False,'api_mutations':False,'session_provenance':'explicit_root_task_selection',
        'owners_sha256':registry['registry_sha256'],'capture_success_required_to_resume':False}
    processes={}
    for name in gate.inventory.NATIVE_WRITERS:
        current=inspector.native(name); old=registry['owners'][name]['live_process']
        gate.inventory.require(all(current[k]==old[k] for k in old if k not in ['pid','started_at'])
            and (current['pid'],current['started_at'])!=(old['pid'],old['started_at']), 'resumed_native_identity_changed')
        processes[name]={**current,'session_id':SESSIONS[name]}
    current=native(inspector,gate,expected['native_ingress'])
    old=expected['native_ingress']['process']
    gate.inventory.require((current['pid'],current['started_at'])!=(old['pid'],old['started_at']), 'native_not_newly_resumed')
    processes['native']={**current,'session_id':SESSIONS['native']}
    result['processes']=processes
    containers={}
    for name in gate.inventory.SERVICES:
        current=inspector.container(name); old=expected['containers'][name]
        gate.inventory.require(current['running'] and all(current[k]==old[k] for k in old if k!='started_at'), 'resumed_container_changed')
        gate.inventory.require((current['started_at']==old['started_at']) == (name in ['postgres','honcho_postgres']), 'store_restart_history_changed')
        containers[name]=current
    result['containers']=containers
    result['health']={'relay_liveness':status('http://127.0.0.1:8089/_liveness'),
        'relay_readiness':status('http://127.0.0.1:8089/_readiness'),
        'api_unauthenticated':status('http://127.0.0.1:8787/api/v1/employees')}
    gate.inventory.require(result['health']=={'relay_liveness':200,'relay_readiness':200,'api_unauthenticated':401}, 'resumed_health_failed')
    main=gate.Commands(gate.private_directory(output/'main',fresh=True));main.inspect()
    proof=json.loads((gate.inventory.STATE/'backups'/BACKUP/'manifest.json').read_text())
    gate.inventory.require(proof['status']=='verified' and proof['verification_database']==VERIFY
        and proof['archive_sha256']=='235e8225f9952a6b403f5b950024131c1d79d78c202271fbbd0443de5a8e2ce0', 'main_baseline_refused')
    witness={}
    for database,label in [('ortak','resumed'),(VERIFY,'paused-baseline')]:
        raw=main.run(label,main.psql(database),sql='BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n'
            + scoped_hash_sql()+'\n'+SEMANTIC+'ROLLBACK;\n',ceiling=16384).decode().splitlines()
        gate.inventory.require(len(raw)==2,'persistence_shape_refused')
        witness[label]={'rows':json.loads(raw[0]),'meaning':json.loads(raw[1])}
    gate.inventory.require(witness['resumed']==witness['paused-baseline'],'source_persistence_changed')
    gate.inventory.require(set(witness['resumed']['rows'])==set(SELECTORS)
        and all(0<r['count']<=64 for r in witness['resumed']['rows'].values()), 'persistence_row_bound')
    semantic=witness['resumed']['meaning']
    gate.inventory.require(semantic=={'employee':['active',REVISION,1,'gpt-5.6-sol','high'],
        'work':['completed',10,PROJECT,True], 'run':['completed',REVISION],
        'artifact':['b685e90b3c885b6d17661e708e9278f85a0b5240b0ce709e1c673970906bbbcd',377,True],
        'fact':[2,True,ARTIFACT,'9730e53e7b89f56c0e061b427c4fcec9338c9da27ffecf29fe46aab36a0509e9']}, 'accepted_fixture_state_changed')
    result['main_persistence']=witness
    honcho=gate.HonchoCommands(gate.private_directory(output/'honcho',fresh=True));honcho.container=containers['honcho_postgres']['id']
    sql="""BEGIN READ ONLY; SELECT jsonb_build_object(
      'records',(SELECT count(*) FROM ortak_reviewed_records WHERE company_id='{company}' AND project_id='{project}' AND record_id='{fact}'),
      'content',(SELECT count(*) FROM ortak_reviewed_record_content c JOIN ortak_reviewed_records r USING(workspace_id,project_id,record_id) WHERE r.company_id='{company}' AND r.project_id='{project}' AND r.record_id='{fact}'),
      'tombstones',(SELECT count(*) FROM ortak_reviewed_tombstones WHERE company_id='{company}' AND project_id='{project}' AND record_id='{fact}'),
      'operations',(SELECT jsonb_agg(o.action ORDER BY o.action) FROM ortak_reviewed_operations o JOIN ortak_reviewed_records r USING(workspace_id,project_id,record_id) WHERE r.company_id='{company}' AND r.project_id='{project}' AND r.record_id='{fact}')); ROLLBACK;""".format(company=COMPANY,project=PROJECT,fact=FACT)
    result['honcho_persistence']=json.loads(honcho.run('selected-reviewed-fact',honcho.psql(gate.HONCHO_DATABASE),sql=sql,ceiling=4096))
    gate.inventory.require(result['honcho_persistence']=={'records':1,'content':0,'tombstones':1,'operations':['publish','withdraw']}, 'honcho_withdrawal_persistence_changed')
    for name in gate.inventory.NATIVE_WRITERS:
        gate.inventory.require(inspector.native(name)=={k:v for k,v in processes[name].items() if k!='session_id'},'owner_changed_during_record')
    gate.inventory.require(native(inspector,gate,expected['native_ingress'])==current_native(processes), 'native_changed_during_record')
    result.update(status='passed',signed_api_read_performed=False,native_ui_acceptance_performed=False,
        offline_restore_authorized_from_failed_capture=False)
    gate.save(output/'receipt.json',result)
    return result


def current_native(processes):
    return {k:v for k,v in processes['native'].items() if k!='session_id'}


def main():
    global SESSIONS
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--owners',type=Path,required=True)
    parser.add_argument('--owners-sha256',required=True)
    parser.add_argument('--resume-dir',type=Path,required=True)
    parser.add_argument('--sessions-json',type=Path,required=True)
    args=parser.parse_args()
    selected,gate,registry=load(args.owners,args.owners_sha256)
    gate.inventory.require(args.resume_dir.parent==args.owners.parent
        and selected.re.fullmatch(r'source-resume-[0-9a-f]{32}',args.resume_dir.name)
        and args.sessions_json.parent==args.resume_dir,'explicit_resume_evidence_scope_required')
    gate.private_directory(args.resume_dir)
    SESSIONS=json.loads(selected.public_bytes(args.sessions_json,0o600,4096))
    gate.inventory.require(set(SESSIONS)==set(gate.inventory.NATIVE_WRITERS)|{'native'}
        and all(type(session) is int and session>0 for session in SESSIONS.values())
        and len(set(SESSIONS.values()))==5,'exact_root_session_selection_required')
    output=gate.private_directory(args.resume_dir/('validation-'+uuid4().hex),fresh=True)
    gate.save(output/'intent.json',{'action':'read_only_post_resume_validation','sessions':SESSIONS,'source_mutations':False})
    try:
        with selected.overall_deadline(120): result=record(selected,gate,registry,output)
    except Exception as error:
        code=str(error) if isinstance(error,(selected.Refused,gate.Refused)) else 'validation_failed'
        if len(code)>128 or not code.replace('_','').isalnum(): code='validation_failed'
        gate.save(output/'failure.json',{'status':'failed','code':code,'source_mutations':False})
        print(json.dumps({'status':'failed','evidence':str(output),'code':code}));return 1
    print(json.dumps({'status':'passed','receipt':str(output/'receipt.json'),
        'processes':{k:{'pid':v['pid'],'session':v['session_id']} for k,v in result['processes'].items()},
        'health':result['health'],'source_mutations':False}));return 0


if __name__=='__main__': raise SystemExit(main())
