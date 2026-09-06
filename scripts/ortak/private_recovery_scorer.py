"""Explicit scorer credential-writer ownership; no scoring, credentials or starts in preparation."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
from uuid import UUID

import private_recovery_inventory as inventory

FORMAT = 'ortak-private-recovery-scorer/1'
CONTAINER_KEYS = {'id','name','image','started_at','user','entrypoint','cmd','mounts',
    'port_bindings','restart','readonly','init','pid_mode','network_mode','stop_timeout'}
OWNER_FORMAT = ('{"container":{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Image}},'
    '"started_at":{{json .State.StartedAt}},"user":{{json .Config.User}},'
    '"entrypoint":{{json .Config.Entrypoint}},"cmd":{{json .Config.Cmd}},"mounts":{{json .Mounts}},'
    '"port_bindings":{{json .HostConfig.PortBindings}},"restart":{{json .HostConfig.RestartPolicy}},'
    '"readonly":{{json .HostConfig.ReadonlyRootfs}},"init":{{json .HostConfig.Init}},'
    '"pid_mode":{{json .HostConfig.PidMode}},"network_mode":{{json .HostConfig.NetworkMode}},'
    '"stop_timeout":{{json .Config.StopTimeout}}},'
    '"state":{"running":{{json .State.Running}},"pid":{{json .State.Pid}},'
    '"exit_code":{{json .State.ExitCode}},"oom":{{json .State.OOMKilled}},'
    '"restarting":{{json .State.Restarting}},"finished_at":{{json .State.FinishedAt}}}}')
WRITER_FORMAT = ('{"id":{{json .Id}},"running":{{json .State.Running}},'
    '"pid":{{json .State.Pid}},"mounts":{{json .Mounts}}}')


def require(value, code='scorer_selection_refused'):
    if not value:
        raise inventory.Refused(code)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def path(value, root):
    require(isinstance(value,str) and len(value)<=2048 and '\\' not in value and '\0' not in value)
    p = Path(value)
    require(p.is_absolute() and str(p)==value and '..' not in p.parts and p.is_relative_to(root) and p!=root)
    return p


def normalize(container):
    require(isinstance(container,dict) and set(container)==CONTAINER_KEYS)
    value=dict(container)
    require(isinstance(value['mounts'],list) and len(value['mounts'])<=8)
    value['mounts']=sorted(value['mounts'],key=lambda m:(m['Destination'],m['Source']))
    require(len({m['Destination'] for m in value['mounts']})==len(value['mounts']))
    return value


def validate(value):
    """Validate public claims only; no receipt grants current execution authority."""
    require(isinstance(value,dict) and set(value)=={'format','company_id','deployment_id','binding_sha256',
        'config','service_token_ref','oauth','container','service_port','stop_seconds'})
    require(value['format']==FORMAT and value['company_id']==inventory.COMPANY)
    require(str(UUID(value['deployment_id']))==value['deployment_id'] and UUID(value['deployment_id']).int)
    require(re.fullmatch('[0-9a-f]{64}',value['binding_sha256']) is not None)
    require(set(value['config'])=={'path','sha256'} and set(value['service_token_ref'])=={'path','reference'})
    path(value['config']['path'],inventory.STATE); path(value['service_token_ref']['path'],inventory.STATE)
    require(re.fullmatch('[0-9a-f]{64}',value['config']['sha256']) is not None)
    require(re.fullmatch(r'(credential|secret)://[A-Za-z0-9._:/-]{1,240}',value['service_token_ref']['reference']) is not None)
    oauth=value['oauth']; require(set(oauth)=={'directory','identity','mount_source','mount_destination'})
    require(oauth['directory']==str(inventory.RUNTIME/'oauth/ada-private'))
    identity=oauth['identity']
    require(set(identity)=={'format','company_id','employee_id','profile_ref','credential_ref'}
        and identity['format']=='ortak-oauth-identity/1' and identity['company_id']==inventory.COMPANY
        and identity['employee_id']=='ada-private')
    require(value['service_port']==8651 and type(value['service_port']) is int
        and value['stop_seconds']==45 and type(value['stop_seconds']) is int)
    c=normalize(value['container'])
    require(re.fullmatch('[0-9a-f]{64}',c['id']) is not None
        and re.fullmatch('sha256:[0-9a-f]{64}',c['image']) is not None
        and isinstance(c['name'],str) and c['name'].startswith('/')
        and isinstance(c['started_at'],str) and c['started_at'] not in ('','0001-01-01T00:00:00Z'))
    require(c['user'] in ('10001','10001:10001') and c['readonly'] is True
        and c['init'] is True and c['pid_mode']=='' and isinstance(c['network_mode'],str)
        and c['network_mode']!='host' and not c['network_mode'].startswith('container:')
        and c['restart']=={'Name':'no','MaximumRetryCount':0} and c['stop_timeout']==45)
    require(c['port_bindings']=={'8651/tcp':[{'HostIp':'127.0.0.1','HostPort':'8651'}]})
    require(isinstance(c['entrypoint'],list) and isinstance(c['cmd'],list)
        and all(isinstance(arg,str) and len(arg)<=2048 and '\0' not in arg for arg in c['entrypoint']+c['cmd']))
    argv=c['entrypoint']+c['cmd']
    require('ortak_hermes_bridge.semantic' in argv and '--enable-selected-semantic-oauth' in argv)
    mounts=c['mounts']
    require(all(m['Type']=='bind' and type(m['RW']) is bool for m in mounts))
    shared=[m for m in mounts if m['Source']==oauth['mount_source'] and m['Destination']==oauth['mount_destination']]
    require(len(shared)==1 and shared[0]['RW'] is True
        and Path(oauth['directory']).is_relative_to(Path(oauth['mount_destination'])))
    require(not any('docker.sock' in m['Destination'] or 'docker.proxy.sock' in m['Source'] for m in mounts))
    for field in ('config','service_token_ref'):
        mapped=[m for m in mounts if m['Source']==value[field]['path'] and m['RW'] is False]
        require(len(mapped)==1)
        option='--config' if field=='config' else '--token-file'
        require(argv.count(option)==1 and argv.index(option)+1<len(argv)
            and argv[argv.index(option)+1]==mapped[0]['Destination'])
    require(argv.count('--port')==1 and argv.index('--port')+1<len(argv) and argv[argv.index('--port')+1]=='8651')
    require(all(not m['RW'] or m==shared[0] for m in mounts))
    return {**value,'container':c}


def selection():
    """Omission remains disabled; never discover a receipt or infer a new writer."""
    selected=getattr(inventory,'SCORER_SELECTION',None)
    if selected is None:return None
    require(isinstance(selected,dict) and set(selected)=={'receipt_path','receipt_sha256'})
    location=path(selected['receipt_path'],inventory.STATE)
    value,metadata=inventory.public_json(location.parent,location.name)
    require(metadata['sha256']==selected['receipt_sha256'])
    return {'receipt':metadata,'selection':validate(value)}


def special_metadata(location, selected):
    """Only the exact original selected public config may retain its immutable0444 mode."""
    s=selected['selection']
    location=Path(location); require(str(location)==s['config']['path'])
    inventory.directory(inventory.STATE)
    for parent in list(reversed(location.parents)):
        if parent==inventory.STATE or not parent.is_relative_to(inventory.STATE):continue
        row=parent.lstat()
        require(stat.S_ISDIR(row.st_mode) and row.st_uid==os.getuid() and stat.S_IMODE(row.st_mode)==0o700
            and parent.resolve()==parent,'scorer_file_metadata_refused')
    row=location.lstat()
    require(stat.S_ISREG(row.st_mode) and row.st_nlink==1 and row.st_uid==os.getuid()
        and stat.S_IMODE(row.st_mode) in {0o400,0o444,0o600}
        and 0<row.st_size<=256*1024,'scorer_file_metadata_refused')
    return {'path':str(location),'uid':row.st_uid,'mode':stat.S_IMODE(row.st_mode),'bytes':row.st_size,
        'device':row.st_dev,'inode':row.st_ino,'mtime_ns':row.st_mtime_ns}


def public_file(root, relative, selected):
    location=root/relative
    if selected is None or str(location)!=selected['selection']['config']['path']:
        return inventory.public_json(root,relative)
    before=special_metadata(location,selected)
    with os.fdopen(os.open(location,os.O_RDONLY|os.O_NOFOLLOW),'rb') as stream:
        opened=os.fstat(stream.fileno()); raw=stream.read(256*1024+1)
    require(opened.st_ino==before['inode'] and opened.st_dev==before['device'] and len(raw)==before['bytes']
        and special_metadata(location,selected)==before,'scorer_configuration_changed')
    value=json.loads(raw);inventory.reject_secret_fields(value)
    digest=hashlib.sha256(raw).hexdigest()
    require(digest==selected['selection']['config']['sha256'],'scorer_configuration_changed')
    return value,{**before,'sha256':digest}


def bind_files(selected, values, secrets):
    """Original config, worker reference and both token copies must remain explicitly enrolled."""
    worker=values.get(str(inventory.WORKER_CONFIG),{})
    if selected is None:
        require(worker.get('semantic') is None,'scorer_selection_required')
        return
    s=selected['selection']; config=values.get(s['config']['path'])
    require(isinstance(config,dict) and set(config)=={'company_id','profiles','semantic'}
        and config['company_id']==inventory.COMPANY and config['semantic']['deployment_id']==s['deployment_id']
        and config['semantic']['binding_sha256']==s['binding_sha256'])
    profiles=[p for p in config['profiles'] if hashlib.sha256(canonical(p['binding'])).hexdigest()==s['binding_sha256']]
    require(len(profiles)==1 and profiles[0]['oauth_directory']==s['oauth']['directory'])
    profile=profiles[0]; identity=s['oauth']['identity']
    require('oauth_owner' not in profile and profile['employee_id']==identity['employee_id']
        and profile['binding']['profile_ref']==identity['profile_ref']
        and profile['binding']['credential_refs']==[identity['credential_ref']])
    semantic=worker.get('semantic'); require(isinstance(semantic,dict) and semantic.get('adapter')=='hermes-codex')
    deployment=semantic['deployment']
    require(deployment['deployment_id']==s['deployment_id'] and deployment['binding_sha256']==s['binding_sha256']
        and deployment['bridge_token_ref']==s['service_token_ref']['reference']
        and deployment['origin']=='http://127.0.0.1:8651')
    require(any(row['path']==s['service_token_ref']['path'] for row in secrets),'scorer_token_not_enrolled')
    # The existing selected D3 launcher reads a separate host-owned copy; both
    # original files are required. Never replace one with the other's bytes.
    worker_token=Path(s['service_token_ref']['path']).parent.parent/'worker-service-token'
    require(any(row['path']==str(worker_token) for row in secrets),'scorer_worker_token_not_enrolled')
    require(str(Path(selected['receipt']['path'])) in values,'scorer_receipt_not_enrolled')


def owner(inspector, selected, *, resumed=False):
    require(selected is not None,'scorer_selection_required')
    s=selected['selection']; actual=json.loads(inspector.run(inspector.docker('inspect','--format',OWNER_FORMAT,s['container']['id']),limit=65536))
    require(set(actual)=={'container','state'},'scorer_owner_changed')
    actual['container']=normalize(actual['container']); expected=s['container']
    skip={'started_at'} if resumed else set()
    require(all(actual['container'][k]==expected[k] for k in CONTAINER_KEYS-skip),'scorer_owner_changed')
    state=actual['state']
    require(set(state)=={'running','pid','exit_code','oom','restarting','finished_at'}
        and all(type(state[k]) is bool for k in ('running','oom','restarting'))
        and type(state['pid']) is int and type(state['exit_code']) is int,'scorer_state_refused')
    return actual


def prepare(inspector, selected):
    if selected is None:return None
    actual=owner(inspector,selected)
    require(actual['state']['running'] and actual['state']['pid']>0 and not actual['state']['restarting']
        and not actual['state']['oom'],'scorer_running_owner_required')
    return {**selected,'owner':actual,'files':file_refs(selected)}


def file_refs(selected):
    """Read public config and stat both original host-token copies; never open either token."""
    s=selected['selection']; config=Path(s['config']['path'])
    _,metadata=public_file(config.parent,config.name,selected)
    token=Path(s['service_token_ref']['path']);worker=token.parent.parent/'worker-service-token'
    result={'config':metadata,'service_token_ref':inventory.file_metadata(token.parent,token.name,service_readable=True),
        'worker_token':inventory.file_metadata(worker.parent,worker.name,service_readable=True)}
    require(all(result[key]['mode']==0o600 and 0<result[key]['bytes']<=4096
        for key in ('service_token_ref','worker_token')),'scorer_token_metadata_refused')
    return result


def writers(inspector, selected):
    """Inspect only IDs and mounts; unknown/replacement credential writers refuse, never stop by discovery."""
    raw=inspector.run(inspector.docker('ps','--quiet','--no-trunc','--filter','status=running'),limit=8192)
    ids=raw.decode().split();require(len(ids)<=128 and len(set(ids))==len(ids)
        and all(re.fullmatch('[0-9a-f]{64}',x) for x in ids),'scorer_writer_scan_refused')
    if not ids:return []
    rows=inspector.run(inspector.docker('inspect','--format',WRITER_FORMAT,*ids),limit=512*1024).decode().splitlines()
    require(len(rows)==len(ids),'scorer_writer_scan_refused')
    def host_path(value):
        # Docker Desktop exposes this one observed host prefix for some older
        # containers. This alias is used only to detect overlapping writers;
        # exact owner comparisons retain every original daemon mount field.
        return Path(value[len('/host_mnt'):] if value.startswith('/host_mnt/') else value)
    shared=host_path(selected['selection']['oauth']['mount_source']); found=[];seen=set()
    for raw in rows:
        row=json.loads(raw);require(set(row)=={'id','running','pid','mounts'} and row['id'] in ids
            and row['id'] not in seen and len(row['mounts'])<=32,'scorer_writer_scan_refused');seen.add(row['id'])
        for mount in row['mounts']:
            if mount['Type']!='bind' or not mount['RW']:continue
            source=host_path(mount['Source'])
            if shared.is_relative_to(source) or source.is_relative_to(shared):
                if row['running'] or row['pid']!=0:found.append(row['id'])
                break
    return sorted(found)


def stopped(inspector, expected):
    """Process termination, not active_scores or a periodic maintenance status, is the drain witness."""
    selected=selection()
    require((selected is None)==(expected is None),'scorer_selection_changed')
    if expected is None:return None
    require(selected=={k:expected[k] for k in ('receipt','selection')},'scorer_selection_changed')
    require(file_refs(selected)==expected['files'],'scorer_configuration_changed')
    actual=owner(inspector,selected);state=actual['state']
    require(not state['running'] and state['pid']==0 and not state['oom'] and not state['restarting']
        and state['exit_code']==0 and state['finished_at'] not in ('','0001-01-01T00:00:00Z'),'scorer_not_cleanly_stopped')
    require(not writers(inspector,selected),'scorer_oauth_writer_remains')
    return {'selection_sha256':selected['receipt']['sha256'],'container':actual['container'],'state':state,
        'credential_writers_absent':True,'active_scores_is_not_credential_drain':True}


def resume_argv(inspector, expected, proof):
    """Return only the original retained container start after an exact current stop proof; never execute it."""
    require(expected is not None and proof is not None,'scorer_selection_required')
    require(stopped(inspector,expected)==proof,'scorer_stop_proof_changed')
    return inspector.docker('start',expected['selection']['container']['id'])


def verify_resumed(inspector, expected, proof):
    """A new generation must retain all original refs/mounts; missing selection cannot approve replay."""
    selected=selection();require(expected is not None and proof is not None and selected is not None,'scorer_selection_required')
    require(selected=={k:expected[k] for k in ('receipt','selection')},'scorer_selection_changed')
    stop_proof(expected,proof)
    require(file_refs(selected)==expected['files'],'scorer_configuration_changed')
    actual=owner(inspector,selected,resumed=True);state=actual['state']
    require(state['running'] and state['pid']>0 and not state['restarting'] and not state['oom']
        and actual['container']['started_at']!=proof['container']['started_at'],'scorer_resume_generation_refused')
    return actual


def stop_proof(expected,proof):
    """Validate retained termination provenance; this does not establish a live lease."""
    require(isinstance(proof,dict) and set(proof)=={'selection_sha256','container','state',
        'credential_writers_absent','active_scores_is_not_credential_drain'}
        and proof['selection_sha256']==expected['receipt']['sha256']
        and proof['container']==expected['selection']['container']
        and proof['credential_writers_absent'] is True
        and proof['active_scores_is_not_credential_drain'] is True,'scorer_archive_stop_refused')
    state=proof['state']
    require(set(state)=={'running','pid','exit_code','oom','restarting','finished_at'}
        and state['running'] is False and type(state['pid']) is int and state['pid']==0
        and type(state['exit_code']) is int and state['exit_code']==0 and state['oom'] is False
        and state['restarting'] is False and isinstance(state['finished_at'],str)
        and state['finished_at'] not in ('','0001-01-01T00:00:00Z'),'scorer_archive_stop_refused')


def require_held(witness, expected):
    """Only the live Linux/OAuth lease plus fresh stopped-owner scan permits secret capture."""
    if expected is None:
        require(selection() is None,'scorer_selection_changed')
        return None
    require(getattr(witness,'active',False) and witness.process.poll() is None,'scorer_held_barrier_required')
    proof=stopped(witness.gate.inspector,expected)
    require(witness.get('scorer')==proof,'scorer_stop_proof_changed')
    return proof


def verify_offline(expected, proof, public, secrets):
    """Validate captured refs and stop provenance without reading live paths or enabling restart."""
    require((expected is None)==(proof is None),'scorer_archive_selection_changed')
    if expected is None:return None
    s=validate(expected['selection'])
    stop_proof(expected,proof)
    for kind,metadata in (('receipt',expected['receipt']),('config',expected['files']['config'])):
        target=public/'selected'/metadata['path'].lstrip('/')
        row=target.lstat()
        require(stat.S_ISREG(row.st_mode) and row.st_uid==os.getuid() and row.st_nlink==1
            and row.st_size==metadata['bytes'] and row.st_size<=256*1024,'scorer_archive_file_refused')
        raw=target.read_bytes()
        require(hashlib.sha256(raw).hexdigest()==metadata['sha256'],'scorer_archive_file_refused')
        if kind=='receipt':require(validate(json.loads(raw))==s,'scorer_archive_selection_changed')
    require(expected['files']['config']['path']==s['config']['path']
        and expected['files']['config']['sha256']==s['config']['sha256'],'scorer_archive_selection_changed')
    by_path={row['path']:row for row in secrets}
    for key in ('service_token_ref','worker_token'):
        meta=expected['files'][key]
        require(by_path.get(meta['path'])==meta and meta['uid']==expected['files']['config']['uid']
            and meta['mode']==0o600 and 0<meta['bytes']<=4096,'scorer_archive_token_refused')
    require(expected['files']['service_token_ref']['path']==s['service_token_ref']['path']
        and expected['files']['worker_token']['path']==str(Path(s['service_token_ref']['path']).parent.parent/'worker-service-token'),
        'scorer_archive_token_refused')
    return {'selection_sha256':expected['receipt']['sha256'],'stopped_credential_owner_verified':True,
        'both_original_service_tokens_in_encrypted_allowlist':True,'runtime_activation':False,
        'source_container_start_authorized':False}
