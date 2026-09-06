"""Read a retained disposable55432 fixture under a real schema lease; capture/restore synthetic exact files.

No live stack, provider, Docker image export or source database mutation. The
actual SQL lease and host-reader absence are exercised; Docker/application pause
ownership is an explicit fixture adapter, not a production containment claim.
"""
import argparse
from datetime import datetime,timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import time
from uuid import uuid4

from backup_private_database import Commands,private_directory
import check_schema_parity as bounded
from private_recovery_schema_lease import response
import private_recovery_workspace_capture as capture
import private_recovery_obligations as obligations
import recovery_lock_holder
import restore_private_recovery as restore
from rehearse_private_recovery_obligations import Fixture,WitnessCommands
from rehearse_private_recovery_schema73 import metadata
from recovery_workspace_layout import canonical,digest
from pause_private_recovery import overall_deadline

EVIDENCE=Path('/private/tmp/ortak-v0-evidence')
READER=EVIDENCE/'integrated74-build-f0f58f767b0a44bfb634b472c7c00fb5/binaries/ortak-workspace-reader'
READER_SHA='5edc64b9dad481c31604dc94bb3e67489a76af5c0dbac1c5a9b13b640ab79857'
NAMES=('private_recovery_workspace_capture.py','private_recovery_workspaces.py','private_recovery_obligations.py',
    'private_recovery_workspace_files.py','recovery_workspace_io.py','recovery_workspace_layout.py',
    'restore_workspace_files.py','recovery_lock_holder.py','restore_private_recovery.py',
    'check_private_recovery_gate.py','capture_private_recovery.py','prepare_private_recovery.py','register_private_recovery.py',
    'rehearse_private_recovery_workspace_files74.py')


def write(path,raw,mode=0o400):
    fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY|os.O_NOFOLLOW,mode)
    with os.fdopen(fd,'wb') as stream:
        stream.write(raw);stream.flush();os.fsync(stream.fileno())


def rehearse(database,company):
    operation=uuid4().hex
    output=private_directory(EVIDENCE/('g-workspace-files74-'+operation),fresh=True)
    command=Commands(output);command.deadline=time.monotonic()+120
    source=Fixture(private_directory(output/'sql',fresh=True),{'dbname':'postgres','user':'ortak','password':'ortak'},database)
    receipt={'status':'started','database':database,'company_id':company,'host':'127.0.0.1','port':55432,
        'source_database_writes':False,'private_stack_access':False,'provider_calls':0,'docker_actions':False,
        'source_sha256':{name:bounded.digest(Path(__file__).parent/name) for name in NAMES},
        'reader_sha256':READER_SHA,'reader_executed':False,'schema_lease_actual':True,
        'docker_application_containment':'fixture_adapter_only','automatic_activation':False}
    bounded.document(output/'intent.json',receipt)
    process=None
    try:
        capture.require(READER.is_file() and bounded.digest(READER)==READER_SHA,'fixture_reader_pin')
        meta=metadata(source)
        capture.require(obligations.schema_version(meta)==74,'fixture_schema74')
        process=subprocess.Popen(['/Applications/Postgres.app/Contents/Versions/17/bin/psql','-X','-qAt','-v','ON_ERROR_STOP=1'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,
            env=source.environment(database),start_new_session=True)
        process.stdin.write(b"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; DO $$BEGIN IF NOT pg_try_advisory_xact_lock_shared(7094711454081051697) THEN RAISE EXCEPTION 'schema busy'; END IF; END$$;\nSELECT jsonb_build_object('pid',pg_backend_pid(),'started',(SELECT backend_start FROM pg_stat_activity WHERE pid=pg_backend_pid()));\n")
        process.stdin.flush()
        owner=response(process,command)
        capture.require(set(owner)=={'pid','started'} and type(owner['pid']) is int,'fixture_schema_owner')
        initial=obligations.observe_workspace_layout(WitnessCommands(source),database,meta,company)
        roots={name:private_directory(output/name,fresh=True) for name in ('inputs','runs','bundle')}
        reader=output/'reader';write(reader,READER.read_bytes(),0o500)
        selected={'company_id':company,'input_root':str(roots['inputs']),'run_root':str(roots['runs']),
            'reader_binary':str(reader),'reader_sha256':READER_SHA,'reader_uid':os.getuid()}
        for name in ('inputs','runs'):
            write(roots[name]/('.ortak-workspace-'+name+'-v1'),f'ortak-workspace/v1:{company}\n'.encode())
        content=b'Selected brief';grants={}
        for row in initial['workspace_layout']['bindings']:
            grant=json.loads(row['grant_bytes']);grants[row['revision']]=grant
            revision=private_directory(roots['inputs']/row['revision'],fresh=True)
            for file in grant['files']:
                capture.require(file['bytes']==len(content) and file['sha256']==digest(content),'fixture_content_binding')
                write(revision/file['file_id'],content)
        company_root=private_directory(roots['runs']/company,fresh=True)
        for row in initial['workspace_layout']['runs']:
            grant=grants[row['revision']];run=row['run_id']
            write(company_root/(run+'.lock'),b'',0o600)
            if row['store_ref']:
                target=private_directory(company_root/run,fresh=True)
                for file in grant['files']:write(target/file['file_id'],content)
                write(target/'manifest.json',canonical(grant));target.chmod(0o500)
            else:
                target=private_directory(company_root/(run+'.preparing'),fresh=True)
                write(target/(grant['files'][0]['file_id']+'.partial'),content[:4],0o600)
        cold=output/'cold.sqlite';db=sqlite3.connect(cold)
        db.executescript("CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);CREATE TABLE events(start_key TEXT,sequence INTEGER);CREATE TABLE workspace_runs(start_key TEXT,grant_json TEXT);CREATE TABLE workspace_tool_calls(start_key TEXT,call_id TEXT,ordinal INTEGER,state TEXT,result_json TEXT,result_hash TEXT);")
        for ordinal,row in enumerate(initial['workspace_layout']['runs']):
            if row['store_ref']:
                key='fixture:'+row['run_id'];db.execute('INSERT INTO runs VALUES(?,?,0)',(key,row['status']))
                db.execute('INSERT INTO workspace_runs VALUES(?,?)',(key,canonical(grants[row['revision']]).decode()))
                db.execute("INSERT INTO workspace_tool_calls VALUES(?, 'call',1,'consumed',NULL,?)",(key,'a'*64))
        db.commit();db.close();cold.chmod(0o600)
        frozen_journal=recovery_lock_holder.staged_journal_status(cold)
        class Inspector:
            def run(self,args,limit): return command.run('ps-'+uuid4().hex,args,ceiling=limit)
        class Barrier(capture.HeldBarrierWitness):
            def workspace_observation(self,selected,deadline):
                capture.require(self.active and process.poll() is None,'fixture_schema_lease_lost')
                clients=json.loads(source.sql("SELECT jsonb_build_object('held',(SELECT count(*) FROM pg_stat_activity WHERE pid="+str(owner['pid'])+" AND backend_start='"+owner['started']+"'::timestamptz AND state='idle in transaction'),'others',(SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid NOT IN (pg_backend_pid(),"+str(owner['pid'])+")));"))
                capture.require(clients=={'held':1,'others':0},'fixture_uncontained_database_client')
                value=obligations.observe_workspace_layout(WitnessCommands(source),database,meta,company)
                capture.require(value==initial,'fixture_database_generation_changed')
                actual=recovery_lock_holder.staged_journal_status(cold)
                capture.require(actual==frozen_journal,'fixture_journal_changed')
                scan=capture.readers_absent(Inspector(),selected)
                value['closure_evidence']={'format':'ortak-workspace-files-closure/v1','barrier_id':self.barrier_id,
                    'selection_sha256':digest(canonical(selected)),'database_evidence_sha256':digest(canonical(value['database_evidence'])),
                    'journal_sha256':digest(canonical(actual)),'process_observation_sha256':digest(canonical({'schema_owner':owner,'readers':scan})),
                    'workspace_journal_pending':0,'live_reader_count':0,'live_writer_count':0}
                self.calls+=1
                return value
        barrier=Barrier({},None,process);barrier.calls=0
        row=capture.capture_workspace(selected,roots['bundle'],barrier,command)
        capture.require(barrier.calls==2,'fixture_same_query_callbacks')
        barrier.active=False
        process.stdin.write(b'ROLLBACK;\n\\q\n');process.stdin.flush()
        capture.require(process.wait(timeout=3)==0,'fixture_schema_release')
        full=private_directory(output/'full-bundle',fresh=True);roots['bundle'].rename(full/'workspace-files')
        row['path']='workspace-files'
        target=private_directory(output/'offline',fresh=True)
        proof=restore.restore_workspace_component(full,row,target)
        capture.require(proof['status']=='workspace_files_restored_offline','fixture_physical_restore_required')
        # Readback must still be independent from the original absolute roots.
        for name in ('inputs','runs'): roots[name].rename(output/('retained-original-'+name))
        reader.rename(output/'retained-original-reader')
        verify=capture.bounded_action('verify',{'bundle':str(full/'workspace-files'),'manifest_sha256':row['manifest_sha256']},command)
        capture.require(verify['database_evidence_sha256']==digest(canonical(initial['database_evidence'])),'fixture_database_archive_binding')
        receipt.update(status='verified',completed_at=datetime.now(timezone.utc).isoformat(),
            schema_sha256=meta['schema_sha256'],database_evidence_sha256=digest(canonical(initial['database_evidence'])),
            bindings=len(grants),runs=len(initial['workspace_layout']['runs']),readers=len(initial['workspace_layout']['readers']),
            capture=row,physical_restore=proof,callbacks=barrier.calls,original_paths_no_longer_present=True,
            archive_verified_again_after_original_move=True)
    except Exception as error:
        receipt.update(status='failed',exception_type=type(error).__name__)
        bounded.document(output/'receipt.json',receipt)
        raise
    finally:
        if process is not None:command.stop(process)
    bounded.document(output/'receipt.json',receipt)
    return output


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--retained-fixture-database',required=True)
    parser.add_argument('--company',required=True)
    args=parser.parse_args()
    capture.require(re.fullmatch(r'ortak_g_obligations_[0-9a-f]{32}',args.retained_fixture_database) is not None,'fixture_database_scope')
    capture.require(re.fullmatch(r'[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}',args.company) is not None,'fixture_company_scope')
    with overall_deadline(120):
        output=rehearse(args.retained_fixture_database,args.company)
    print(json.dumps({'status':'verified','receipt':str(output/'receipt.json'),'live_actions':False}))
