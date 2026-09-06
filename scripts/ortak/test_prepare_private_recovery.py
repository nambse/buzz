"""Production guard/state-machine tests using disposable files; no real stack access."""

import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import private_recovery_inventory as inventory
import prepare_private_recovery as subject


def observation():
    """Historical61 plan fixture, also used to prove later contracts cannot be adopted implicitly."""
    return {'files': {'public': [{'path': '/fixture/public.json', 'sha256': 'a' * 64}],
            'secret_metadata_only': [{'path': '/fixture/secret', 'bytes': 42}],
            'opaque_bindings': {'company_id': inventory.COMPANY}},
            'containers': {'fixture': {'id': 'a' * 64, 'running': True}},
            'native_processes': {'worker': {'pid': 123, 'executable': '/fixture/artifacts/worker/ortak-worker'}},
            'native_ingress': {'artifact': {'path': '/fixture/native.app'}, 'process': None, 'running': False},
            'contained_children': [], 'honcho': {'catalog': {'tables': {'t': 1, **{name: 1 for name in inventory.obligations.HONCHO_BASE}}, 'schema_sha256': 'h',
                'extensions': {'vector': '0.8.0'}, 'owners': ['ortak_honcho']}},
            'main_database': {'schema_sha256': 'm', 'migration_checksums': [[v, 'a' * 96, True] for v in range(1, 62)], 'tables': {'t': 1}},
            'observed_at': 'fixture', 'quiesced': False, 'cross_store_snapshot': False}


def owner_fixture(name, pid):
    """Synthetic process generation with the selected public artifact/launcher paths."""
    return {'pid': pid, 'session_id': pid + 1, 'uid': os.getuid(), 'cwd': str(inventory.STATE),
        'executable': str(inventory.NATIVE_BINARIES[name]), 'sha256': 'b' * 64,
        'launcher': str(inventory.NATIVE_LAUNCHERS[name]), 'launcher_sha256': 'a' * 64,
        'started_at': 'Sun Sep 6 03:53:10 2026',
        'identity': f'{pid} {os.getuid()} Sun Sep 6 03:53:10 2026'}


def fixture_hashes():
    """Synthetic read metadata pins both intermediate and final owner receipts."""
    return {str(inventory.API_CONFIG): inventory.API_CONFIG_SHA,
        str(inventory.WORKER_CONFIG): inventory.WORKER_CONFIG_SHA,
        str(inventory.WORKER_OWNERS): inventory.WORKER_OWNERS_SHA,
        str(inventory.CURRENT_OWNERS): inventory.CURRENT_OWNERS_SHA}


def runtime_fixture():
    """Synthetic public receipts at the exact selected paths; never reads real profile/auth files."""
    values = {str(root / name): {} for root, names in inventory.PUBLIC_FILES.items() for name in names}
    rows = []
    for path, model, effort in inventory.RUNTIME_VARIANTS:
        binding = {'adapter': 'hermes', 'profile_ref': 'ortak-private-20260905-ada-oauth-v0',
            'model': model, 'workspace_ref': inventory.WORKSPACE_REF,
            'credential_refs': ['secret://ortak-private-20260905/ada-codex-oauth-v0'],
            'options': {'reasoning_effort': effort}}
        rows.append({'binding': binding, 'directory': str(path), 'employee_id': 'ada-private',
            'oauth_directory': str(inventory.RUNTIME / 'oauth/ada-private')})
        values[str(path / 'ORTAK_RUNTIME_BINDING.json')] = copy.deepcopy(binding)
        values[str(path / 'ORTAK_PROVIDER.json')] = {'provider': 'openai-codex',
            'credential_ref': binding['credential_refs'][0]}
        values[str(path / 'ORTAK_DISPOSABLE_PROFILE.json')] = {'company_id': inventory.COMPANY,
            'employee_id': 'ada-private', 'profile_ref': binding['profile_ref']}
    values[str(inventory.EVIDENCE / 'private-hermes-controller-selection.json')] = {
        'company_id': inventory.COMPANY, 'root': str(inventory.RUNTIME),
        'config': str(inventory.RUNTIME / 'controller/config.json'),
        'journal': str(inventory.RUNTIME / 'state/journal.sqlite'),
        'oauth_directory': str(inventory.RUNTIME / 'oauth/ada-private'), 'binding': {**rows[0]['binding'],'workspace_ref':'none'}}
    values[str(inventory.CONTROLLER_CONFIG / 'controller/config.json')] = {'company_id':inventory.COMPANY,
        'profiles':rows,'executor':{'image':inventory.WORKER_IMAGE,'validated_digest':inventory.WORKER_IMAGE,
            'workspace_validated_digest':inventory.WORKER_IMAGE}}
    values[str(inventory.RUNTIME_RECEIPT)]={'status':'controller_handoff_verified','new_id':inventory.SERVICES['controller'][0],
        'image':inventory.SERVICES['controller'][2],'old_journal_rows_preserved':True,'oauth_reused_in_place':True,
        'capabilities':{'capabilities':['workspace_text_read']}}
    if inventory.JOURNAL_VOLUME is not None:
        values[str(inventory.CONTROLLER_CONFIG/'controller/config.json')]['executor']['journal_volume']=copy.deepcopy(inventory.JOURNAL_VOLUME)
        values[str(inventory.RUNTIME_RECEIPT)]={'id':inventory.SERVICES['controller'][0],
            'image':inventory.SERVICES['controller'][2],'capabilities':['workspace_text_read']}
        values[str(inventory.CONTROLLER_CONFIG/'receipt.json')]={'status':'journal_volume_prepared_not_activated',
            'original_untouched':True,'temporary_container_removed':True,'selection':copy.deepcopy(inventory.JOURNAL_VOLUME)}
    values[str(inventory.STATE/'memory/prepared-memory.json')]={'creation_receipt':{'company_id':inventory.COMPANY,
        'employee_id':'ada-private','deployment_id':'fixture'},'origin':'http://127.0.0.1:8009',
        'token_ref':'secret://ortak-private-20260905/honcho-admin'}
    root=inventory.CURRENT_ROLLOUT
    values[str(root/'main-migration76/receipt.json')]={'status':'migrated_verified','code':'ok',
        'to_schema':inventory.MAIN_SCHEMA_VERSION}
    values[str(root/'main-migration76/database-after.json')]={'metadata':{
        'migration_checksums':[[v,'a'*96,True] for v in range(1,inventory.MAIN_SCHEMA_VERSION+1)]}}
    values[str(inventory.HONCHO_ROLLOUT/'honcho-verified.json')]={'status':'upgraded_verified',
        'new_api':inventory.SERVICES['honcho_api'][0],'new_image':inventory.SERVICES['honcho_api'][2],
        'metadata_unchanged':True,'settings_sequences_unchanged':True}
    original={name:owner_fixture(name,100+n) for n,name in enumerate(inventory.NATIVE_WRITERS)}
    original['ortak-worker'].update(executable=str(inventory.BACKEND_ARTIFACTS/'ortak-worker'),
        launcher=str(root/'launch-ortak-worker76.py'))
    original['native']={'pid':300,'launcher':str(root/'launch-native76.py'),
        'launcher_sha256':'c'*64,'sha256':'d'*64}
    worker_owners=copy.deepcopy(original); worker_owners['ortak-worker']=owner_fixture('ortak-worker',200)
    owners=copy.deepcopy(worker_owners)
    owners['native']={'pid':400,'launcher':str(subject.native_ingress.LAUNCHER),
        'launcher_sha256':subject.native_ingress.LAUNCHER_SHA,'sha256':subject.native_ingress.EXPECTED_SHA}
    values[str(root/'current-owners76.json')]=copy.deepcopy(original)
    values[str(inventory.WORKER_OWNERS)]=copy.deepcopy(worker_owners)
    values[str(inventory.CURRENT_OWNERS)]=copy.deepcopy(owners)
    values[str(root/'live76-proof-25a1ac11c7e041778cbe413baa681dcd/receipt.json')]={'status':'passed',
        'schema':inventory.MAIN_SCHEMA_VERSION,'owners':str(root/'current-owners76.json'),
        'health':{'relay_liveness':200,'relay_readiness':200,'api_unauthenticated':401}}
    values[str(inventory.WORKER_ROLLOUT/'deployed.json')]={'status':'passed','schema':inventory.MAIN_SCHEMA_VERSION,
        'current_owners':str(inventory.WORKER_OWNERS),'current_owners_sha256':inventory.WORKER_OWNERS_SHA,
        'worker':copy.deepcopy(worker_owners['ortak-worker']),'all_other_owners_unchanged':True,
        'no_image_or_config_or_schema_change':True}
    values[str(root/'launcher-selection.json')]={'launchers':{
        **{name:{'path':row['launcher'],'sha256':row['launcher_sha256']}
            for name,row in original.items() if name!='native'},
        'native':{'path':original['native']['launcher'],'sha256':original['native']['launcher_sha256'],
            'binary_sha256':original['native']['sha256']}},'worker_config_sha256':inventory.WORKER_CONFIG_SHA,
        'helper_import_root_retained':str(inventory.CURRENT_LAUNCH_HELPERS),
        'reviewed_conversations':copy.deepcopy(inventory.REVIEWED_CONVERSATIONS)}
    values[str(subject.native_ingress.BUILD_RECEIPT)]={'status':'built_policy_verified','source_unchanged':True,
        'previous_sha256':original['native']['sha256'],'native_sha256':owners['native']['sha256'],
        'launcher':owners['native']['launcher'],'launcher_sha256':owners['native']['launcher_sha256']}
    values[str(inventory.NATIVE_ROLLOUT/'deployed.json')]={'status':'passed','schema':inventory.MAIN_SCHEMA_VERSION,
        'current_owners':str(inventory.CURRENT_OWNERS),'current_owners_sha256':inventory.CURRENT_OWNERS_SHA,
        'native':copy.deepcopy(owners['native']),'four_backend_owners_unchanged':True,
        'build_receipt':str(subject.native_ingress.BUILD_RECEIPT)}
    grant={'workspace_ref':inventory.WORKSPACE_REF}
    values[str(inventory.BACKEND_ROLLOUT/'config/grant.json')]=grant
    values[str(inventory.WORKER_CONFIG)]={'memory':{'employees':[{'employee_id':'ada-private',
        'reviewed_runtime_projects':[inventory.REVIEWED_PROJECT],
        'reviewed_conversations':copy.deepcopy(inventory.REVIEWED_CONVERSATIONS)}]},'workspace':{
            **{key:inventory.WORKSPACE_SELECTION[key] for key in ('input_root','run_root','reader_binary','reader_sha256')},
            'expires_at':'fixture-fixed-expiry','register_selected_inputs':False,'grants':[grant]}}
    values[str(inventory.WORKSPACE_REGISTRATION)]={'status':'verified','worker_mode':'retained','expiry_unchanged':True,
        'reader_sha256':inventory.WORKSPACE_SELECTION['reader_sha256'],
        'registry':{'bindings':[{'expires_at':'fixture-fixed-expiry','grant':grant}]}}
    return values


class RecoveryTests(unittest.TestCase):
    def test_selected_eight_binary_artifact_receipt_requires_current_schema_and_exact_entries(self):
        original = {'status': 'staged_not_deployed', 'schema': inventory.MAIN_SCHEMA_VERSION,
            'binaries': {name: {'bytes': 12, 'sha256': 'a' * 64} for name in
                [*inventory.NATIVE_WRITERS, 'buzz-admin', 'ortak-cohort', 'ortak-provision','ortak-workspace-reader']}}
        inventory.native_artifact_receipt(original)
        rebuilt = copy.deepcopy(original)
        rebuilt['binaries']['buzz-relay']['rebuilt'] = True
        inventory.native_artifact_receipt(rebuilt)
        rebuilt['binaries']['buzz-relay']['rebuilt'] = 'true'
        with self.assertRaises(subject.Refused):
            inventory.native_artifact_receipt(rebuilt)
        for case in ['schema', 'status', 'missing', 'new', 'hash', 'size', 'unknown_field']:
            value = copy.deepcopy(original)
            if case == 'schema': value['schema'] -= 1
            elif case == 'status': value['status'] = 'unreviewed'
            elif case == 'missing': del value['binaries']['ortak-management']
            elif case == 'new': value['binaries']['unrelated'] = {'bytes': 12, 'sha256': 'b' * 64}
            elif case == 'hash': value['binaries']['ortak-worker']['sha256'] = 'invalid'
            elif case == 'size': value['binaries']['ortak-worker']['bytes'] = True
            else: value['binaries']['ortak-worker']['directory'] = '/unrelated'
            with self.subTest(case=case), self.assertRaises(subject.Refused): inventory.native_artifact_receipt(value)

    def test_file_capture_binds_all_three_profiles_to_the_one_selected_oauth_store(self):
        values = runtime_fixture()
        hashes = fixture_hashes()
        def public_json(root, relative):
            path = str(root / relative)
            return copy.deepcopy(values[path]), {'path': path, 'sha256': hashes.get(path, 'a' * 64)}
        with patch.object(inventory, 'public_json', side_effect=public_json), \
            patch.object(inventory, 'file_metadata', side_effect=lambda root, name, **_: {'path': str(root / name)}):
            result = subject.files()
        self.assertEqual(len(result['opaque_bindings']['runtime_variants']), 3)
        oauth_roots = {row['oauth_directory'] for row in result['opaque_bindings']['runtime_variants']}
        self.assertEqual(oauth_roots, {str(inventory.RUNTIME / 'oauth/ada-private')})
        public_paths = {row['path'] for row in result['public']}
        self.assertTrue({str(inventory.WORKER_OWNERS),str(inventory.CURRENT_OWNERS),
            str(subject.native_ingress.BUILD_RECEIPT),str(inventory.NATIVE_ROLLOUT/'deployed.json')} <= public_paths)
        for path, _, _ in inventory.RUNTIME_VARIANTS:
            self.assertTrue({str(path / name) for name in ['ORTAK_RUNTIME_BINDING.json',
                'ORTAK_PROVIDER.json', 'ORTAK_DISPOSABLE_PROFILE.json']} <= public_paths)
        self.assertNotIn(str(inventory.BACKEND_ROLLOUT / 'paused-drain.json'), public_paths)

    def test_current76_config_and_receipts_reject_old_or_expanded_authority(self):
        root = inventory.CURRENT_ROLLOUT
        self.assertEqual(subject.deployment_bindings(runtime_fixture())['schema'], inventory.MAIN_SCHEMA_VERSION)
        for case in ['old_schema', 'old_honcho', 'new_image', 'old_worker', 'missing_project', 'new_employee']:
            values = runtime_fixture()
            if case == 'old_schema': values[str(root/'main-migration76/database-after.json')]['metadata']['migration_checksums'].pop()
            elif case == 'old_honcho': values[str(inventory.HONCHO_ROLLOUT / 'honcho-verified.json')]['new_api'] = 'a' * 64
            elif case == 'new_image': values[str(inventory.HONCHO_ROLLOUT / 'honcho-verified.json')]['new_image'] = 'sha256:' + 'a' * 64
            elif case == 'old_worker': values[str(inventory.CURRENT_OWNERS)]['ortak-worker']['launcher'] = str(root / 'launch-ortak-worker.py')
            elif case == 'missing_project': values[str(inventory.WORKER_CONFIG)]['memory']['employees'][0]['reviewed_runtime_projects'] = []
            else: values[str(inventory.WORKER_CONFIG)]['memory']['employees'].append({'employee_id': 'second-private'})
            with self.subTest(case=case), self.assertRaises(subject.Refused): subject.deployment_bindings(values)

    def test_unknown_missing_duplicate_or_rebound_runtime_variant_refuses(self):
        key = str(inventory.CONTROLLER_CONFIG / 'controller/config.json')
        for case in ['missing', 'duplicate', 'unknown', 'oauth', 'employee', 'model', 'effort',
            'credential', 'binding_file', 'provider_file', 'marker_file', 'new_controller', 'new_worker']:
            values = runtime_fixture(); rows = values[key]['profiles']; row = rows[1]
            if case == 'missing': rows.pop()
            elif case == 'duplicate': rows[2] = copy.deepcopy(row)
            elif case == 'unknown': row['directory'] = '/unrelated/profile'
            elif case == 'oauth': row['oauth_directory'] = '/unrelated/auth'
            elif case == 'employee': row['employee_id'] = 'unrelated'
            elif case == 'model': row['binding']['model'] = 'unreviewed-model'
            elif case == 'effort': row['binding']['options']['reasoning_effort'] = 'ultra'
            elif case == 'credential': row['binding']['credential_refs'] = ['secret://unrelated/ref']
            elif case.endswith('_file'):
                name = {'binding_file': 'ORTAK_RUNTIME_BINDING.json', 'provider_file': 'ORTAK_PROVIDER.json',
                    'marker_file': 'ORTAK_DISPOSABLE_PROFILE.json'}[case]
                values[str(Path(row['directory']) / name)] = {}
            elif case == 'new_controller': values[str(inventory.RUNTIME_RECEIPT)]['id' if inventory.JOURNAL_VOLUME else 'new_id'] = 'b' * 64
            elif case == 'new_worker': values[key]['executor']['validated_digest'] = 'sha256:' + 'b' * 64
            with self.subTest(case=case), self.assertRaises(subject.Refused): subject.runtime_bindings(values)

    def test_workspace_selection_rejects_extra_roots_changed_reader_publish_mode_and_expiry(self):
        for case in ('root','reader','publish','expiry','grant','helper_import','capability'):
            values=runtime_fixture(); selected=values[str(inventory.WORKER_CONFIG)]['workspace']
            if case=='root':selected['run_root']='/unrelated'
            elif case=='reader':selected['reader_sha256']='f'*64
            elif case=='publish':selected['register_selected_inputs']=True
            elif case=='expiry':selected['expires_at']='changed-expiry'
            elif case=='grant':selected['grants']=[{'workspace_ref':'unrelated'}]
            elif case=='helper_import':values[str(inventory.CURRENT_ROLLOUT/'launcher-selection.json')]['helper_import_root_retained']='/unrelated'
            else:values[str(inventory.CONTROLLER_CONFIG/'controller/config.json')]['executor']['workspace_validated_digest']='f'*64
            with self.subTest(case=case),self.assertRaises(subject.Refused):
                subject.runtime_bindings(values);subject.deployment_bindings(values)

    def test_immutable_public_profile_mode_is_accepted_but_secret_mode_not_expanded(self):
        path=self.write('public.json','{"credential_ref":"secret://fixture/ref"}',0o400)
        subject.inventory.public_json(self.root,path.name)
        with self.assertRaises(subject.Refused):subject.inventory.file_metadata(self.root,path.name)
        path.chmod(0o444)
        with self.assertRaises(subject.Refused):subject.inventory.public_json(self.root,path.name)

    def test_new_schema_requires_explicit_selection_review_before_new_preparation(self):
        inspector = inventory.Inventory.__new__(inventory.Inventory)
        class Commands:
            def inspect(self): return {'container_id': inventory.SERVICES['postgres'][0]}
            def metadata(self, *args): return {'migration_checksums': [[inventory.MAIN_SCHEMA_VERSION + 1, 'fixture', True]]}
        inspector.commands = Commands()
        with self.assertRaisesRegex(subject.Refused, 'schema_review_required'): inspector.main_database()

    def setUp(self):
        # One setup owns both the local-file fixture and its historical
        # selection. A later setUp must not override this isolation.
        for name in ('DEPLOYMENT76_SELECTION','SCORER_SELECTION'):
            context=patch.object(inventory,name,None);context.start();self.addCleanup(context.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.root.chmod(0o700)

    def write(self, relative, value, mode=0o600):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        path.write_text(value)
        path.chmod(mode)
        return path

    def test_honcho_url_strips_all_secret_material_and_requires_exact_target(self):
        token = 'fixture-do-not-leak'
        uri = f'postgresql+psycopg://ortak_honcho:{token}@honcho-test-db:5432/ortak_honcho_adapter_test'
        result = inventory.honcho_url(uri)
        self.assertEqual(result, {'host': 'honcho-test-db', 'port': 5432,
            'role': 'ortak_honcho', 'database': 'ortak_honcho_adapter_test'})
        self.assertNotIn(token, json.dumps(result))
        for changed in [uri.replace('honcho-test-db', 'outside.test'), uri.replace('5432', '55433'),
                        uri.replace('adapter_test', 'other'), uri + '?options=foreign', uri + '#fragment',
                        uri.replace('postgresql+psycopg', 'postgres'), None, uri + '\n']:
            with self.subTest(value_type=type(changed).__name__):
                with self.assertRaises(inventory.Refused) as error:
                    inventory.honcho_url(changed)
                self.assertNotIn(token, str(error.exception))

    def test_saved_setting_is_private_bounded_and_duplicate_refuses_before_selection(self):
        values = {'DB_CONNECTION_URI': 'postgresql+psycopg://ortak_honcho:fixture-secret@honcho-test-db:5432/ortak_honcho_adapter_test',
            'AUTH_USE_AUTH': 'true', 'AUTH_JWT_SECRET': 'fixture-jwt', 'LLM_OPENAI_API_KEY': 'fixture-provider',
            'EMBED_MESSAGES': 'false', 'CACHE_ENABLED': 'false', 'METRICS_ENABLED': 'false',
            'TELEMETRY_ENABLED': 'false', 'SENTRY_ENABLED': 'false'}
        content = ''.join(f'{k}={v}\n' for k, v in values.items())
        self.write('honcho-tests/service.env', content)
        with patch.object(inventory, 'STATE', self.root):
            result = inventory.saved_honcho_selection()
            self.assertNotIn('fixture', json.dumps(result))
            self.write('honcho-tests/service.env', content + 'DB_CONNECTION_URI=hidden\n')
            with self.assertRaisesRegex(inventory.Refused, 'duplicate'):
                inventory.saved_honcho_selection()

    def test_secret_inventory_does_not_read_or_hash_secret_content(self):
        self.write('secrets/value', 'fixture-secret', 0o444)
        with patch.object(inventory, 'private_file', side_effect=AssertionError('must not read')):
            result = inventory.file_metadata(self.root, 'secrets/value', service_readable=True)
        self.assertEqual(result['bytes'], 14)
        self.assertNotIn('sha256', result)
        self.assertNotIn('fixture-secret', json.dumps(result))

    def test_private_file_guards_reject_parent_symlink_hardlink_and_public_modes(self):
        path = self.write('nested/value', '{}')
        (self.root / 'alias').symlink_to(self.root / 'nested', target_is_directory=True)
        for relative in ['alias/value', '../escape', str(path)]:
            with self.assertRaises(inventory.Refused):
                inventory.file_metadata(self.root, relative)
        os.link(path, self.root / 'linked')
        with self.assertRaises(inventory.Refused):
            inventory.file_metadata(self.root, 'nested/value')
        (self.root / 'linked').unlink()
        path.chmod(0o644)
        with self.assertRaises(inventory.Refused):
            inventory.file_metadata(self.root, 'nested/value')

    def test_public_configuration_secret_fields_are_refused_recursively(self):
        for key in ['access_token', 'refresh_token', 'secret_key', 'password', 'api_key']:
            self.write('public.json', json.dumps({'nested': [{'field': {key: 'fixture-secret'}}]}))
            with self.assertRaisesRegex(inventory.Refused, 'secret_field'):
                inventory.public_json(self.root, 'public.json')
        self.write('public.json', '{"credential_ref":"secret://fixture/opaque"}')
        value, metadata = inventory.public_json(self.root, 'public.json')
        self.assertEqual(value['credential_ref'], 'secret://fixture/opaque')
        self.assertEqual(len(metadata['sha256']), 64)

    def fixture_container(self, key):
        identifier, name, image, volume, target = inventory.SERVICES[key]
        mounts = [{'Type': 'bind', 'Source': source, 'Destination': destination, 'RW': writable}
                  for source, destination, writable in inventory.expected_binds(key)]
        if volume:
            mounts.append({'Type': 'volume', 'Name': volume, 'Source': '/fixture/volume',
                           'Destination': target, 'RW': True})
        row = {'id': identifier, 'name': '/' + name, 'image': image, 'running': True,
               'started_at': 'fixed', 'mounts': mounts, 'ports': {}, 'restart': {}, 'user': '',
               'project': 'ortak-private-20260905', 'service': key,
               'networks': {inventory.HONCHO_NETWORK: {'NetworkID': 'network', 'Aliases': [inventory.HONCHO_HOST]}}}
        volume_row = {'name': volume, 'driver': 'local', 'mountpoint': '/fixture/volume',
                      'project': 'ortak-private-20260905', 'volume': key + '_data'}
        if key == 'honcho_postgres':
            volume_row.update(project=None, volume=None)
        return row, volume_row

    def test_container_added_mount_changed_id_image_volume_or_exposure_refuses(self):
        for mutation in ['id', 'image', 'volume', 'mount', 'port', 'compose']:
            row, volume = self.fixture_container('postgres')
            if mutation == 'id': row['id'] = 'b' * 64
            elif mutation == 'image': row['image'] = 'sha256:' + 'b' * 64
            elif mutation == 'volume': row['mounts'][-1]['Name'] = 'unrelated-volume'
            elif mutation == 'mount': row['mounts'].append({'Type': 'bind', 'Source': '/unrelated', 'Destination': '/extra', 'RW': True})
            elif mutation == 'port': row['ports'] = {'5432/tcp': [{'HostIp': '0.0.0.0', 'HostPort': '55433'}]}
            else: volume['project'] = 'other'
            probe = inventory.Inventory(self.root)
            with patch.object(probe, 'run', side_effect=[json.dumps(row).encode(), json.dumps(volume).encode()]):
                with self.assertRaises(inventory.Refused): probe.container('postgres')

    def test_exact_unlabelled_honcho_volume_needs_frozen_container_and_network_alias(self):
        row, volume = self.fixture_container('honcho_postgres')
        probe = inventory.Inventory(self.root)
        with patch.object(probe, 'run', side_effect=[json.dumps(row).encode(), json.dumps(volume).encode()]):
            result = probe.container('honcho_postgres')
        self.assertIn('exact_retained', result['volume']['authority'])
        row['networks'][inventory.HONCHO_NETWORK]['Aliases'] = ['different']
        with patch.object(probe, 'run', side_effect=[json.dumps(row).encode(), json.dumps(volume).encode()]):
            with self.assertRaisesRegex(inventory.Refused, 'network'):
                probe.container('honcho_postgres')

    def test_container_order_is_canonical_without_discarding_mount_authority(self):
        row, volume = self.fixture_container('redis')
        row['networks'][inventory.HONCHO_NETWORK]['Aliases'].append('second')
        changed = copy.deepcopy(row)
        changed['mounts'].reverse()
        changed['networks'][inventory.HONCHO_NETWORK]['Aliases'].reverse()
        probe = inventory.Inventory(self.root)
        with patch.object(probe, 'run', side_effect=[json.dumps(item).encode() for item in [row, volume, changed, volume]]):
            self.assertEqual(probe.container('redis'), probe.container('redis'))
        changed['mounts'][0]['RW'] = not changed['mounts'][0]['RW']
        with patch.object(probe, 'run', return_value=json.dumps(changed).encode()):
            with self.assertRaises(inventory.Refused):
                probe.container('redis')

    def test_live_setting_parse_emits_only_public_fields_and_does_not_inspect_env_array(self):
        probe = inventory.Inventory(self.root)
        calls = []
        rows = {'honcho_api': {'running': True, 'id': 'api', 'networks': {inventory.HONCHO_NETWORK: {'id': 'selected'}}},
                'honcho_postgres': {'running': True, 'id': 'database', 'networks': {inventory.HONCHO_NETWORK: {'id': 'selected'}}}}
        selection = {'host': inventory.HONCHO_HOST, 'port': 5432, 'role': inventory.HONCHO_ROLE, 'database': inventory.HONCHO_DATABASE}
        catalog = {'database': inventory.HONCHO_DATABASE, 'role': inventory.HONCHO_ROLE,
                   'extensions': {'vector': '0.8.0'}, 'owners': [inventory.HONCHO_ROLE], 'schema_sha256': 'a' * 64,
                   'tables': {'public.' + k: 1 for k in ['ortak_resource_receipts', 'ortak_session_ownership', 'ortak_write_receipts']}}
        def run(args, **kwargs):
            calls.append((args, kwargs))
            return json.dumps(selection if len(calls) == 1 else catalog).encode()
        with patch.object(inventory, 'saved_honcho_selection', return_value=selection), patch.object(probe, 'run', side_effect=run):
            result = probe.honcho(rows)
        self.assertEqual(result['saved_selection'], result['live_api_selection'])
        self.assertFalse(result['cross_store_snapshot'])
        self.assertIn('READ ONLY', calls[1][1]['sql'])
        self.assertIn('ROLLBACK', calls[1][1]['sql'])
        self.assertNotIn('.Config.Env', str(calls))
        self.assertNotIn('pg_dump', str(calls))
        self.assertNotIn('createdb', str(calls))
        self.assertNotIn('PGPASSWORD', str(calls))
        with patch.object(inventory, 'saved_honcho_selection', return_value=selection), patch.object(probe, 'run', return_value=b'{}'):
            with self.assertRaisesRegex(inventory.Refused, 'saved_live_mismatch'):
                probe.honcho(rows)

    def prepare(self, value=None, previous=None):
        with patch.object(inventory, 'STATE', self.root), patch.object(subject, 'selected_root', return_value=self.root):
            return subject.prepare(self.root, previous, observer=lambda output: copy.deepcopy(value or observation()))

    def test_preparation_is_private_sealed_and_truthfully_not_a_backup(self):
        path = self.prepare()
        manifest = json.loads((path / 'preparation.json').read_text())
        self.assertEqual(manifest['status'], 'prepared')
        self.assertFalse(manifest['observation']['quiesced'])
        self.assertFalse(manifest['observation']['cross_store_snapshot'])
        self.assertEqual(manifest['plan_sha256'], subject.sha(manifest['plan']))
        self.assertEqual(path.stat().st_mode & 0o777, 0o700)
        self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in path.iterdir()))
        destination = manifest['plan']['destination']
        self.assertNotIn(destination['main_database'], ['ortak', inventory.HONCHO_DATABASE])
        self.assertFalse(destination['executor'])
        self.assertFalse(destination['docker_socket_mount'])
        self.assertFalse(destination['provider_egress'])
        self.assertFalse(destination['office_egress'])

    def test_revalidation_refuses_new_resource_or_process_authority_and_retains_failure(self):
        original = self.prepare()
        for key in ['containers', 'native_processes', 'files', 'contained_children', 'native_ingress']:
            changed = observation()
            if key == 'contained_children': changed[key].append({'id': 'new'})
            elif key == 'files': changed[key]['secret_metadata_only'].append({'path': '/new/secret'})
            else: changed[key]['new'] = {'id': 'new'}
            with self.assertRaisesRegex(inventory.Refused, 'private_evidence'):
                self.prepare(changed, original / 'preparation.json')
        failures = list((self.root / 'recovery-preparations').glob('*/failure.json'))
        self.assertEqual(len(failures), 5)
        self.assertTrue(all(json.loads(x.read_text())['error_code'] == 'prepared_authority_changed' for x in failures))
        self.assertEqual(json.loads((original / 'preparation.json').read_text())['status'], 'prepared')

    def test_revalidation_accepts_live_row_count_change_but_never_claims_snapshot(self):
        original = self.prepare()
        current = observation()
        current['honcho']['catalog']['tables']['t'] = 2
        current['observed_at'] = 'later'
        fresh = self.prepare(current, original / 'preparation.json')
        manifest = json.loads((fresh / 'preparation.json').read_text())
        self.assertFalse(manifest['observation']['cross_store_snapshot'])

    def test_revalidation_refuses_new_schema_or_table_even_without_process_change(self):
        original = self.prepare()
        for which in ['main_database', 'honcho']:
            current = observation()
            catalog = current[which] if which == 'main_database' else current[which]['catalog']
            catalog['tables']['new_sensitive_receipt'] = 0
            with self.assertRaises(inventory.Refused):
                self.prepare(current, original / 'preparation.json')

    def test_tampered_plan_even_with_recomputed_hash_cannot_add_restore_authority(self):
        original = self.prepare()
        p = original / 'preparation.json'
        value = json.loads(p.read_text())
        value['plan']['destination']['docker_socket_mount'] = True
        value['plan_sha256'] = subject.sha(value['plan'])
        p.write_text(json.dumps(value))
        with patch.object(inventory, 'STATE', self.root):
            with self.assertRaisesRegex(inventory.Refused, 'integrity'):
                subject.load_preparation(p)

    def test_observer_failure_is_durable_without_sealing_or_mutating_sources(self):
        marker = self.write('original', 'preserved')
        with patch.object(inventory, 'STATE', self.root), patch.object(subject, 'selected_root', return_value=self.root):
            with self.assertRaises(inventory.Refused):
                subject.prepare(self.root, observer=lambda output: (_ for _ in ()).throw(OSError('fixture-secret')))
        failure = list((self.root / 'recovery-preparations').glob('*/failure.json'))[0]
        self.assertNotIn('fixture-secret', failure.read_text())
        self.assertFalse((failure.parent / 'preparation.json').exists())
        self.assertEqual(marker.read_text(), 'preserved')


if __name__ == '__main__':
    unittest.main()
