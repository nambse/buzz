"""Production D3 selection/read guards with synthetic metadata and local files only."""
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import private_recovery_deployment76 as subject
import private_recovery_inventory as inventory
from test_prepare_private_recovery import runtime_fixture


def fixture():
    values=runtime_fixture()
    paths={'current_owners':inventory.CURRENT_OWNERS,'api_config':inventory.API_CONFIG,
        'worker_config':inventory.WORKER_CONFIG,'controller_config':inventory.D3_CONTROLLER_CONFIG,
        'controller_observation':inventory.D3_CONTROLLER_OBSERVATION}
    pins=subject.selected_hashes()
    values[inventory.DEPLOYMENT76_SELECTION['receipt_path']]={
        'format':'ortak-private-current-deployment/1','status':'current_selection_verified',
        'schema':inventory.MAIN_SCHEMA_VERSION,'company_id':inventory.COMPANY,
        **{name:{'path':str(path),'sha256':pins[str(path)]} for name,path in paths.items()}}
    if inventory.MAIN_SCHEMA_VERSION in (77,78):
        values[inventory.DEPLOYMENT76_SELECTION['receipt_path']]['evidence']=copy.deepcopy(inventory.DEPLOYMENT77_EVIDENCE)
    config=copy.deepcopy(values[str(inventory.CONTROLLER_CONFIG/'controller/config.json')])
    config['executor'].update(docker_binary='/usr/bin/docker',network='ortak-v0-hermes-run-5214763bf281407fb8412121b4d26315')
    if inventory.MAIN_SCHEMA_VERSION in (77,78):
        config['executor']['confidential_validated_digest']=inventory.WORKER_IMAGE
    for employee in ('bora-private','deniz-private'):
        name=employee.split('-')[0]; path=inventory.D3_ROOT/employee/'profile'
        binding={'adapter':'hermes','model':'gpt-5.6-sol','workspace_ref':'none',
            'profile_ref':f'ortak-private-20260905-{name}-oauth-v0',
            'credential_refs':['secret://ortak-private-20260905/ada-codex-oauth-v0'],
            'options':{'reasoning_effort':'high'}}
        row={'employee_id':employee,'directory':str(path),'binding':binding,
            'oauth_directory':str(inventory.RUNTIME/'oauth/ada-private'),
            'oauth_owner':{'format':'ortak-oauth-identity/1','company_id':inventory.COMPANY,'employee_id':'ada-private',
                'profile_ref':'ortak-private-20260905-ada-oauth-v0','credential_ref':'secret://ortak-private-20260905/ada-codex-oauth-v0'}}
        config['profiles'].append(row)
        values[str(inventory.D3_ROOT/employee/'controller-profile.json')]=copy.deepcopy(row)
        values[str(path/'ORTAK_RUNTIME_BINDING.json')]=copy.deepcopy(binding)
        values[str(path/'ORTAK_DISPOSABLE_PROFILE.json')]={'company_id':inventory.COMPANY,'employee_id':employee,'profile_ref':binding['profile_ref']}
        values[str(path/'ORTAK_PROVIDER.json')]={'provider':'openai-codex','credential_ref':binding['credential_refs'][0]}
    values[str(inventory.D3_CONTROLLER_CONFIG)]=config
    return values


class DeploymentTests(unittest.TestCase):
    def test_explicit76_77_78_runtime_receipts_never_cross_replay_or_implicitly_enable(self):
        for version in (76,77,78):
            with self.subTest(version=version), patch.object(inventory,'MAIN_SCHEMA_VERSION',version):
                values=fixture()
                self.assertEqual(len(subject.runtime_bindings(values)),5)
                receipt=values[inventory.DEPLOYMENT76_SELECTION['receipt_path']]
                receipt['schema']=version-1
                with self.assertRaisesRegex(inventory.Refused,'current_deployment_receipt_refused'):
                    subject.runtime_bindings(values)
                if version in (77,78):
                    values=fixture()
                    with patch.object(inventory,'JOURNAL_CONFIDENTIAL',None), self.assertRaisesRegex(
                            inventory.Refused,'journal_storage_selection_refused'):
                        subject.runtime_bindings(values)
        for version in (75,79,True,78.0,'78'):
            with patch.object(inventory,'MAIN_SCHEMA_VERSION',version), self.assertRaises(inventory.Refused):
                subject.selected_hashes()

    def test_current_controller_mounts_and_configured_ports_survive_stop_but_never_repoint(self):
        identifier,name,image,_,_=inventory.SERVICES['controller']
        ports={'8650/tcp':[{'HostIp':'127.0.0.1','HostPort':'8650'}]}
        row={'id':identifier,'name':'/'+name,'image':image,'running':True,'networks':{},
            'ports':ports,'port_bindings':ports,'user':'10001:10001',
            'restart':{'MaximumRetryCount':0,'Name':'no'},'mounts':[
                {'Type':'bind','Source':source,'Destination':target,'RW':rw}
                for source,target,rw in inventory.expected_binds('controller')]}
        self.assertEqual(sum(m['Source'].startswith('/host_mnt/') for m in row['mounts']),6)
        selected=copy.deepcopy(row)
        inspector=inventory.Inventory.__new__(inventory.Inventory)
        inspector.docker=lambda *args:list(args)
        inspector.run=lambda *args,**kwargs:json.dumps(row).encode()
        with patch.object(inventory.selected_journal,'verify_volume',return_value={}), \
            patch.object(inventory,'public_json',return_value=(selected,{'sha256':inventory.D3_CONTROLLER_OBSERVATION_SHA})):
            inspector.container('controller')
            row.update(running=False,ports={})
            inspector.container('controller')
            row['port_bindings']={'8650/tcp':[{'HostIp':'0.0.0.0','HostPort':'8650'}]}
            with self.assertRaises(inventory.Refused): inspector.container('controller')
            row['port_bindings']=ports;row['mounts'][0]['Source']='/unselected'
            with self.assertRaises(inventory.Refused): inspector.container('controller')

    def test_exact_five_profiles_keep_one_original_owner_and_refuse_relabel_or_scope_change(self):
        values=fixture()
        self.assertEqual([r['employee_id'] for r in subject.runtime_bindings(values)],
            ['ada-private']*3+['bora-private','deniz-private'])
        for case in ('owner','oauth_directory','profile','model','workspace','extra','original_volume','receipt_hash'):
            changed=copy.deepcopy(values); config=changed[str(inventory.D3_CONTROLLER_CONFIG)]
            row=config['profiles'][3]
            if case=='owner': row['oauth_owner']['employee_id']='bora-private'
            elif case=='oauth_directory': row['oauth_directory']=str(inventory.D3_ROOT/'bora-private/oauth')
            elif case=='profile': row['binding']['profile_ref']='ortak-private-20260905-ada-oauth-v0'
            elif case=='model': row['binding']['model']='unselected'
            elif case=='workspace': row['binding']['workspace_ref']=inventory.WORKSPACE_REF
            elif case=='extra': config['profiles'].append(copy.deepcopy(row))
            elif case=='original_volume': changed[str(inventory.CONTROLLER_CONFIG/'receipt.json')]['original_untouched']=False
            else: changed[inventory.DEPLOYMENT76_SELECTION['receipt_path']]['current_owners']['sha256']='0'*64
            with self.subTest(case=case),self.assertRaises(inventory.Refused): subject.runtime_bindings(changed)

    def test_new_memory_namespaces_require_owned_receipts_and_explicit_independent_review(self):
        values={}; employees=[]
        for employee in ('ada-private','bora-private','deniz-private'):
            namespace='fixture-'+employee
            binding={'adapter':'honcho','employee_peer':employee,'user_peer':'operator-private','workspace':namespace,'options':{}}
            receipt={'company_id':inventory.COMPANY,'employee_id':employee,'deployment_id':subject.MEMORY_DEPLOYMENT,
                'creation_key':'created-'+employee,'binding':binding,'native_ids':{'workspace':'native-'+employee},
                'resources':{'workspace':{'ownership':'created','resource_ref':'workspace:'+namespace},
                    'employee_peer':{'ownership':'created','resource_ref':'peer:'+namespace+'/'+employee},
                    'user_peer':{'ownership':'created','resource_ref':'peer:'+namespace+'/operator-private'}}}
            root=inventory.STATE if employee=='ada-private' else inventory.D3_ROOT/employee
            values[str(root/'memory/prepared-memory.json')]={'creation_receipt':copy.deepcopy(receipt)}
            row={'employee_id':employee,'binding':binding,'creation_key':receipt['creation_key'],'creation_receipt':receipt}
            if inventory.MAIN_SCHEMA_VERSION in (77,78):
                row['reviewed_employee_destinations']=copy.deepcopy(inventory.EMPLOYEE77_DESTINATIONS[employee])
            if employee=='ada-private': row.update(reviewed_projects=[inventory.REVIEWED_PROJECT],
                reviewed_runtime_projects=[inventory.REVIEWED_PROJECT],reviewed_conversations=inventory.REVIEWED_CONVERSATIONS)
            employees.append(row)
        worker={'memory':{'deployment_id':subject.MEMORY_DEPLOYMENT,'employees':employees,'origin':'http://127.0.0.1:8009',
            'endpoint_ref':'service://ortak-private-20260905/honcho','token_ref':'secret://ortak-private-20260905/honcho-admin',
            'require_creation_receipts':True,'validate_memory_io':True}}
        self.assertEqual(subject._memory(values,worker),list(subject.EMPLOYEES))
        for case in ('foreign_company','adopted','shared_native','implicit_review'):
            changed=copy.deepcopy(worker); row=changed['memory']['employees'][1]
            if case=='foreign_company': row['creation_receipt']['company_id']='foreign'
            elif case=='adopted': row['creation_receipt']['resources']['workspace']['ownership']='adopted'
            elif case=='shared_native': row['creation_receipt']['native_ids']['workspace']='native-ada-private'
            else: row['reviewed_conversations']=inventory.REVIEWED_CONVERSATIONS
            copied=copy.deepcopy(values)
            copied[str(inventory.D3_ROOT/'bora-private/memory/prepared-memory.json')]['creation_receipt']=copy.deepcopy(row['creation_receipt'])
            with self.subTest(case=case),self.assertRaises(inventory.Refused): subject._memory(copied,changed)

    def test_exact_public_readonly_leaf_rejects_link_mode_and_secret_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary).resolve();root.chmod(0o700)
            parent=root/'bora-private';parent.mkdir(mode=0o700)
            profile=parent/'profile';profile.mkdir(mode=0o700)
            path=profile/'ORTAK_PROVIDER.json';path.write_text('{"provider":"openai-codex"}');path.chmod(0o444);profile.chmod(0o555)
            with patch.object(inventory,'D3_ROOT',root):
                value,record=subject.public_file(root,'bora-private/profile/ORTAK_PROVIDER.json')
                self.assertEqual(value,{'provider':'openai-codex'});self.assertEqual(record['mode'],0o444)
                self.assertFalse(subject.is_public_path(root/'bora-private/signer.json'))
                path.chmod(0o600)
                with self.assertRaises(inventory.Refused): subject.public_file(root,'bora-private/profile/ORTAK_PROVIDER.json')
                path.write_text('{"secret_key":"synthetic forbidden"}');path.chmod(0o444)
                with self.assertRaisesRegex(inventory.Refused,'secret_field'): subject.public_file(root,'bora-private/profile/ORTAK_PROVIDER.json')
                profile.chmod(0o700);other=profile/'other';other.write_text('{}');other.chmod(0o444)
                path.unlink();path.symlink_to(other);profile.chmod(0o555)
                with self.assertRaises(inventory.Refused): subject.public_file(root,'bora-private/profile/ORTAK_PROVIDER.json')
                profile.chmod(0o700)


if __name__=='__main__': unittest.main()
