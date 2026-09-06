"""Real watched FS capture/extraction with synthetic closure; production closure refusal seams."""
import copy
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from backup_private_database import Commands, Refused
import private_recovery_workspace_capture as subject
import private_recovery_workspaces as workspaces
import recovery_lock_holder as journal
import restore_private_recovery as restore
from test_private_recovery_workspace_files import Fixture


class WorkspaceCompositionTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name).resolve(); self.root.chmod(0o700)
        self.x=Fixture(self.root)
        self.command=Commands(self.root)

    def tearDown(self):
        for path,_,_ in os.walk(self.root): os.chmod(path,0o700)
        self.temp.cleanup()

    def barrier(self):
        x=self.x
        class SyntheticBarrier(subject.HeldBarrierWitness):
            def workspace_observation(self,selected,deadline):
                self.calls+=1
                if self.calls==2 and self.changed: self.active=False; raise Refused('workspace_barrier_not_held')
                return x.observe()
        value=SyntheticBarrier({},None,None); value.calls=0; value.changed=False
        return value

    def test_real_child_capture_and_outer_physical_restore_preserve_every_byte(self):
        barrier=self.barrier()
        # This exercises the actual RPC, production file helper and physical
        # foundation component, with synthetic containment (not a live pause).
        row=subject.capture_workspace(self.x.selected,self.x.output,barrier,self.command)
        self.assertEqual(barrier.calls,2)
        started=list(self.root.glob('workspace-child-*-started.json'))
        finished=list(self.root.glob('workspace-child-*-finish.json'))
        self.assertEqual(len(started),1);self.assertEqual(len(finished),1)
        self.assertEqual(json.loads(finished[0].read_bytes())['state'],'contained')
        row['path']='workspace-files'
        bundle=self.root/'full-bundle'; bundle.mkdir(mode=0o700)
        self.x.output.rename(bundle/'workspace-files')
        output=self.root/'restored'; output.mkdir(mode=0o700)
        result=restore.restore_workspace_component(bundle,row,output)
        self.assertEqual(result['status'],'workspace_files_restored_offline')
        self.assertEqual((output/'workspace-files'/'inputs'/self.x.revision/self.x.file_id).read_bytes(),self.x.content)
        self.assertEqual((output/'workspace-files'/'runs'/self.x.company/self.x.run/self.x.file_id).read_bytes(),self.x.content)
        self.assertEqual((output/'workspace-files'/'reader').read_bytes(),self.x.binary.read_bytes())
        self.assertEqual(json.loads((output/'workspace-files-verified.json').read_bytes()),result)
        self.assertFalse(result['automatic_activation'])
        self.assertFalse(result['physical_erasure'])
        with self.assertRaises(FileExistsError): restore.restore_workspace_component(bundle,row,output)

    def test_saved_json_or_closed_context_never_authorizes_copy(self):
        for value in ({},self.x.observe()):
            with self.assertRaisesRegex(Refused,'workspace_barrier_required'):
                subject.capture_workspace(self.x.selected,self.x.output,value,self.command)
        value=self.barrier(); value.active=False
        with self.assertRaisesRegex(Refused,'workspace_barrier_required'):
            subject.capture_workspace(self.x.selected,self.x.output,value,self.command)
        self.assertEqual(list(self.x.output.iterdir()),[])

    def test_after_copy_barrier_loss_keeps_failure_and_no_accepted_receipt(self):
        value=self.barrier(); value.changed=True
        with self.assertRaisesRegex(Refused,'workspace_barrier_not_held'):
            subject.capture_workspace(self.x.selected,self.x.output,value,self.command)
        self.assertFalse((self.x.output/subject.files.MANIFEST).exists())
        self.assertEqual(value.calls,2)

    def test_stuck_child_is_killed_and_reaped_under_whole_operation_deadline(self):
        real=subprocess.Popen; children=[]
        def stuck(*args,**kwargs):
            child=real(['/bin/sleep','30'],**kwargs); children.append(child); return child
        self.command.deadline=time.monotonic()+0.15
        start=time.monotonic()
        with patch.object(subject.subprocess,'Popen',side_effect=stuck):
            with self.assertRaisesRegex(Refused,'deadline'):
                subject.capture_workspace(self.x.selected,self.x.output,self.barrier(),self.command)
        self.assertLess(time.monotonic()-start,3)
        self.assertIsNotNone(children[0].returncode)
        self.assertTrue(children[0].stdout.closed)

    def test_containment_failure_retains_exact_started_identity(self):
        process=type('Child',(),{'pid':123456})()
        started=('workspace-child-fixture',{'pid':123456,'uid':os.getuid(),'identity':'exact-fixture-start'})
        with patch.object(self.command,'stop',side_effect=subprocess.TimeoutExpired('fixture',3)):
            with self.assertRaisesRegex(Refused,'workspace_child_containment_unconfirmed'):
                subject.stop_child(self.command,process,started)
        value=json.loads((self.root/'workspace-child-fixture-finish.json').read_bytes())
        self.assertEqual(value['state'],'containment_unconfirmed')
        self.assertEqual(value['identity'],'exact-fixture-start')
        self.assertTrue(value['root_reconciliation_required'])

    def test_physical_failure_cannot_be_promoted_from_tar_verification(self):
        row=self.x.capture();row['path']='workspace-files'
        bundle=self.root/'full-bundle';bundle.mkdir(mode=0o700)
        self.x.output.rename(bundle/'workspace-files')
        output=self.root/'failed'; output.mkdir(mode=0o700)
        with patch.object(subject,'bounded_action',return_value={'status':'workspace_files_verified_offline'}):
            with self.assertRaisesRegex(Refused,'offline_workspace_restore_incomplete'):
                restore.restore_workspace_component(bundle,row,output)
        self.assertFalse((output/'workspace-files-verified.json').exists())

    def test_long_executable_path_is_not_a_false_absence(self):
        reader=self.root/('long-selected-'+('x'*150))/'ortak-workspace-reader'
        class Inspector:
            def run(self,args,limit):
                self.args=args
                return f'93339 {os.getuid()} {reader}\n'.encode()
        inspector=Inspector(); selected={**self.x.selected,'reader_binary':str(reader)}
        with self.assertRaisesRegex(Refused,'workspace_reader_still_running'):
            subject.readers_absent(inspector,selected)
        self.assertIn('-ww',inspector.args)

    def test_current_closure_lost_lease_journal_database_and_live_reader_refuse(self):
        x=self.x; observation=x.observe()
        database=observation['database_evidence']
        meta={'schema_sha256':'a'*64,'migration_checksums':[[74,'b'*64]],
            'tables':{name:len(rows) for name,rows in database['tables'].items()}}
        heldjournal={'workspace':{'pending':0,'invalid':0}}
        lease={'id':'c'*64,'image':'d'*64,'running':True,'pid':32,'started':'exact-start','name':'/fixture-lease'}
        class Process:
            def poll(self): return None
        class Inspector:
            def run(self,args,limit): return json.dumps(lease).encode() if 'inspect' in args else b''
        class Gate:
            command=self.command
            output=self.root
            registry={'registry_sha256':'e'*64}
            preparation={'observation':{'workspace_selection':x.selected,'main_database':meta,
                'containers':{'controller':{'image':'d'*64}}}}
            inspector=Inspector()
            def stopped_owners(self): self.stopped=getattr(self,'stopped',0)+1
            def drained_databases(self): return {'recovery_obligations':database}
        gate=Gate(); barrier=subject.HeldBarrierWitness({'container_name':'fixture-lease',
            'linux_lease':{'journal':heldjournal},'databases':{'recovery_obligations':database}},gate,Process())
        class Db:
            def __init__(self,*args): pass
            def inspect(self): pass
            def metadata(self,*args): return meta
        with patch.object(subject.inventory,'WORKSPACE_SELECTION',x.selected), \
             patch.object(subject,'Commands',Db), \
             patch.object(subject.selected_journal,'status',return_value=heldjournal) as cold, \
             patch.object(subject,'bounded_action',side_effect=AssertionError('stale host journal read')), \
             patch.object(subject.obligations,'observe_workspace_layout',return_value={
                'database_evidence':database,'workspace_layout':x.layout}):
            first=barrier.workspace_observation(x.selected,time.monotonic()+5)
            second=barrier.workspace_observation(x.selected,time.monotonic()+5)
            self.assertEqual(first,second)
            self.assertEqual(gate.stopped,2)
            self.assertEqual(len(list(self.root.glob('workspace-*'))),2)
            for key,value,code in [('running',False,'lease_lost'),('pid',45,'lease_changed')]:
                old=lease[key]; lease[key]=value
                with self.assertRaisesRegex(Refused,code): barrier.workspace_observation(x.selected,time.monotonic()+5)
                lease[key]=old
            cold.return_value={'workspace':{'pending':1,'invalid':0}}
            with self.assertRaisesRegex(Refused,'journal_not_closed'): barrier.workspace_observation(x.selected,time.monotonic()+5)
            cold.return_value=heldjournal
            with patch.object(gate,'drained_databases',return_value={'different':True}):
                with self.assertRaisesRegex(Refused,'drain_changed'): barrier.workspace_observation(x.selected,time.monotonic()+5)
            with patch.object(subject,'readers_absent',side_effect=Refused('workspace_reader_still_running')):
                with self.assertRaisesRegex(Refused,'reader_still_running'): barrier.workspace_observation(x.selected,time.monotonic()+5)
            barrier.active=False
            with self.assertRaisesRegex(Refused,'barrier_not_held'): barrier.workspace_observation(x.selected,time.monotonic()+5)

    def test_populated_database_requires_explicit_files_even_after_preparation(self):
        meta={'migration_checksums':[[74,'a'*64]],'tables':dict.fromkeys(workspaces.TABLE_KEYS,0)}
        workspaces.require_capture_selection(meta,None,self.x.company)
        for table in workspaces.TABLE_KEYS:
            value=copy.deepcopy(meta); value['tables'][table]=1
            with self.assertRaisesRegex(Refused,'selection_required'):
                workspaces.require_capture_selection(value,None,self.x.company)
        workspaces.require_capture_selection(meta,self.x.selected,self.x.company)
        evidence={'schema_version':74,'tables':dict.fromkeys(workspaces.TABLE_KEYS,[])}
        workspaces.require_capture_scope(meta,evidence)
        for name in workspaces.TABLE_KEYS:
            foreign=copy.deepcopy(meta);foreign['tables'][name]=1
            with self.assertRaisesRegex(Refused,'foreign_scope'):workspaces.require_capture_scope(foreign,evidence)
        with self.assertRaisesRegex(Refused,'company_refused'):
            workspaces.require_capture_selection(meta,self.x.selected,'00000000-0000-0000-0000-000000000099')


class JournalClosureTests(unittest.TestCase):
    def test_cold_workspace_journal_requires_settled_no_result_bytes_and_same_history_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory).resolve()/'journal.sqlite'
            db=sqlite3.connect(path)
            db.executescript("""CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);
                CREATE TABLE events(start_key TEXT,sequence INTEGER);
                INSERT INTO runs VALUES('owned','completed',0);
                CREATE TABLE workspace_runs(start_key TEXT,grant_json TEXT);
                CREATE TABLE workspace_tool_calls(start_key TEXT,call_id TEXT,ordinal INTEGER,state TEXT,result_json TEXT,result_hash TEXT);
                INSERT INTO workspace_runs VALUES('owned','fixture-grant');
                INSERT INTO workspace_tool_calls VALUES('owned','call',1,'consumed',NULL,'hash');""")
            db.commit()
            first=journal.staged_journal_status(path)
            self.assertEqual(first['workspace']['workspace_tool_calls'],1)
            for state,result in [('pending',None),('resolved','text'),('consumed','text'),('interrupted','text')]:
                db.execute('UPDATE workspace_tool_calls SET state=?,result_json=?',(state,result)); db.commit()
                with self.assertRaises(ValueError): journal.staged_journal_status(path)
            db.execute("UPDATE workspace_tool_calls SET state='interrupted',result_json=NULL,result_hash='retained-new-hash'"); db.commit()
            self.assertNotEqual(first['workspace']['sha256'],journal.staged_journal_status(path)['workspace']['sha256'])
            db.execute("UPDATE workspace_tool_calls SET start_key='foreign'"); db.commit()
            with self.assertRaises(ValueError): journal.staged_journal_status(path)
            db.close()


if __name__=='__main__': unittest.main()
