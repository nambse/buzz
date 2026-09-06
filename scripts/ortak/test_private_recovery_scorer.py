"""Explicit credential-owner recovery seams; synthetic files and daemon replies only."""
from contextlib import ExitStack, contextmanager
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import private_recovery_inventory as inventory
import private_recovery_scorer as scorer
import pause_private_recovery as pause
import capture_private_recovery as capture
import restore_private_recovery as restore


@contextmanager
def fixture():
    with tempfile.TemporaryDirectory() as temporary, ExitStack() as patches:
        root=Path(temporary).resolve(); root.chmod(0o700)
        state=root/'selected'; runtime=root/'runtime'
        for directory in (state,runtime,state/'config',state/'semantic-stage',state/'semantic-stage/scorer-token'):
            directory.mkdir(mode=0o700)
        def write(path,value,mode=0o600):
            path.write_bytes(scorer.canonical(value));path.chmod(mode)
            return hashlib.sha256(path.read_bytes()).hexdigest()
        identity={'format':'ortak-oauth-identity/1','company_id':inventory.COMPANY,
            'employee_id':'ada-private','profile_ref':'selected-profile','credential_ref':'secret://synthetic/owner'}
        binding={'adapter':'hermes','profile_ref':identity['profile_ref'],'model':'selected-model',
            'workspace_ref':'none','credential_refs':[identity['credential_ref']],'options':{'reasoning_effort':'low'}}
        binding_hash=hashlib.sha256(scorer.canonical(binding)).hexdigest()
        deployment='77bde1e3-13de-44c0-992e-19ef74a9cf35'
        config={'company_id':inventory.COMPANY,'profiles':[{'employee_id':'ada-private','binding':binding,
            'directory':'/public/profile','oauth_directory':str(runtime/'oauth/ada-private')}],
            'semantic':{'deployment_id':deployment,'binding_sha256':binding_hash,'response_model':'selected-model'}}
        config_path=state/'config/semantic.json';config_hash=write(config_path,config,0o444)
        token=state/'semantic-stage/scorer-token/service-token';worker_token=state/'semantic-stage/worker-service-token'
        for target in (token,worker_token):target.write_bytes(b'synthetic-service-token');target.chmod(0o600)
        def mount(source,destination,rw):
            return {'Type':'bind','Source':str(source),'Destination':str(destination),'RW':rw,
                'Mode':'','Propagation':'rprivate'}
        container={'id':'a'*64,'name':'/synthetic-scorer','image':'sha256:'+'b'*64,
            'started_at':'2026-09-06T10:28:00.190186458Z','user':'10001:10001',
            'entrypoint':['python'],'cmd':['-m','ortak_hermes_bridge.semantic','--config','/private/semantic-config.json',
                '--token-file','/private/semantic-service-token','--port','8651','--enable-selected-semantic-oauth'],
            'mounts':[mount(config_path,'/private/semantic-config.json',False),
                mount(token,'/private/semantic-service-token',False),mount(runtime/'oauth',runtime/'oauth',True)],
            'port_bindings':{'8651/tcp':[{'HostIp':'127.0.0.1','HostPort':'8651'}]},
            'restart':{'Name':'no','MaximumRetryCount':0},'readonly':True,'init':True,'pid_mode':'',
            'network_mode':'selected-network','stop_timeout':45}
        receipt={'format':scorer.FORMAT,'company_id':inventory.COMPANY,'deployment_id':deployment,
            'binding_sha256':binding_hash,'config':{'path':str(config_path),'sha256':config_hash},
            'service_token_ref':{'path':str(token),'reference':'credential://synthetic/scorer'},
            'oauth':{'directory':str(runtime/'oauth/ada-private'),'identity':identity,
                'mount_source':str(runtime/'oauth'),'mount_destination':str(runtime/'oauth')},
            'container':container,'service_port':8651,'stop_seconds':45}
        receipt_path=state/'scorer.json';digest=write(receipt_path,receipt)
        worker_path=state/'worker.json'
        worker={'semantic':{'adapter':'hermes-codex','deployment':{'deployment_id':deployment,
            'binding_sha256':binding_hash,'bridge_token_ref':'credential://synthetic/scorer',
            'origin':'http://127.0.0.1:8651'}}}
        write(worker_path,worker)
        for key,value in {'STATE':state,'RUNTIME':runtime,'WORKER_CONFIG':worker_path,
            'SCORER_SELECTION':{'receipt_path':str(receipt_path),'receipt_sha256':digest}}.items():
            patches.enter_context(patch.object(inventory,key,value,create=True))
        selected=scorer.selection()
        files=scorer.file_refs(selected)
        running={'container':selected['selection']['container'],'state':{'running':True,'pid':123,
            'exit_code':0,'oom':False,'restarting':False,'finished_at':'0001-01-01T00:00:00Z'}}
        stopped=copy.deepcopy(running);stopped['state'].update(running=False,pid=0,finished_at='2026-09-06T11:00:00Z')
        expected={**selected,'files':files,'owner':running}
        yield SimpleNamespace(root=root,selected=selected,expected=expected,running=running,stopped=stopped,
            config=config,config_path=config_path,token=token,worker_token=worker_token,
            values={str(config_path):config,str(receipt_path):receipt,str(worker_path):worker},
            secrets=[files['service_token_ref'],files['worker_token']])


def inspector(owner,writer_rows=()):
    value=SimpleNamespace(docker=lambda *args:list(args))
    def run(args,*,limit):
        if args[0]=='ps':return ('\n'.join(row['id'] for row in writer_rows)).encode()
        if args[2]==scorer.OWNER_FORMAT:return scorer.canonical(owner)
        if args[2]==scorer.WRITER_FORMAT:return b'\n'.join(scorer.canonical(row) for row in writer_rows)
        raise AssertionError('unexpected synthetic daemon request')
    value.run=Mock(side_effect=run)
    return value


class ScorerRecoveryTests(unittest.TestCase):
    def test_selected_public_file_mapping_both_tokens_and_missing_selection(self):
        with fixture() as f:
            # This guard catches accidental token reads, even though all fixture bytes are synthetic.
            original=os.open
            def checked(path,*args,**kwargs):
                self.assertNotIn(Path(path),(f.token,f.worker_token))
                return original(path,*args,**kwargs)
            with patch.object(os,'open',side_effect=checked):
                self.assertEqual(scorer.file_refs(f.selected),f.expected['files'])
                scorer.bind_files(f.selected,f.values,f.secrets)
            for mutation in ('missing_scorer_token','missing_worker_token','wrong_ref','changed_owner'):
                values=copy.deepcopy(f.values);secrets=copy.deepcopy(f.secrets)
                if mutation=='missing_scorer_token':secrets=secrets[1:]
                elif mutation=='missing_worker_token':secrets=secrets[:1]
                elif mutation=='wrong_ref':values[str(inventory.WORKER_CONFIG)]['semantic']['deployment']['bridge_token_ref']='credential://other'
                else:values[str(f.config_path)]['profiles'][0]['employee_id']='unregistered'
                with self.subTest(mutation=mutation),self.assertRaises(inventory.Refused):
                    scorer.bind_files(f.selected,values,secrets)
            with patch.object(inventory,'SCORER_SELECTION',None):
                self.assertIsNone(scorer.selection())
                with self.assertRaises(inventory.Refused):scorer.bind_files(None,f.values,f.secrets)
                with self.assertRaises(inventory.Refused):scorer.resume_argv(inspector(f.stopped),f.expected,{})
            unrelated=f.config_path.parent/'unselected.json';unrelated.write_text('{}');unrelated.chmod(0o444)
            with self.assertRaises(inventory.Refused):scorer.public_file(unrelated.parent,unrelated.name,f.selected)
            f.config_path.chmod(0o600);f.config_path.write_text('{}')
            with self.assertRaises(inventory.Refused):scorer.file_refs(f.selected)

    def test_exact_daemon_template_owner_stop_and_replacement_writer(self):
        with fixture() as f:
            # Substitute real production Go-template actions, retaining every literal JSON delimiter.
            values={'.Id':f.stopped['container']['id'],'.Name':f.stopped['container']['name'],
                '.Image':f.stopped['container']['image'],'.State.StartedAt':f.stopped['container']['started_at']}
            for field,key in [('User','user'),('Entrypoint','entrypoint'),('Cmd','cmd'),('StopTimeout','stop_timeout')]:
                values['.Config.'+field]=f.stopped['container'][key]
            for field,key in [('PortBindings','port_bindings'),('RestartPolicy','restart'),('ReadonlyRootfs','readonly'),
                              ('Init','init'),('PidMode','pid_mode'),('NetworkMode','network_mode')]:
                values['.HostConfig.'+field]=f.stopped['container'][key]
            values['.Mounts']=f.stopped['container']['mounts']
            for field,key in [('Running','running'),('Pid','pid'),('ExitCode','exit_code'),('OOMKilled','oom'),
                              ('Restarting','restarting'),('FinishedAt','finished_at')]:values['.State.'+field]=f.stopped['state'][key]
            rendered=re.sub(r'\{\{json ([^}]+)\}\}',lambda match:json.dumps(values[match[1]]),scorer.OWNER_FORMAT)
            self.assertEqual(json.loads(rendered),f.stopped)
            proof=scorer.stopped(inspector(f.stopped),f.expected)
            self.assertTrue(proof['credential_writers_absent'])
            for field,value in [('running',True),('pid',9),('exit_code',137),('oom',True)]:
                changed=copy.deepcopy(f.stopped);changed['state'][field]=value
                with self.subTest(field=field),self.assertRaises(inventory.Refused):scorer.stopped(inspector(changed),f.expected)
            changed=copy.deepcopy(f.stopped);changed['container']['mounts'][0]['RW']=True
            with self.assertRaises(inventory.Refused):scorer.stopped(inspector(changed),f.expected)
            replacement={'id':'c'*64,'running':True,'pid':5,'mounts':[{'Type':'bind','RW':True,
                'Source':'/host_mnt'+f.selected['selection']['oauth']['mount_source']}]}
            with self.assertRaisesRegex(inventory.Refused,'scorer_oauth_writer_remains'):
                scorer.stopped(inspector(f.stopped,[replacement]),f.expected)
            replacement['mounts'][0]['RW']=False
            self.assertTrue(scorer.stopped(inspector(f.stopped,[replacement]),f.expected)['credential_writers_absent'])

    def test_cooperative_pause_is_exact_and_uncertain_stop_never_becomes_success(self):
        with fixture() as f:
            for fails in (False,True):
                value=pause.Pause.__new__(pause.Pause);value.expected={'scorer_owner':f.expected}
                value.api=SimpleNamespace(scorer=scorer);value.effects=[];value.event=Mock()
                value.deadline=time.monotonic()+60;value.remaining=Mock(return_value=45)
                value.command=SimpleNamespace(deadline=value.deadline,docker=lambda *args:list(args))
                current=copy.deepcopy(f.running)
                value.inspector=inspector(current);read=value.inspector.run.side_effect
                def run(args,*,limit):
                    if args[0]!='stop':return read(args,limit=limit)
                    if fails:raise inventory.Refused('command_deadline_exceeded')
                    current['state']=copy.deepcopy(f.stopped['state']);return b''
                value.inspector.run.side_effect=run
                if fails:
                    with self.assertRaises(inventory.Refused):value.stop_scorer()
                else:value.stop_scorer()
                commands=[call.args[0] for call in value.inspector.run.call_args_list if call.args[0][0]=='stop']
                self.assertEqual(commands,[['stop','--signal','SIGTERM','--timeout','-1','a'*64]])
                self.assertEqual(value.effects[0]['outcome'],'stop_not_yet_acknowledged' if fails else 'graceful_stop_acknowledged')
                value.remaining.assert_called_once_with(45)

    def test_secret_capture_requires_live_barrier_and_rechecks_writer_before_bytes(self):
        with fixture() as f:
            backend=capture.Capture.__new__(capture.Capture);backend.current=Mock()
            backend.observation={'scorer_owner':f.expected};backend.held_witness=None
            with patch.object(capture.payload,'secret_envelope') as envelope:
                with self.assertRaisesRegex(inventory.Refused,'scorer_held_barrier_required'):backend.secrets({})
                envelope.assert_not_called()
            proof=scorer.stopped(inspector(f.stopped),f.expected)
            class Witness(dict):pass
            witness=Witness(scorer=proof);witness.active=True
            witness.process=SimpleNamespace(poll=lambda:None);witness.gate=SimpleNamespace(inspector=inspector(f.stopped))
            self.assertEqual(scorer.require_held(witness,f.expected),proof)
            backend.held_witness=witness;witness.gate.inspector=inspector(f.running)
            with patch.object(capture.payload,'secret_envelope') as envelope:
                with self.assertRaisesRegex(inventory.Refused,'scorer_not_cleanly_stopped'):backend.secrets({})
                envelope.assert_not_called()
            witness.active=False
            with self.assertRaises(inventory.Refused):scorer.require_held(witness,f.expected)

    def test_offline_archive_retains_both_original_refs_without_restart_authority(self):
        with fixture() as f:
            proof=scorer.stopped(inspector(f.stopped),f.expected)
            bundle=inventory.STATE/'recovery-fixture-bundles'/('e'*32);bundle.mkdir(parents=True,mode=0o700)
            manifest={'format':restore.BUNDLE_FORMAT,'operation_id':bundle.name,'status':'captured',
                'fixture_only':True,'automatic_activation':False,'full_restore_executed':False,
                'components':dict.fromkeys(('databases','volumes','journal','public_artifacts','images'),{})}
            manifest['components']['scorer']=proof
            manifest['manifest_sha256']=restore.sha(manifest)
            target=bundle/'manifest.json';target.write_bytes(scorer.canonical(manifest));target.chmod(0o600)
            self.assertEqual(restore.load_bundle(target,fixture=True)['components']['scorer'],proof)
            public=f.root/'offline';public.mkdir(mode=0o700)
            for metadata in (f.expected['receipt'],f.expected['files']['config']):
                target=public/'selected'/metadata['path'].lstrip('/');target.parent.mkdir(parents=True,exist_ok=True)
                target.write_bytes(Path(metadata['path']).read_bytes());target.chmod(metadata['mode'])
            result=scorer.verify_offline(f.expected,proof,public,f.secrets)
            self.assertFalse(result['source_container_start_authorized']);self.assertFalse(result['runtime_activation'])
            for key in ('service_token_ref','worker_token'):
                missing=[row for row in f.secrets if row['path']!=f.expected['files'][key]['path']]
                with self.subTest(key=key),self.assertRaises(inventory.Refused):
                    scorer.verify_offline(f.expected,proof,public,missing)
            forged=copy.deepcopy(proof);forged['state']['pid']=123
            with self.assertRaises(inventory.Refused):scorer.verify_offline(f.expected,forged,public,f.secrets)
            self.assertEqual(scorer.resume_argv(inspector(f.stopped),f.expected,proof),['start','a'*64])
            resumed=copy.deepcopy(f.running);resumed['container']['started_at']='2026-09-06T12:00:00Z'
            self.assertEqual(scorer.verify_resumed(inspector(resumed),f.expected,proof),resumed)
            f.config_path.unlink()
            with self.assertRaises(FileNotFoundError):scorer.resume_argv(inspector(f.stopped),f.expected,proof)


if __name__=='__main__':unittest.main()
