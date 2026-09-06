"""Selected storage, actual held-lock RPC and inert recovery seams; no Docker or live stores."""
import copy
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import private_recovery_journal as subject
import recovery_journal_archive as archive
import recovery_lock_holder as holder
import check_private_recovery_gate as gate
import capture_private_recovery as capture
import private_recovery_payloads as payload
import restore_private_recovery as restore
import private_recovery_workspace_capture as child_owner
from backup_private_database import Commands


class JournalTests(unittest.TestCase):
    def setUp(self):
        # This fixture is the retained original74 journal/controller selection.
        for name in ('DEPLOYMENT76_SELECTION','SCORER_SELECTION'):
            context=patch.object(gate.inventory,name,None);context.start();self.addCleanup(context.stop)
        controller=list(gate.inventory.SERVICES['controller'])
        controller[0]='2ec604cb372a9ca42708d6ca8962b3d66195929f608278cb70b19ba5c339b630'
        context=patch.dict(gate.inventory.SERVICES,controller=tuple(controller))
        context.start();self.addCleanup(context.stop)
        self.temporary=tempfile.TemporaryDirectory();self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name).resolve();self.root.chmod(0o700)
        self.selected=copy.deepcopy(gate.inventory.JOURNAL_VOLUME)

    def test_exact_volume_projection_generation_and_controller_mounts(self):
        selected=self.selected;company=gate.inventory.COMPANY;target=self.root/'state'
        volume={'name':selected['name'],'created_at':selected['created_at'],'driver':'local','scope':'local',
            'options':None,'source':'/var/lib/docker/volumes/fixture/_data','company':company,'owner':selected['owner_id']}
        controller={'journal_company':company,'journal_owner':selected['owner_id'],'mounts':[
            {'Type':'volume','Name':selected['name'],'Source':volume['source'],'Destination':str(target),'RW':True}]}
        class Inspector:
            def docker(self,*args):return list(args)
            def run(self,args,limit):
                template=args[args.index('--format')+1]
                mapping={'.Name':'name','.CreatedAt':'created_at','.Driver':'driver','.Scope':'scope',
                    '.Options':'options','.Mountpoint':'source','(index .Labels "org.ortak.company")':'company',
                    '(index .Labels "org.ortak.journal_owner")':'owner'}
                # Preserve literal punctuation: the previous missing JSON brace
                # startup failure cannot pass by returning prebuilt JSON.
                return re.sub(r'\{\{json (.*?)\}\}',lambda m:json.dumps(volume[mapping[m[1]]]),template).encode()
        inspector=Inspector()
        self.assertEqual(subject.verify_volume(inspector,controller,selected,company,target),volume)
        for key,value in [('created_at','2026-09-06T03:49:26Z'),('owner','unowned'),('company','other'),
            ('options',{'device':'/elsewhere'}),('scope','global')]:
            old=volume[key];volume[key]=value
            with self.assertRaisesRegex(subject.Refused,'generation_refused'):
                subject.verify_volume(inspector,controller,selected,company,target)
            volume[key]=old
        controller['mounts'].append(dict(controller['mounts'][0],Destination=str(target/'journal.sqlite')))
        with self.assertRaises(subject.Refused):subject.verify_volume(inspector,controller,selected,company,target)

    def test_lease_mount_is_selected_read_only_nocopy_and_legacy_is_explicit(self):
        class Command:
            def docker(self,*args):return list(args)
        args=gate.lease_args(Command(),'fixture','sha256:'+'a'*64,'pass')
        mounts=[args[i+1] for i,k in enumerate(args[:-1]) if k=='--mount']
        self.assertEqual(mounts[0],subject.source_mount(gate.inventory.RUNTIME,self.selected))
        self.assertTrue(mounts[0].startswith('type=volume,'));self.assertTrue(mounts[0].endswith(',readonly,volume-nocopy'))
        self.assertNotIn('docker.sock',str(args));self.assertEqual(args[args.index('--network')+1],'none')
        self.assertIn('--init',args)  # The SIGALRM watchdog must not be namespace PID1.
        self.assertTrue(subject.source_mount(self.root,None).startswith('type=bind,'))
        with self.assertRaises(subject.Refused):subject.source_mount(self.root,{})

    def test_lease_bundle_supplies_archive_import_without_image_installation(self):
        code=subject.lease_script(holder)
        self.assertLessEqual(len(code),65536)
        namespace={'__name__':'lease_definition_fixture'}
        with patch.dict(sys.modules):
            sys.modules.pop('recovery_journal_archive',None)
            exec(compile(code,'<frozen-lease>','exec'),namespace)
            self.assertTrue(callable(namespace['recovery_journal_archive'].write))
            self.assertTrue(callable(namespace['serve']))

    def test_current_controller_daemon_bind_sources_are_exact_without_global_aliases(self):
        inventory=gate.inventory
        expected=inventory.expected_binds('controller')
        self.assertEqual(sum(source.startswith('/host_mnt/private/tmp/') for source,_,_ in expected),4)
        self.assertIn(('/run/host-services/docker.proxy.sock','/var/run/docker.sock',False),expected)
        self.assertTrue(all(not source.startswith('/host_mnt/') for source,_,_ in inventory.expected_binds('postgres')))
        identifier,name,image,_,_=inventory.SERVICES['controller']
        row={'id':identifier,'name':'/'+name,'image':image,'running':True,'networks':{},'ports':{},
            'mounts':[{'Type':'bind','Source':source,'Destination':target,'RW':rw} for source,target,rw in expected]}
        inspector=inventory.Inventory.__new__(inventory.Inventory)
        inspector.docker=lambda *args:list(args)
        inspector.run=lambda *args,**kwargs:json.dumps(row).encode()
        with patch.object(subject,'verify_volume',return_value={}) as verify:
            inspector.container('controller');verify.assert_called_once()
            for index,mount in enumerate(row['mounts']):
                original=mount['Source']
                mount['Source']=(original.removeprefix('/host_mnt') if original.startswith('/host_mnt/')
                    else '/Users/nambse/.docker/run/docker.sock')
                verify.reset_mock()
                with self.subTest(index=index),self.assertRaisesRegex(subject.Refused,'unapproved_mount_refused'):
                    inspector.container('controller')
                verify.assert_not_called();mount['Source']=original

    def start_lease(self):
        state=self.root/'source/state';state.mkdir(parents=True,mode=0o700)
        oauth=self.root/'source/oauth/ada-private';oauth.mkdir(parents=True,mode=0o700)
        oauth.parent.chmod(0o700)
        working=self.root/'working';working.mkdir(mode=0o700)
        for path in [state/'executor.lock',oauth/'oauth.lock']:
            path.touch(mode=0o600)
        journal=state/'journal.sqlite'
        with sqlite3.connect(journal) as database:
            database.executescript("CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);"
                "CREATE TABLE events(start_key TEXT,sequence INTEGER);"
                "INSERT INTO runs VALUES('fixture','completed',1);INSERT INTO events VALUES('fixture',1);")
        journal.chmod(0o600)
        script=('import os,sys;from pathlib import Path;import recovery_lock_holder as h;'
            # Host Python lacks Linux xattr support. This fixture substitutes
            # only the empty xattr probe; real descriptors/locks/bytes/RPC run.
            'os.listxattr=getattr(os,"listxattr",lambda fd:[]);'
            'h.serve(Path(sys.argv[1]),Path(sys.argv[2]),sys.stdin.buffer,sys.stdout.buffer)')
        process=subprocess.Popen([sys.executable,'-u','-c',script,str(state.parent),str(working)],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
            env={'PYTHONPATH':str(Path(__file__).resolve().parent),'PYTHONDONTWRITEBYTECODE':'1'},start_new_session=True)
        def stop():
            if process.poll() is None:process.kill()
            process.wait(timeout=3)
            for stream in (process.stdin,process.stdout,process.stderr):stream.close()
        self.addCleanup(stop)
        class Command:
            deadline=time.monotonic()+15
            def remaining(self):
                left=self.deadline-time.monotonic()
                subject.require(left>0,'journal_lease_deadline');return left
        command=Command();initial=subject.reply(process,command)
        class Witness(dict):pass
        witness=Witness(linux_lease=initial);witness.process=process;witness.active=True
        witness.gate=SimpleNamespace(command=command)
        return witness,state

    def test_real_held_process_exports_same_cold_bytes_and_releases_only_after_requests(self):
        witness,state=self.start_lease()
        before={p.name:p.read_bytes() for p in state.iterdir()}
        self.assertEqual(subject.status(witness),witness['linux_lease']['journal'])
        with (state/'executor.lock').open('rb') as lock:
            with self.assertRaises(BlockingIOError):fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        path=self.root/'journal-raw.tar';record=subject.receive(witness,path)
        with path.open('rb') as stream:
            restored=archive.extract(stream,self.root/'physical-restore',expected_uid=os.getuid())
        self.assertEqual(restored,record['archive'])
        self.assertEqual({p.name:p.read_bytes() for p in (self.root/'physical-restore').iterdir()},before)
        self.assertEqual({p.name:p.read_bytes() for p in state.iterdir()},before)
        self.assertEqual(set(restored['absent']),{'journal.sqlite-wal','journal.sqlite-shm'})
        with self.assertRaises(FileExistsError):subject.receive(witness,path)
        # A refused occupied target must be rejected before a new archive RPC,
        # otherwise the lease stream would be desynchronized.
        witness.process.stdin.write(b'release\n');witness.process.stdin.flush()
        self.assertEqual(subject.reply(witness.process,witness.gate.command),{'status':'released'})
        self.assertEqual(witness.process.wait(timeout=3),0)
        with (state/'executor.lock').open('rb') as lock:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)

    def test_serialized_or_stopped_witness_never_authorizes_stale_host_read(self):
        for witness in ({'linux_lease':{}},SimpleNamespace(active=False),
            SimpleNamespace(active=True,process=SimpleNamespace(poll=lambda:1))):
            with self.assertRaisesRegex(subject.Refused,'not_held'):subject.status(witness)

    def test_volume_capture_never_uses_original_host_state(self):
        backend=capture.Capture.__new__(capture.Capture)
        backend.output=self.root;backend.command=SimpleNamespace(remaining=lambda:10)
        backend.observation={'containers':{'controller':{'id':'a'*64,'image':'sha256:'+'b'*64}}}
        backend.held_witness=object()
        metadata={'fixture':'raw'}
        def stage(path,target,expected,command):
            target.mkdir(mode=0o700)
            with sqlite3.connect(target/'journal.sqlite') as db:db.execute('CREATE TABLE retained(value TEXT)')
            (target/'journal.sqlite').chmod(0o600)
        with patch.object(subject,'receive',return_value={'archive':metadata}) as receive, \
             patch.object(subject,'extract',side_effect=stage), \
             patch.object(payload,'sqlite_backup',wraps=payload.sqlite_backup) as backup:
            result=backend.journal()
        self.assertEqual(backup.call_args.args[0],self.root/'journal-raw/journal.sqlite')
        self.assertIs(receive.call_args.args[0],backend.held_witness)
        self.assertEqual(result['source_storage']['selection'],self.selected)
        self.assertEqual(result['integrity'],'ok')
        del backend.held_witness
        with self.assertRaisesRegex(subject.Refused,'not_held'):backend.journal()

    def test_production_offline_component_requires_physical_raw_copy_and_equal_rows(self):
        cold=self.root/'cold';cold.mkdir(mode=0o700)
        (cold/'executor.lock').touch(mode=0o600)
        with sqlite3.connect(cold/'journal.sqlite') as db:
            db.executescript("CREATE TABLE runs(start_key TEXT,status TEXT,sequence INTEGER);"
                "CREATE TABLE events(start_key TEXT,sequence INTEGER);CREATE TABLE retained(value TEXT);"
                "INSERT INTO runs VALUES('fixture','completed',1);INSERT INTO events VALUES('fixture',1);"
                "INSERT INTO retained VALUES('original');")
        (cold/'journal.sqlite').chmod(0o600)
        bundle=self.root/'bundle';bundle.mkdir(mode=0o700)
        component=payload.sqlite_backup(cold/'journal.sqlite',bundle/'journal.sqlite',cold=True)
        def raw_archive():
            stream=io.BytesIO()
            with patch.object(os,'listxattr',return_value=[],create=True):
                expected=archive.write(cold,stream,uid=os.getuid())
            # Synthetic Linux transfer headers exercise the actual host owner
            # mapping; no claim that this process acquired Linux UID10001.
            expected['root'].update(uid=10001,gid=10001)
            encoded=io.BytesIO();budget=archive.Budget(encoded)
            archive.header(budget,'.',expected['root'],0,root=True)
            for row in expected['files']:
                row.update(uid=10001,gid=10001)
                archive.header(budget,row['name'],row,row['bytes'])
                budget.write((cold/row['name']).read_bytes());budget.write(bytes(-row['bytes']%512))
            budget.write(archive.ZERO*2)
            raw=encoded.getvalue();path=bundle/'journal-raw.tar';path.write_bytes(raw);path.chmod(0o600)
            return {'path':path.name,'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'archive':expected}
        component.update(raw_archive=raw_archive(),source_storage={'kind':'docker_volume'},cold_companions=[])
        output=self.root/'restored';output.mkdir(mode=0o700)
        result=restore.restore_journal_component(bundle,component,output)
        self.assertEqual(result['raw_restore']['status'],'journal_raw_restored_offline')
        self.assertFalse(result['raw_restore']['runtime_activation'])
        self.assertEqual((output/'journal-raw/journal.sqlite').read_bytes(),(cold/'journal.sqlite').read_bytes())
        self.assertTrue((output/'journal-raw/executor.lock').is_file())
        with sqlite3.connect(cold/'journal.sqlite') as db:db.execute("UPDATE retained SET value='changed'")
        component['raw_archive']=raw_archive()
        refused=self.root/'mismatched';refused.mkdir(mode=0o700)
        with self.assertRaisesRegex(subject.Refused,'offline_journal_raw_changed'):
            restore.restore_journal_component(bundle,component,refused)
        # Raw files remain for diagnosis; no successful foundation is returned.
        self.assertTrue((refused/'journal-raw/journal.sqlite').is_file())

    def test_journal_extraction_stuck_child_is_killed_and_reaped_with_exact_receipt(self):
        marker=self.root/'admitted';script=self.root/'stuck-extractor.py'
        script.write_text('import json,sys,time\nfrom pathlib import Path\n'
            'request=json.loads(sys.stdin.buffer.readline())\n'
            'assert request["action"]=="journal-extract"\n'
            'Path('+repr(str(marker))+').write_text(request["action"])\n'
            'time.sleep(30)\n')
        command=Commands(self.root);command.deadline=time.monotonic()+0.5
        start=time.monotonic()
        with patch.object(child_owner,'__file__',str(script)):
            with self.assertRaisesRegex(subject.Refused,'deadline'):
                subject.extract(self.root/'raw.tar',self.root/'destination',{},command)
        self.assertEqual(marker.read_text(),'journal-extract')
        self.assertLess(time.monotonic()-start,3)
        finished=list(self.root.glob('workspace-child-*-finish.json'))
        self.assertEqual(len(finished),1)
        receipt=json.loads(finished[0].read_text())
        self.assertEqual(receipt['mode'],'journal-extract');self.assertEqual(receipt['state'],'contained')
        self.assertLess(receipt['returncode'],0)
        with self.assertRaises(ProcessLookupError):os.kill(receipt['pid'],0)


if __name__=='__main__':unittest.main()
