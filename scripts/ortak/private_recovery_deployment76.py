"""Explicit root-frozen D3 deployment selection; no process/secret discovery.

The old74 volume creation and76 migration remain historical foundations. Current
owners/configs are independently pinned; no unchanged-owner history is invented.
"""
import hashlib
import json
import os
from pathlib import Path
import stat

import private_recovery_inventory as inventory
import private_recovery_obligations as obligations
import private_recovery_journal as journal

EMPLOYEES = ('ada-private', 'bora-private', 'deniz-private')
PROFILE_FILES = ('ORTAK_DISPOSABLE_PROFILE.json', 'ORTAK_RUNTIME_BINDING.json', 'ORTAK_PROVIDER.json')
ADA_PROFILE = 'ortak-private-20260905-ada-oauth-v0'
OAUTH_REF = 'secret://ortak-private-20260905/ada-codex-oauth-v0'
MEMORY_DEPLOYMENT = 'efd1ad6f-df29-4346-8a2d-f2c271ff4b72'
require = inventory.require


def _version():
    """Only the three explicitly reviewed deployment versions enter this adapter."""
    version=inventory.MAIN_SCHEMA_VERSION
    require(type(version) is int and version in (76,77,78),'d3_selection_schema_refused')
    return version


def selected_hashes():
    """Only these source-bound public files establish the current deployment."""
    selected = inventory.DEPLOYMENT76_SELECTION
    require(isinstance(selected, dict) and set(selected)=={'receipt_path','receipt_sha256'}
        and selected['receipt_path']==str(inventory.D3_RECOVERY/'deployment.json')
        and isinstance(selected['receipt_sha256'],str) and len(selected['receipt_sha256'])==64,
        'current_deployment_pin_required')
    result={selected['receipt_path']:selected['receipt_sha256'],
        str(inventory.CURRENT_OWNERS):inventory.CURRENT_OWNERS_SHA,
        str(inventory.API_CONFIG):inventory.API_CONFIG_SHA,
        str(inventory.WORKER_CONFIG):inventory.WORKER_CONFIG_SHA,
        str(inventory.D3_CONTROLLER_CONFIG):inventory.D3_CONTROLLER_CONFIG_SHA,
        str(inventory.D3_CONTROLLER_OBSERVATION):inventory.D3_CONTROLLER_OBSERVATION_SHA}
    if _version() in (77,78):
        evidence=inventory.DEPLOYMENT77_EVIDENCE
        require(isinstance(evidence,dict) and set(evidence)=={
            'service_readiness','honcho_preservation','target_registration'},'deployment77_evidence_required')
        for ref in evidence.values():
            require(isinstance(ref,dict) and set(ref)=={'path','sha256'}
                and Path(ref['path']).is_relative_to(inventory.CURRENT_ROLLOUT)
                and isinstance(ref['sha256'],str) and len(ref['sha256'])==64,
                'deployment77_evidence_refused')
            result[ref['path']]=ref['sha256']
    return result


def _receipt(values):
    hashes=selected_hashes()
    expected={'format':'ortak-private-current-deployment/1','status':'current_selection_verified',
        'schema':_version(),'company_id':inventory.COMPANY}
    for name,path in (('current_owners',inventory.CURRENT_OWNERS),('api_config',inventory.API_CONFIG),
        ('worker_config',inventory.WORKER_CONFIG),('controller_config',inventory.D3_CONTROLLER_CONFIG),
        ('controller_observation',inventory.D3_CONTROLLER_OBSERVATION)):
        expected[name]={'path':str(path),'sha256':hashes[str(path)]}
    if _version() in (77,78):expected['evidence']=inventory.DEPLOYMENT77_EVIDENCE
    require(values.get(inventory.DEPLOYMENT76_SELECTION['receipt_path'])==expected,
        'current_deployment_receipt_refused')
    return expected


def _public_paths():
    return {inventory.D3_CONTROLLER_CONFIG} | {inventory.D3_ROOT/employee/'profile'/name
        for employee in EMPLOYEES[1:] for name in PROFILE_FILES}


def is_public_path(path):
    """Exactly seven selected read-only leaves; never arbitrary public modes."""
    return inventory.DEPLOYMENT76_SELECTION is not None and Path(path) in _public_paths()


def _public_metadata(path):
    require(is_public_path(path), 'd3_public_path_refused')
    root=inventory.CURRENT_ROLLOUT if _version() in (77,78) and path==inventory.D3_CONTROLLER_CONFIG else inventory.D3_ROOT
    inventory.directory(root)
    relative=path.relative_to(root)
    public_parents={item.parent for item in _public_paths()}
    for depth in range(1,len(relative.parts)):
        parent=root.joinpath(*relative.parts[:depth]); row=parent.lstat()
        require(parent.resolve()==parent and stat.S_ISDIR(row.st_mode) and row.st_uid==os.getuid()
            and stat.S_IMODE(row.st_mode)==(0o555 if parent in public_parents else 0o700),
            'd3_public_parent_refused')
    row=path.lstat()
    require(path.resolve()==path and stat.S_ISREG(row.st_mode) and row.st_uid==os.getuid()
        and row.st_nlink==1 and stat.S_IMODE(row.st_mode)==0o444 and 0<row.st_size<=65536,
        'd3_public_metadata_refused')
    return {'path':str(path),'uid':row.st_uid,'mode':stat.S_IMODE(row.st_mode),'bytes':row.st_size,
        'device':row.st_dev,'inode':row.st_ino,'mtime_ns':row.st_mtime_ns}


def public_file(root, relative):
    """Bounded no-follow read of the selected public-only files; no secrets."""
    path=root/relative; before=_public_metadata(path)
    with os.fdopen(os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK),'rb') as stream:
        row=os.fstat(stream.fileno()); raw=stream.read(65537)
    require(row.st_dev==before['device'] and row.st_ino==before['inode']
        and stat.S_IMODE(row.st_mode)==before['mode'] and row.st_uid==before['uid'] and row.st_nlink==1
        and row.st_mtime_ns==before['mtime_ns'] and len(raw)==row.st_size==before['bytes']
        and _public_metadata(path)==before,'d3_public_changed_during_read')
    value=json.loads(raw); inventory.reject_secret_fields(value)
    return value,{**before,'sha256':hashlib.sha256(raw).hexdigest()}


def runtime_bindings(values):
    """Five exact variants share only the original Ada OAuth owner in place."""
    _receipt(values)
    config=values[str(inventory.D3_CONTROLLER_CONFIG)]
    original=values[str(inventory.EVIDENCE/'private-hermes-controller-selection.json')]
    owner={'format':'ortak-oauth-identity/1','company_id':inventory.COMPANY,'employee_id':'ada-private',
        'profile_ref':ADA_PROFILE,'credential_ref':OAUTH_REF}
    oauth=str(inventory.RUNTIME/'oauth/ada-private')
    require(original['company_id']==inventory.COMPANY and original['root']==str(inventory.RUNTIME)
        and original['oauth_directory']==oauth and original['config']==str(inventory.RUNTIME/'controller/config.json')
        and original['journal']==str(inventory.RUNTIME/'state/journal.sqlite'),'original_oauth_selection_refused')
    expected=[]
    for path,model,effort in inventory.RUNTIME_VARIANTS:
        binding={'adapter':'hermes','profile_ref':ADA_PROFILE,'model':model,'workspace_ref':inventory.WORKSPACE_REF,
            'credential_refs':[OAUTH_REF],'options':{'reasoning_effort':effort}}
        expected.append({'employee_id':'ada-private','directory':str(path),'oauth_directory':oauth,'binding':binding})
    require({**expected[0]['binding'],'workspace_ref':'none'}==original['binding'],'original_runtime_binding_changed')
    for employee in EMPLOYEES[1:]:
        name=employee.removesuffix('-private')
        binding={'adapter':'hermes','profile_ref':f'ortak-private-20260905-{name}-oauth-v0','model':'gpt-5.6-sol',
            'workspace_ref':'none','credential_refs':[OAUTH_REF],'options':{'reasoning_effort':'high'}}
        row={'employee_id':employee,'directory':str(inventory.D3_ROOT/employee/'profile'),
            'oauth_directory':oauth,'oauth_owner':owner,'binding':binding}
        require(values[str(inventory.D3_ROOT/employee/'controller-profile.json')]==row,'d3_profile_selection_changed')
        expected.append(row)
    require(set(config)=={'company_id','executor','profiles'} and config['company_id']==inventory.COMPANY
        and config['profiles']==expected,'d3_runtime_variant_inventory_mismatch')
    executor=config['executor']
    executor_keys={'docker_binary','image','journal_volume','network','validated_digest','workspace_validated_digest'}
    if _version() in (77,78):executor_keys.add('confidential_validated_digest')
    require(set(executor)==executor_keys
        and executor['docker_binary']=='/usr/bin/docker' and executor['network']=='ortak-v0-hermes-run-5214763bf281407fb8412121b4d26315'
        and executor['image']==executor['validated_digest']==executor['workspace_validated_digest']==inventory.WORKER_IMAGE
        and executor['journal_volume']==journal.selection(inventory.JOURNAL_VOLUME),'d3_runtime_executor_changed')
    if _version() in (77,78):
        require(executor['confidential_validated_digest']==inventory.WORKER_IMAGE,
            'confidential_runtime_artifact_changed')
        journal.require_confidential_schema(inventory.JOURNAL_CONFIDENTIAL,_version())
    volume=values[str(inventory.CONTROLLER_CONFIG/'receipt.json')]
    require(volume['status']=='journal_volume_prepared_not_activated' and volume['original_untouched'] is True
        and volume['temporary_container_removed'] is True and volume['selection']==executor['journal_volume'],
        'original_journal_volume_receipt_refused')
    for row in expected:
        path=Path(row['directory']); binding=row['binding']
        require(values[str(path/'ORTAK_RUNTIME_BINDING.json')]==binding
            and values[str(path/'ORTAK_DISPOSABLE_PROFILE.json')]=={'company_id':inventory.COMPANY,
                'employee_id':row['employee_id'],'profile_ref':binding['profile_ref']}
            and values[str(path/'ORTAK_PROVIDER.json')]=={'provider':'openai-codex','credential_ref':OAUTH_REF},
            'd3_runtime_profile_changed')
    return expected


def _memory(values, worker):
    memory=worker.get('memory',{}); employees=memory.get('employees')
    require(memory.get('deployment_id')==MEMORY_DEPLOYMENT and memory.get('origin')=='http://127.0.0.1:8009'
        and memory.get('endpoint_ref')=='service://ortak-private-20260905/honcho'
        and memory.get('token_ref')=='secret://ortak-private-20260905/honcho-admin'
        and memory.get('require_creation_receipts') is True and memory.get('validate_memory_io') is True
        and isinstance(employees,list) and [e.get('employee_id') for e in employees]==list(EMPLOYEES),
        'd3_memory_selection_refused')
    namespaces=set(); native_workspaces=set()
    if _version() in (77,78):
        require(isinstance(inventory.EMPLOYEE77_DESTINATIONS,dict)
            and set(inventory.EMPLOYEE77_DESTINATIONS)==set(EMPLOYEES),'employee77_selection_required')
    for row in employees:
        employee=row['employee_id']; root=inventory.STATE if employee=='ada-private' else inventory.D3_ROOT/employee
        prepared=values[str(root/'memory/prepared-memory.json')]; receipt=prepared['creation_receipt']
        binding=row['binding']; namespace=binding['workspace']; resources=receipt['resources']
        require(row['creation_receipt']==receipt and receipt['company_id']==inventory.COMPANY
            and receipt['employee_id']==employee and receipt['deployment_id']==MEMORY_DEPLOYMENT
            and receipt['binding']==binding and row['creation_key']==receipt['creation_key']
            and binding['adapter']=='honcho' and binding['employee_peer']==employee
            and binding['user_peer']=='operator-private' and binding['options']=={}
            and set(resources)=={'workspace','employee_peer','user_peer'}
            and all(r.get('ownership')=='created' for r in resources.values())
            and resources['workspace']['resource_ref']=='workspace:'+namespace
            and resources['employee_peer']['resource_ref']=='peer:'+namespace+'/'+employee
            and resources['user_peer']['resource_ref']=='peer:'+namespace+'/operator-private',
            'd3_memory_ownership_changed')
        require(namespace not in namespaces and receipt['native_ids']['workspace'] not in native_workspaces,
            'd3_memory_namespace_shared')
        namespaces.add(namespace);native_workspaces.add(receipt['native_ids']['workspace'])
        if _version() in (77,78):
            require(row.get('reviewed_employee_destinations',[])==inventory.EMPLOYEE77_DESTINATIONS[employee],
                'employee77_destination_selection_changed')
        if employee=='ada-private':
            require(row.get('reviewed_projects')==row.get('reviewed_runtime_projects')==[inventory.REVIEWED_PROJECT]
                and row.get('reviewed_conversations')==inventory.REVIEWED_CONVERSATIONS,'d3_reviewed_selection_changed')
        else:
            require(all(row.get(key,[])==[] for key in ('reviewed_projects','reviewed_runtime_projects','reviewed_conversations')),
                'd3_implicit_reviewed_memory_refused')
    return list(EMPLOYEES)


def _workspace(values, worker):
    selected=worker.get('workspace',{})
    require(set(selected)=={'expires_at','grants','input_root','reader_binary','reader_sha256','register_selected_inputs','run_root'}
        and selected['register_selected_inputs'] is False
        and {**{key:selected[key] for key in ('input_root','run_root','reader_binary','reader_sha256')},
            'company_id':inventory.COMPANY,'reader_uid':os.getuid()}==inventory.WORKSPACE_SELECTION,
        'current_workspace_selection_refused')
    registration=values[str(inventory.WORKSPACE_REGISTRATION)]; retained=registration['registry']['bindings']
    require(registration['status']=='verified' and registration['worker_mode']=='retained'
        and registration['expiry_unchanged'] is True and registration['reader_sha256']==selected['reader_sha256']
        and len(retained)==1 and selected['grants']==[retained[0]['grant']]
        and retained[0]['expires_at']==selected['expires_at']
        and retained[0]['grant']==values[str(inventory.BACKEND_ROLLOUT/'config/grant.json')]
        and retained[0]['grant']['workspace_ref']==inventory.WORKSPACE_REF,'current_workspace_registration_refused')


def deployment_bindings(values):
    """Current receipts + unchanged database/artifact/namespace/role boundaries."""
    receipt=_receipt(values)
    version=_version()
    root=inventory.CURRENT_ROLLOUT
    migrated=values[str(root/f'main-migration{version}/receipt.json')]
    after=values[str(root/f'main-migration{version}/database-after.json')]['metadata']
    require(migrated.get('status')=='migrated_verified' and migrated.get('code')=='ok'
        and migrated.get('to_schema')==version and obligations.schema_version(after)==version,'current_migration_receipt_refused')
    if version==76:
        honcho=values[str(inventory.HONCHO_ROLLOUT/'honcho-verified.json')]
        require(honcho.get('status')=='upgraded_verified' and honcho.get('new_api')==inventory.SERVICES['honcho_api'][0]
            and honcho.get('new_image')==inventory.SERVICES['honcho_api'][2] and honcho.get('metadata_unchanged') is True
            and honcho.get('settings_sequences_unchanged') is True,'current_honcho_receipt_refused')
    else:
        foundation=migrated if version==77 else values[str(root/'main-migration77/receipt.json')]
        require(foundation.get('status')=='migrated_verified' and foundation.get('code')=='ok'
            and foundation.get('to_schema')==77,'deployment77_foundation_refused')
        _deployment77_evidence(values,foundation)
    owners=values[str(inventory.CURRENT_OWNERS)]
    require(set(owners)==set(inventory.NATIVE_WRITERS)|{'native'} and all(
        owners[name]['launcher']==str(inventory.NATIVE_LAUNCHERS[name])
        and owners[name]['executable']==str(inventory.NATIVE_BINARIES[name])
        and owners[name]['uid']==os.getuid() and owners[name]['cwd']==str(inventory.STATE)
        for name in inventory.NATIVE_WRITERS),'d3_current_owner_selection_refused')
    observation=values[str(inventory.D3_CONTROLLER_OBSERVATION)]
    require(observation['id']==inventory.SERVICES['controller'][0]
        and observation['name']=='/'+inventory.SERVICES['controller'][1] and observation['image']==inventory.SERVICES['controller'][2]
        and observation['running'] is True and observation['user']=='10001:10001'
        and observation['readonly'] is True and observation['init'] is True and observation['pid_mode']==''
        and observation['restart']=={'MaximumRetryCount':0,'Name':'no'} and observation['stop_timeout']==1
        and observation['port_bindings']=={'8650/tcp':[{'HostIp':'127.0.0.1','HostPort':'8650'}]}
        and observation['entrypoint']==['python','-m','ortak_hermes_bridge']
        and observation['cmd']==['--config',str(inventory.D3_CONTROLLER_CONFIG),'--token-file',str(inventory.RUNTIME/'controller/service-token'),
            '--journal',str(inventory.RUNTIME/'state/journal.sqlite'),'--listen-address','0.0.0.0','--enable-validated-docker-executor']
        and sorted((m['Source'],m['Destination'],m['RW']) for m in observation['mounts'] if m['Type']=='bind')==inventory.expected_binds('controller'),
        'd3_controller_observation_refused')
    worker=values[str(inventory.WORKER_CONFIG)]
    employees=_memory(values,worker);_workspace(values,worker)
    if version in (77,78):
        require(isinstance(inventory.ENCRYPTED77_WORKER_SELECTION,dict)
            and worker.get('encrypted_dm')==inventory.ENCRYPTED77_WORKER_SELECTION,
            'encrypted77_selection_changed')
    api=values[str(inventory.API_CONFIG)]
    require(api['community_id']=='55bebe0f-90f0-44a2-a021-3b69fbb520a6' and api['origin']=='http://127.0.0.1:8787'
        and len(api['humans'])==1 and api['humans'][0]['role']=='operator'
        and api['humans'][0]['employee_ids']==employees,'d3_api_scope_refused')
    return {'schema':version,'honcho_id':inventory.SERVICES['honcho_api'][0],'honcho_image':inventory.SERVICES['honcho_api'][2],
        'api_config':str(inventory.API_CONFIG),'worker_config':str(inventory.WORKER_CONFIG),
        'reviewed_runtime_projects':[inventory.REVIEWED_PROJECT],'reviewed_conversations':inventory.REVIEWED_CONVERSATIONS,
        'workspace_selection':inventory.WORKSPACE_SELECTION,'workspace_ref':inventory.WORKSPACE_REF,
        'retained_workspace_registration':str(inventory.WORKSPACE_REGISTRATION),
        'historical_target_epoch_is_not_current_authority':True,'scorer_or_additional_employee_scope_added':True,
        'employees':employees,'current_selection':receipt,'oauth_owner_reused_in_place':'ada-private'} | (
            {'employee_reviewed_destinations':inventory.EMPLOYEE77_DESTINATIONS,
             'encrypted_worker_selection':inventory.ENCRYPTED77_WORKER_SELECTION,
             'confidential_journal':journal.require_confidential_schema(inventory.JOURNAL_CONFIDENTIAL,version),
             'native_confidential_app_data':str(inventory.NATIVE_CONFIDENTIAL_APP_DATA)} if version in (77,78) else {})


def _deployment77_evidence(values,migrated):
    """Retain startup/registration provenance; never renew targets or assert current ACLs."""
    refs=inventory.DEPLOYMENT77_EVIDENCE
    ready=values[refs['service_readiness']['path']]
    require(ready.get('format')=='ortak-services77-readiness/1'
        and ready.get('status')=='service_interfaces_verified','services77_readiness_refused')
    for role,key in (('controller','controller'),('honcho','honcho_api')):
        require(ready[role]['id']==inventory.SERVICES[key][0]
            and ready[role]['image']==inventory.SERVICES[key][2],'services77_generation_changed')
    require(ready['controller']['provider_probe'] is False and ready['honcho']['health']==200
        and ready['honcho']['authenticated_protocol']==200 and ready['honcho']['employee_routes_present'] is True,
        'services77_interfaces_refused')
    # New-image profile inspection can honestly be healthy=false. It is neither
    # startup authority nor a substitute for separate real runtime acceptance.
    honcho=values[refs['honcho_preservation']['path']]
    require(honcho.get('status')=='upgraded_verified'
        and honcho.get('new_api')==inventory.SERVICES['honcho_api'][0]
        and honcho.get('new_image')==inventory.SERVICES['honcho_api'][2]
        and honcho.get('old_tables_unchanged') is True and honcho.get('settings_sequences_unchanged') is True
        and set(honcho.get('new_employee_tables',[]))==set(obligations.extensions77.HONCHO_KEYS)
        and honcho.get('capture_manifest_sha256')==migrated.get('capture_manifest_sha256')
        and honcho.get('readiness_sha256')==refs['service_readiness']['sha256'],
        'honcho77_preservation_refused')
    registration=values[refs['target_registration']['path']]
    require(registration.get('status')=='registrations_verified' and registration.get('code')=='ok'
        and registration.get('action')=='register' and registration.get('no_target_renewal') is True
        and registration.get('runtime_opt_in_changed') is False,'employee77_registration_refused')
    retained=[row for row in registration['public_events'] if row.get('status')=='registered_retained']
    expected=inventory.EMPLOYEE77_DESTINATIONS
    require(isinstance(expected,dict) and set(expected)==set(EMPLOYEES)
        and len(retained)==sum(len(rows) for rows in expected.values())
        and sorted((row['employee_id'],row['destination_channel_id'],row['target_id']) for row in retained)
            ==sorted((employee,row['destination_channel_id'],row['target_id'])
                for employee,rows in expected.items() for row in rows)
        and all(row['current_authority_claimed'] is False for row in retained),
        'employee77_registration_tuple_changed')
    require(inventory.NATIVE_CONFIDENTIAL_APP_DATA==Path('/Users/nambse/Library/Application Support/dev.ortak.private20260905'),
        'native77_ciphertext_path_refused')
