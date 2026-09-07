"""Live held-barrier composition and bounded private RPC for selected workspace capture."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time
from uuid import uuid4

from backup_private_database import Commands, Refused, private_directory, private_binary
import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import private_recovery_workspace_files as files
import recovery_lock_holder
import private_recovery_journal as selected_journal
import restore_workspace_files
from recovery_workspace_layout import canonical, digest, selection

MAX_FRAME = 1024**2
FAILURE_CODES=frozenset(('workspace_barrier_required','workspace_barrier_not_held','workspace_capture_selection_changed',
    'workspace_process_inventory_bound','workspace_process_inventory_refused','workspace_reader_still_running',
    'workspace_linux_lease_lost','workspace_linux_lease_changed','workspace_drain_changed','workspace_journal_not_closed',
    'workspace_schema_changed','workspace_database_generation_changed','workspace_capture_frame_bound',
    'workspace_capture_deadline','workspace_capture_child_protocol','workspace_capture_child_failed',
    'workspace_capture_verification_failed','workspace_action_refused','workspace_action_failed',
    'workspace_capture_schema_refused','workspace_capture_selection_required','workspace_capture_company_refused',
    'workspace_capture_foreign_scope','workspace_child_containment_unconfirmed','workspace_child_identity_refused'))
LEASE_FORMAT = '{"id":{{json .Id}},"image":{{json .Image}},"running":{{json .State.Running}},"pid":{{json .State.Pid}},"started":{{json .State.StartedAt}},"name":{{json .Name}}}'


def require(value, code):
    if not value: raise Refused(code)


def readers_absent(inspector, selected):
    """Read executable names only; never inspect other process arguments or environments."""
    raw=inspector.run(['/bin/ps','-ww','-axo','pid=,uid=,comm='],limit=512*1024).decode()
    rows=raw.splitlines()
    require(len(rows)<=4096,'workspace_process_inventory_bound')
    for row in rows:
        fields=row.split(None,2)
        require(len(fields)==3 and fields[0].isdigit() and fields[1].isdigit(), 'workspace_process_inventory_refused')
        # Every production reader executes this immutable absolute path. A
        # match under any UID refuses; a missing old PID is never the test.
        require(fields[2] != selected['reader_binary'], 'workspace_reader_still_running')
    return {'reader_binary':selected['reader_binary'],'reader_sha256':selected['reader_sha256'],
            'reader_uid':selected['reader_uid'],'live_reader_count':0}


class HeldBarrierWitness(dict):
    """Only a live context owns this callable; JSON serialization preserves the old witness shape."""
    def __init__(self, value, gate, process):
        super().__init__(value)
        self.gate, self.process, self.active = gate, process, True
        self.barrier_id=str(uuid4())
        self.lease_identity=None

    def workspace_observation(self, selected, deadline):
        """Constrain every nested command without shortening the outer capture lease."""
        old=self.gate.command.deadline
        try:
            self.gate.command.deadline=min(old,deadline)
            return self._workspace_observation(selected,deadline)
        finally:
            self.gate.command.deadline=old

    def _workspace_observation(self, selected, deadline):
        """Derive closure from current owners/leases and a cold journal, never from saved JSON."""
        selected=selection(selected)
        require(self.active and self.process.poll() is None,'workspace_barrier_not_held')
        require(selected==self.gate.preparation['observation'].get('workspace_selection')
            ==inventory.WORKSPACE_SELECTION, 'workspace_capture_selection_changed')
        self.gate.stopped_owners()
        current=json.loads(self.gate.inspector.run(self.gate.command.docker('inspect','--format',LEASE_FORMAT,
            self['container_name']),limit=2048))
        require(current['running'] is True and type(current['pid']) is int and current['pid']>0
            and current['name']=='/'+self['container_name']
            and current['image']==self.gate.preparation['observation']['containers']['controller']['image'],
            'workspace_linux_lease_lost')
        if self.lease_identity is None: self.lease_identity=current
        require(current==self.lease_identity,'workspace_linux_lease_changed')
        # This also verifies the actual schema PID/start is idle in transaction
        # and excludes only that retained owner from the connected-client gate.
        require(self.gate.drained_databases()==self['databases'],'workspace_drain_changed')
        process=readers_absent(self.gate.inspector,selected)
        journal=(selected_journal.status(self) if inventory.JOURNAL_VOLUME is not None else
            bounded_action('journal', {'root':str(inventory.RUNTIME)}, self.gate.command, deadline))
        require(journal==self['linux_lease']['journal'] and 'workspace' in journal
            and journal['workspace']['pending']==0 and journal['workspace']['invalid']==0,
            'workspace_journal_not_closed')
        # Each callback has its own directory: repeated observations never
        # collide with the ordinary obligation query's O_EXCL diagnostics.
        command=Commands(private_directory(self.gate.output/('workspace-'+uuid4().hex),fresh=True))
        command.deadline=deadline
        command.inspect()
        metadata=command.metadata('ortak','schema')
        previous=self.gate.preparation['observation']['main_database']
        require(metadata['schema_sha256']==previous['schema_sha256']
            and metadata['migration_checksums']==previous['migration_checksums'],'workspace_schema_changed')
        value=obligations.observe_workspace_layout(command,'ortak',metadata,selected['company_id'])
        obligations.workspaces.require_capture_scope(metadata,value['database_evidence'])
        require(value['database_evidence']==self['databases']['recovery_obligations'],'workspace_database_generation_changed')
        require(self.active and self.process.poll() is None,'workspace_barrier_not_held')
        value['closure_evidence']={'format':'ortak-workspace-files-closure/v1','barrier_id':self.barrier_id,
            'selection_sha256':digest(canonical(selected)),
            'database_evidence_sha256':digest(canonical(value['database_evidence'])),
            'journal_sha256':digest(canonical(journal)),
            'process_observation_sha256':digest(canonical({'reader':process,'lease':current,
                'owners_sha256':self.gate.registry['registry_sha256']})),
            'workspace_journal_pending':0,'live_reader_count':0,'live_writer_count':0}
        require(len(canonical(value))<=MAX_FRAME,'workspace_capture_frame_bound')
        return value


def read_frame(process, deadline):
    raw=bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout,selectors.EVENT_READ)
        while b'\n' not in raw:
            left=deadline-time.monotonic()
            require(left>0 and selector.select(left),'workspace_capture_deadline')
            block=os.read(process.stdout.fileno(),min(65536,MAX_FRAME+1-len(raw)))
            require(block and len(raw)+len(block)<=MAX_FRAME,'workspace_capture_child_protocol')
            raw.extend(block)
    require(raw.endswith(b'\n') and raw.count(b'\n')==1,'workspace_capture_child_protocol')
    return json.loads(raw)


def write_frame(process, value, deadline):
    raw=canonical(value)+b'\n'
    require(len(raw)<=MAX_FRAME,'workspace_capture_frame_bound')
    os.set_blocking(process.stdin.fileno(),False)
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdin,selectors.EVENT_WRITE)
        offset=0
        while offset<len(raw):
            left=deadline-time.monotonic()
            require(left>0 and selector.select(left),'workspace_capture_deadline')
            offset+=os.write(process.stdin.fileno(),raw[offset:offset+65536])


def child_record(command, path, value):
    """Exclusive durable receipts belong to a fresh operation output, never the sealed input bundle."""
    with private_binary(path) as stream:
        stream.write(canonical(value));stream.flush();os.fsync(stream.fileno())
    descriptor=os.open(command.root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:os.fsync(descriptor)
    finally:os.close(descriptor)


def start_child(command, process, mode):
    """Record exact process identity before sending any source path or file-copy request."""
    key='workspace-child-'+uuid4().hex
    identity=command.run(key+'-identity',['/bin/ps','-ww','-p',str(process.pid),'-o','pid=,uid=,lstart=,comm='],ceiling=4096).decode().strip()
    parts=identity.split(None,2)
    require(len(parts)==3 and parts[:2]==[str(process.pid),str(os.getuid())],'workspace_child_identity_refused')
    value={'pid':process.pid,'uid':os.getuid(),'identity':identity,'mode':mode,
        'python':sys.executable,'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'started_at':datetime.now(timezone.utc).isoformat(),'state':'started','source_service_actions':False}
    child_record(command,command.root/(key+'-started.json'),value)
    return key,value


def stop_child(command, process, started):
    """An unconfirmed kill/reap stays explicitly pending for root containment reconciliation."""
    key,value=started or ('workspace-child-unadmitted-'+uuid4().hex,{'pid':process.pid,'uid':os.getuid()})
    try:command.stop(process)
    except BaseException:
        child_record(command,command.root/(key+'-finish.json'),{**value,'state':'containment_unconfirmed',
            'root_reconciliation_required':True})
        raise Refused('workspace_child_containment_unconfirmed') from None
    child_record(command,command.root/(key+'-finish.json'),{**value,'state':'contained','returncode':process.returncode})


def capture_workspace(selected, output, barrier, command):
    """Copy in a kill/reap-bounded child; only the live parent supplies before/after observations."""
    require(isinstance(barrier,HeldBarrierWitness) and barrier.active,'workspace_barrier_required')
    selected=selection(selected)
    deadline=min(command.deadline,time.monotonic()+60)
    old_deadline=command.deadline;command.deadline=deadline
    process=subprocess.Popen([sys.executable,str(Path(__file__)),'--workspace-file-child'],
        stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True,
        env={'PATH':'/usr/bin:/bin:/usr/sbin:/sbin','LANG':'C','LC_ALL':'C','PYTHONDONTWRITEBYTECODE':'1'})
    started=None
    try:
        started=start_child(command,process,'capture')
        write_frame(process,{'selection':selected,'output':str(output)},deadline)
        for _ in range(2):
            require(read_frame(process,deadline)=={'event':'observe'},'workspace_capture_child_protocol')
            write_frame(process,barrier.workspace_observation(selected,deadline),deadline)
        result=read_frame(process,deadline)
        require(isinstance(result,dict) and set(result)=={'event','receipt'} and result['event']=='captured',
            'workspace_capture_child_protocol')
        require(process.wait(timeout=max(0.001,min(3,deadline-time.monotonic())))==0,'workspace_capture_child_failed')
        return {'path':output.name,**result['receipt']}
    finally:
        try:stop_child(command,process,started)
        finally:command.deadline=old_deadline


def bounded_action(action, request, command, deadline=None, *, observation=None):
    """Verify, physically restore, or inspect a cold journal in a bounded owned child."""
    require(action in ('verify','extract','journal','journal-extract','native-confidential-capture',
        'native-confidential-extract'),'workspace_action_refused')
    require((observation is not None)==(action=='native-confidential-capture'),'workspace_action_refused')
    deadline=min(command.deadline,time.monotonic()+60,deadline or command.deadline)
    old_deadline=command.deadline;command.deadline=deadline
    process=subprocess.Popen([sys.executable,str(Path(__file__)),'--workspace-file-action'],
        stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True,
        env={'PATH':'/usr/bin:/bin:/usr/sbin:/sbin','LANG':'C','LC_ALL':'C','PYTHONDONTWRITEBYTECODE':'1'})
    started=None
    try:
        started=start_child(command,process,action)
        write_frame(process,{'action':action,**request},deadline)
        for _ in range(3):
            value=read_frame(process,deadline)
            if value!={'event':'observe'}:break
            require(observation is not None,'workspace_action_failed')
            write_frame(process,observation(),deadline)
        else:raise Refused('workspace_action_failed')
        require(isinstance(value,dict) and set(value)=={'event','receipt'} and value['event']=='complete',
            'workspace_action_failed')
        require(process.wait(timeout=max(0.001,min(3,deadline-time.monotonic())))==0,'workspace_action_failed')
        return value['receipt']
    finally:
        try:stop_child(command,process,started)
        finally:command.deadline=old_deadline


def verified_metadata(bundle, manifest_sha256):
    """Keep verification and bounded metadata reads inside the watched child."""
    proof=files.verify(bundle,manifest_sha256)
    with files.Source(maximum=files.MAX_MANIFEST) as source:
        root=source.root(str(bundle))
        _,row=source.file(root,files.MANIFEST,'manifest',files.MAX_MANIFEST,(0o600,))
        raw=b''.join(source.blocks(source.entries['manifest'][1],row['bytes']))
        require(digest(raw)==manifest_sha256,'workspace_capture_verification_failed')
        value=json.loads(raw)
        source.check()
    return {**proof,**{key:value[key] for key in ('selection','archive_sha256',
        'database_evidence_sha256','workspace_layout_sha256','closure_evidence_sha256')}}


def child_main():
    """No database, socket, credentials or process authority enters the file-copy child."""
    def receive():
        raw=sys.stdin.buffer.readline(MAX_FRAME+1)
        require(raw.endswith(b'\n') and len(raw)<=MAX_FRAME,'workspace_capture_frame_bound')
        return json.loads(raw)
    def observe():
        print('{"event":"observe"}',flush=True)
        return receive()
    value=receive()
    if sys.argv[1:]==['--workspace-file-action']:
        action=value.pop('action')
        if action=='journal':
            require(set(value)=={'root'},'workspace_action_refused')
            result=recovery_lock_holder.journal_status(Path(value['root']))
        elif action=='journal-extract':
            require(set(value)=={'archive','destination','expected'},'workspace_action_refused')
            result=selected_journal.extract_in_child(Path(value['archive']),Path(value['destination']),value['expected'])
        elif action in ('native-confidential-capture','native-confidential-extract'):
            import private_recovery_native_confidential as native_store
            result=(native_store.capture_in_child(value,observe) if action=='native-confidential-capture'
                else native_store.extract_in_child(value))
        else:
            require(action in ('verify','extract') and set(value)==({'bundle','manifest_sha256'} |
                ({'destination'} if action=='extract' else set())), 'workspace_action_refused')
            result=(verified_metadata(Path(value['bundle']),value['manifest_sha256']) if action=='verify' else
                restore_workspace_files.extract(Path(value['bundle']),value['manifest_sha256'],Path(value['destination'])))
        print(json.dumps({'event':'complete','receipt':result}),flush=True)
        return
    require(set(value)=={'selection','output'},'workspace_capture_child_protocol')
    receipt=files.capture(value['selection'],Path(value['output']),observe)
    files.verify(Path(value['output']),receipt['manifest_sha256'])
    print(json.dumps({'event':'captured','receipt':receipt}),flush=True)


if __name__=='__main__':
    try:
        require(sys.argv[1:] in (['--workspace-file-child'],['--workspace-file-action']),'workspace_capture_mode_refused')
        child_main()
    except BaseException:
        print('{"event":"failed","code":"workspace_capture_child_failed"}',flush=True)
        raise SystemExit(1) from None
