"""Explicit journal storage and bounded RPC to the already-held Linux lease."""
import base64
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import time
from uuid import UUID

from backup_private_database import Refused, private_binary
import recovery_journal_archive as archive
import recovery_confidential_journal as confidential

MAX_FRAME = 8192
CHUNK = 3072
FAILURE_CODES = frozenset(('journal_storage_selection_refused','journal_storage_generation_refused',
    'journal_lease_protocol_refused','journal_lease_deadline','journal_lease_not_held',
    'journal_archive_changed','journal_archive_refused'))


def require(value, code):
    if not value: raise Refused(code)


def selection(value):
    """None preserves the frozen legacy binding; an explicit volume never falls back."""
    if value is None: return None
    try:
        require(isinstance(value,dict) and set(value)=={'name','created_at','owner_id'},'journal_storage_selection_refused')
        require(isinstance(value['name'],str) and re.fullmatch('[A-Za-z0-9][A-Za-z0-9_.-]{0,127}',value['name'])
            and isinstance(value['owner_id'],str) and str(UUID(value['owner_id']))==value['owner_id']
            and UUID(value['owner_id']).int>0 and isinstance(value['created_at'],str)
            and 20<=len(value['created_at'])<=40
            and datetime.fromisoformat(value['created_at']).utcoffset()==timezone.utc.utcoffset(None),
            'journal_storage_selection_refused')
    except (KeyError,TypeError,ValueError):
        raise Refused('journal_storage_selection_refused') from None
    return dict(value)


def source_mount(runtime, selected):
    """Mount only the selected cold journal read-only at its existing logical path."""
    value=selection(selected); target=str(runtime/'state')
    if value is None: return 'type=bind,source='+target+',target='+target+',readonly'
    return 'type=volume,source='+value['name']+',target='+target+',readonly,volume-nocopy'


def verify_volume(inspector, container, selected, company, target):
    """Bind local storage generation and both owner labels to the exact controller mount."""
    value=selection(selected)
    require(value is not None,'journal_storage_selection_refused')
    template=('{"name":{{json .Name}},"created_at":{{json .CreatedAt}},"driver":{{json .Driver}},'
        '"scope":{{json .Scope}},"options":{{json .Options}},"source":{{json .Mountpoint}},'
        '"company":{{json (index .Labels "org.ortak.company")}},'
        '"owner":{{json (index .Labels "org.ortak.journal_owner")}}}')
    current=json.loads(inspector.run(inspector.docker('volume','inspect','--format',template,value['name']),limit=2048))
    mounts=[m for m in container['mounts'] if m['Type']=='volume']
    require(current.get('name')==value['name'] and current.get('created_at')==value['created_at']
        and current.get('driver')=='local' and current.get('scope')=='local'
        and current.get('options') in (None,{}) and current.get('company')==company
        and current.get('owner')==value['owner_id'] and isinstance(current.get('source'),str)
        and current['source'].startswith('/') and len(current['source'])<=512
        and container.get('journal_company')==company and container.get('journal_owner')==value['owner_id']
        and len(mounts)==1 and mounts[0]['Name']==value['name'] and mounts[0]['Source']==current['source']
        and mounts[0]['Destination']==str(target) and mounts[0]['RW'] is True,
        'journal_storage_generation_refused')
    return current


def confidential_selection(value):
    """Bind explicit opt-in to this exact reviewed validator; omission preserves the old contract."""
    if value is None: return None
    raw=Path(confidential.__file__).read_bytes()
    require(len(raw)<=32768 and isinstance(value,dict)
        and set(value)=={'format','validator_sha256'}
        and value['format']=='ortak-confidential-journal-recovery/1'
        and value['validator_sha256']==hashlib.sha256(raw).hexdigest(),
        'journal_storage_selection_refused')
    return dict(value)


def require_confidential_schema(value, schema):
    """77/78 require an explicit extension; historical ledgers never gain one implicitly."""
    selected=confidential_selection(value)
    require(type(schema) is int and ((schema in (77,78) and selected is not None)
        or (61<=schema<=76 and selected is None)), 'journal_storage_selection_refused')
    return selected


def lease_script(holder, *, confidential_reviewed=None):
    """Embed the reviewed closure and frozen opt-in in the existing stdlib-only lease process."""
    selected=confidential_selection(confidential_reviewed)
    module=Path(archive.__file__).read_bytes(); raw=Path(holder.__file__).read_bytes()
    require(len(module)<=32768 and len(raw)<=32768,'journal_storage_selection_refused')
    prefix=('import sys,types\n_archive=types.ModuleType("recovery_journal_archive")\n'
        'exec('+repr(module.decode())+',_archive.__dict__)\n'
        'sys.modules["recovery_journal_archive"]=_archive\n')
    if selected is not None:
        extension=Path(confidential.__file__).read_bytes()
        require(hashlib.sha256(extension).hexdigest()==selected['validator_sha256'],
            'journal_storage_selection_refused')
        prefix+=('_protected=types.ModuleType("recovery_confidential_journal")\n'
            'exec('+repr(extension.decode())+',_protected.__dict__)\n'
            'sys.modules["recovery_confidential_journal"]=_protected\n')
    prefix+='RECOVERY_CONFIDENTIAL_REVIEWED='+repr(selected is not None)+'\n'
    result=(prefix+raw.decode()).encode()
    require(len(result)<=98304,'journal_storage_selection_refused')
    return result


def reply(process, command):
    """Keep coalesced frames without consuming a following request's response."""
    data=getattr(process,'_journal_replies',bytearray())
    with selectors.DefaultSelector() as ready:
        ready.register(process.stdout,selectors.EVENT_READ)
        while b'\n' not in data:
            require(len(data)<=MAX_FRAME and ready.select(min(5,command.remaining())),'journal_lease_deadline')
            block=os.read(process.stdout.fileno(),MAX_FRAME+1-len(data))
            require(block,'journal_lease_protocol_refused');data.extend(block)
    line,rest=data.split(b'\n',1);process._journal_replies=bytearray(rest)
    require(len(line)<=MAX_FRAME,'journal_lease_protocol_refused')
    try: value=json.loads(line)
    except (ValueError,TypeError): raise Refused('journal_lease_protocol_refused') from None
    require(isinstance(value,dict),'journal_lease_protocol_refused')
    return value


def request(witness, action):
    """A saved witness or lost process cannot authorize a read, let alone a snapshot."""
    require(getattr(witness,'active',False) and witness.process.poll() is None,'journal_lease_not_held')
    require(action in (b'journal-status\n',b'journal-archive\n'),'journal_lease_protocol_refused')
    witness.process.stdin.write(action);witness.process.stdin.flush()


def status(witness):
    """Read the current volume through the process that still owns both Linux locks."""
    request(witness,b'journal-status\n')
    value=reply(witness.process,witness.gate.command)
    require(set(value)=={'status','journal'} and value['status']=='journal'
        and value['journal']==witness['linux_lease']['journal'],'journal_archive_changed')
    return value['journal']


def receive(witness, target):
    """Persist capped tar frames from the held lease, requiring its final digest and manifest."""
    command=witness.gate.command; old=command.deadline
    command.deadline=min(old,time.monotonic()+45)
    try:
        count=0; hashed=hashlib.sha256()
        with private_binary(target) as output:
            request(witness,b'journal-archive\n')
            while True:
                value=reply(witness.process,command)
                if set(value)=={'chunk'}:
                    try: block=base64.b64decode(value['chunk'],validate=True)
                    except (ValueError,TypeError): raise Refused('journal_lease_protocol_refused') from None
                    require(0<len(block)<=CHUNK,'journal_lease_protocol_refused')
                    count+=len(block)
                    require(count<=archive.MAX_ARCHIVE_BYTES,'journal_archive_refused')
                    output.write(block);hashed.update(block)
                    continue
                require(set(value)=={'status','bytes','sha256','archive'} and value['status']=='archive'
                    and value['bytes']==count and value['sha256']==hashed.hexdigest(),'journal_archive_changed')
                output.flush();os.fsync(output.fileno())
                break
        parent=os.open(target.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
        try:os.fsync(parent)
        finally:os.close(parent)
        require(witness.active and witness.process.poll() is None,'journal_lease_not_held')
        return {'path':target.name,'bytes':count,'sha256':hashed.hexdigest(),'archive':value['archive']}
    finally:command.deadline=old


def extract(path, target, expected, command):
    """Use the existing bounded/reaped file child, including for blocked extraction I/O."""
    from private_recovery_workspace_capture import bounded_action
    return bounded_action('journal-extract',{'archive':str(path),'destination':str(target),
        'expected':expected},command)


def extract_in_child(path, target, expected):
    """Restore raw evidence to a fresh inert host tree; source owner IDs remain provenance."""
    try:
        with path.open('rb') as incoming:
            result=archive.extract(incoming,target,expected_uid=10001)
    except archive.Refused: raise Refused('journal_archive_refused') from None
    require(result==expected,'journal_archive_changed')
    return {'status':'journal_raw_restored_offline','archive':result,
        'source_uid':10001,'restored_uid':os.getuid(),'owner_mapping':'inert_host_only',
        'runtime_activation':False,'original_volume_recreated':False,
        'activation_requires':['fresh_owned_local_journal_volume_and_explicit_generation_rebinding']}
