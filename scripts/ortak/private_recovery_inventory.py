"""Fixed selected-stack inventory for recovery preparation; never stop or copy data."""

import hashlib
import json
import os
from pathlib import Path
import re
import stat
from urllib.parse import urlsplit

from backup_private_database import Commands, Refused, COLUMN_ROWS_SQL, SCHEMA_SQL
from private_native_services import private_file
import private_recovery_obligations as obligations
import private_recovery_journal as selected_journal

STATE = Path('/private/tmp/ortak-private-20260905')
RUNTIME = Path('/private/tmp/ortak-hermes-v0-private-20260905')
EVIDENCE = Path('/private/tmp/ortak-v0-evidence')
BACKEND_ROLLOUT = STATE / 'rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6'
# Storage, profiles and the immutable workspace reader remain on their74 selection.
CURRENT_ROLLOUT = STATE / 'rollouts/schema77-121365f433e34b52ac9cb77558f6e694'
HISTORICAL76_ROLLOUT = STATE / 'rollouts/schema76-9d31e77457c54d2c9d219fd8f7d7b434'
D3_ROOT = HISTORICAL76_ROLLOUT / 'd3-employees-4a91157a7b3d4043b5d7cbd417557c34'
D3_RECOVERY = CURRENT_ROLLOUT / 'recovery-selection78-final'
D3_CONTROLLER_CONFIG = CURRENT_ROLLOUT / 'controller/config.json'
D3_CONTROLLER_OBSERVATION = CURRENT_ROLLOUT / 'service-handoff77/controller-started.json'
D3_CONTROLLER_CONFIG_SHA = 'ba6df6fa1525d43e714ba0f768152afb5144fe8370ce8f41be665c82872d65da'
D3_CONTROLLER_OBSERVATION_SHA = '7d623137f4f204b51a6b1615f1a3cad1cdee65268a074f1a0033e60a1945ebae'
# Root observed and froze this complete explicit current selection. No old
# deployment receipt can authorize D3; a replacement needs another source pin.
DEPLOYMENT76_SELECTION = {'receipt_path': '/private/tmp/ortak-private-20260905/rollouts/schema77-121365f433e34b52ac9cb77558f6e694/recovery-selection78-final/deployment.json', 'receipt_sha256': '5e31d4da5cb90971b000ebd46d142c144666bd3fbcde8865366e2d9f2afa8013'}
SCORER_SELECTION = {'receipt_path': '/private/tmp/ortak-private-20260905/rollouts/schema77-121365f433e34b52ac9cb77558f6e694/recovery-selection78-g/scorer-public78-g.json', 'receipt_sha256': '8c5d459cf17d470204d74f0edc6e49a85503536139bc536a55918bddeae98282'}
WORKER_ROLLOUT = HISTORICAL76_ROLLOUT / 'reply-worker-1943c405b30c41f6827fe09ac8503a1f'
NATIVE_ROLLOUT = EVIDENCE / 'native78-final-97a97244333048f4b586e9270878c62e'
CONTROLLER_CONFIG = BACKEND_ROLLOUT / 'journal-volume-7d40b392f693427caa2e5e2be61d84d9'
NATIVE_RESUME = CURRENT_ROLLOUT
CURRENT_LAUNCH_HELPERS = STATE / 'recovery-operations/2805d70cb1674acd8719b119fd886a0d/resume-code'
RUNTIME_RECEIPT = CONTROLLER_CONFIG / 'controller-active.json'
JOURNAL_VOLUME = {'name':'ortak-journal-74-7d40b392f693427caa2e5e2be61d84d9',
    'created_at':'2026-09-06T03:49:25Z','owner_id':'7d40b392-f693-427c-aa2e-5e2be61d84d9'}
HONCHO_ROLLOUT = STATE / 'rollouts/schema73-9cde353c5a0c464982ecffb99de1f671'
BACKEND_ARTIFACTS = STATE / 'artifacts/backend78-timestamp-d7310263619b4de389883c4b3a7fb6f5'
WORKER_ARTIFACTS = BACKEND_ARTIFACTS
MAIN_SCHEMA_VERSION = 78
# Explicit reviewed77 selection, never inferred from discovered tables or files.
JOURNAL_CONFIDENTIAL = {'format':'ortak-confidential-journal-recovery/1',
    'validator_sha256':'3bf9d34c144aef4c47126f4aaaf7725bc4684156f1807e53eb8314bb2871c91a'}
NATIVE_CONFIDENTIAL_APP_DATA = Path('/Users/nambse/Library/Application Support/dev.ortak.private20260905')
DEPLOYMENT77_EVIDENCE = {
    'service_readiness':{'path':str(CURRENT_ROLLOUT/'service-handoff77/readiness.json'),
        'sha256':'28d7a16d432aa9f6a97b28306ab16ae98d60d989567d2297dd7a7e8b06b1d162'},
    'honcho_preservation':{'path':str(CURRENT_ROLLOUT/'honcho77/receipt.json'),
        'sha256':'f9c7be3778baad7e1c19886af778b5853b556dbb1989c148924b908306290670'},
    'target_registration':{'path':str(CURRENT_ROLLOUT/'employee-targets/operations/51e64a88-5295-4087-9a7e-5b0123fec32f/receipt.json'),
        'sha256':'1cca53a6b48e91701693779e8184900b8d07e9bf8b40e5891ac7527d384d2ebd'}}
EMPLOYEE77_DESTINATIONS = {
    'ada-private':[{'destination_channel_id':'f6bcbca6-9974-4792-8f2c-e19718f6bc11',
        'target_id':'4e2d89fb-b6d2-4b8f-81a7-0ddcac6101c7'}],
    'bora-private':[{'destination_channel_id':'f6bcbca6-9974-4792-8f2c-e19718f6bc11',
        'target_id':'5609015f-6dc8-42fc-9044-0a01b53d0625'}],
    'deniz-private':[{'destination_channel_id':'f6bcbca6-9974-4792-8f2c-e19718f6bc11',
        'target_id':'c1af3654-ae08-41c2-8741-23cc181f2623'}]}
ENCRYPTED77_WORKER_SELECTION = {'format':'ortak-encrypted-worker/1',
    'key_bindings':[{'key_version':0,'office_binding_id':'c356c2e7-0132-42d7-93c0-29049bfe6cfe',
        'purposes':['dm_decrypt','confidential_wrap','confidential_unwrap','dm_seal'],
        'signer':{'company_id':'a4013353-a84d-49a1-8d2b-10a1caf896fe','employee_id':'deniz-private',
            'public_key':'8e28956a5a4aef42e172ab25893494847be604778e703758b55e4edf403bf5a2',
            'secret_env':'ORTAK_PRIVATE_DENIZ_OFFICE_KEY','signer_ref':'secret://ortak-private-20260905/deniz-office-v0'}}],
    'pair_ids':['70666e4a-b972-40de-9fa6-a169728a4f41'],'relay_origin':'ws://localhost:3038/'}
# Root must explicitly bind future C2 roots and the immutable host reader before
# any populated workspace can enter a newly frozen preparation. Never discover.
WORKSPACE_SELECTION = {'company_id':'a4013353-a84d-49a1-8d2b-10a1caf896fe',
    'input_root':str(BACKEND_ROLLOUT/'inputs'),'run_root':str(BACKEND_ROLLOUT/'runs'),
    'reader_binary':str(STATE/'artifacts/backend74-1cdd73aa319c47b597c40265a3333912/ortak-workspace-reader'),
    'reader_sha256':'5edc64b9dad481c31604dc94bb3e67489a76af5c0dbac1c5a9b13b640ab79857','reader_uid':501}
WORKSPACE_REF = 'input:c2:71aabe0e266c4882af5249768c03cdff'
WORKSPACE_REGISTRATION = BACKEND_ROLLOUT / 'workspace-registration-retained-6329201eb08a4252b621523421583133/receipt.json'
API_CONFIG = CURRENT_ROLLOUT / 'staged-configs/api77.json'
API_CONFIG_SHA = '0cd9417416cf77de4b804f476844cf4c1fbad293f049327c70fead2c46525880'
WORKER_CONFIG = CURRENT_ROLLOUT / 'staged-configs/worker-final.json'
WORKER_CONFIG_SHA = 'b5da8024fa4d22b7e4d12e040434a09fde703b9a88a23b29cc08c01490d7f68f'
REVIEWED_PROJECT = '3a06f7cf-ff9c-4deb-bb8c-7ef422eb9b6e'
REVIEWED_CONVERSATIONS = [{'project_id': REVIEWED_PROJECT, 'channel_id': 'f6bcbca6-9974-4792-8f2c-e19718f6bc11'}]
COMPANY = 'a4013353-a84d-49a1-8d2b-10a1caf896fe'
HONCHO_DATABASE = 'ortak_honcho_adapter_test'
HONCHO_ROLE = 'ortak_honcho'
HONCHO_HOST = 'honcho-test-db'
HONCHO_NETWORK = 'ortak-honcho-test-20260905'
WORKER_IMAGE = 'sha256:80aaa3d95b6abb4105f849e33bf4650653718be14fd274e211ee45bd26d75cee'
RUNTIME_VARIANTS = ((BACKEND_ROLLOUT / 'profiles/variant-0', 'gpt-6-astra', 'max'),
    (BACKEND_ROLLOUT / 'profiles/variant-1', 'gpt-5.6-sol', 'high'),
    (BACKEND_ROLLOUT / 'profiles/variant-2', 'gpt-6-astra', 'high'))
NATIVE_WRITERS = ('buzz-relay', 'ortak-server', 'ortak-worker', 'ortak-management')
WORKER_OWNERS = HISTORICAL76_ROLLOUT/'current-owners76-reply.json'
WORKER_OWNERS_SHA = 'c60aab0ce0b96f1e1f0acc8dfdf2dd593dbbb771ba70779a65a7d1d53c20e7be'
CURRENT_OWNERS = CURRENT_ROLLOUT / 'current-owners78-final.json'
CURRENT_OWNERS_SHA = 'd76cc84ab431d49a033602fb847c7e2af2534d691cd71a80ac6d313941e692d5'
NATIVE_BINARIES = {'buzz-relay': Path('/private/tmp/ortak-private-20260905/artifacts/backend78-pruned-bfdf0d3a61a94a02a79de44a4b13e43a/buzz-relay'), 'ortak-management': Path('/private/tmp/ortak-private-20260905/artifacts/backend78-timestamp-d7310263619b4de389883c4b3a7fb6f5/ortak-management'), 'ortak-server': Path('/private/tmp/ortak-private-20260905/artifacts/backend78-timestamp-d7310263619b4de389883c4b3a7fb6f5/ortak-server'), 'ortak-worker': Path('/private/tmp/ortak-private-20260905/artifacts/backend78-timestamp-d7310263619b4de389883c4b3a7fb6f5/ortak-worker')}
NATIVE_LAUNCHERS = {'buzz-relay': Path('/private/tmp/ortak-private-20260905/rollouts/schema77-121365f433e34b52ac9cb77558f6e694/cutover78-pruned-a411e81a69f74dd8803b8058b819211d/launch-buzz-relay78.py'), 'ortak-management': Path('/private/tmp/ortak-private-20260905/recovery-operations/2805d70cb1674acd8719b119fd886a0d/source-resume-da75bf0a64e54d908e64affcd7887dfe/ortak-management-resume.py'), 'ortak-server': Path('/private/tmp/ortak-private-20260905/recovery-operations/2805d70cb1674acd8719b119fd886a0d/source-resume-da75bf0a64e54d908e64affcd7887dfe/ortak-server-resume.py'), 'ortak-worker': Path('/private/tmp/ortak-private-20260905/recovery-operations/2805d70cb1674acd8719b119fd886a0d/source-resume-da75bf0a64e54d908e64affcd7887dfe/ortak-worker-resume.py')}
NATIVE_RECEIPTS = {name: CURRENT_OWNERS for name in NATIVE_WRITERS}
SERVICES = {
    'postgres': ('01ad59c9f79fd50e47281ef85b829fb2a8d556f627a43b175e36fc8ecfde53c7',
                 'ortak-private-20260905-postgres-1',
                 'sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94',
                 'ortak-private-20260905_postgres_data', '/var/lib/postgresql/data'),
    'redis': ('90776da21e0a84d0f3e369e6dc82da0fe5c696afa407502ff772e0b16f48f6f9',
              'ortak-private-20260905-redis-1',
              'sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf',
              'ortak-private-20260905_redis_data', '/data'),
    'minio': ('40163c68f2d617651e7e6460225634e1de73b78dc5b9f0311559095de41ac07a',
              'ortak-private-20260905-minio-1',
              'sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41',
              'ortak-private-20260905_minio_data', '/data'),
    'honcho_postgres': ('e5d4bd4ff4cabcc6f8e8d4712c3001e83fb8cd89291214dd840f4ea5edfe3d88',
                        'ortak-honcho-test-db-20260905',
                        'sha256:cf134a767f474095eeba57e0117be8e568e011a63f33fbf252f14c9b760f8e6f',
                        'ortak-honcho-test-data-20260905', '/var/lib/postgresql/data'),
    'honcho_api': ('6881b3162ae9d90e62642fb19cf9db4f55202c10a6dc77ad6b3bf9f8f10e396c',
                  'ortak-honcho77-121365f433e3',
                  'sha256:fb13e7f8fa0ae66e02b1097d89acfee23ea4c169610fe1494a949fde86db1dc3', None, None),
    'controller': ('79d31b8b6fd59377c6a611d02ed1b9c1160f8a212c905f1a7b2deb2db641da3f',
                   'ortak-hermes-shared77-121365f433e3',
                   'sha256:4cea528012f51086598e7898d3d3e9264c0fe710aba6f142cad1284a410f9361', None, None),
}
PUBLIC_FILES = {
    STATE: ['.ortak-private-stack.json',
            'memory/bootstrap.json', 'memory/worker-memory.json',
            'memory/prepared-memory.json', 'memory/worker-memory-prepared.json',
            'provisioning/ada-oauth-v0.json'],
    RUNTIME: ['controller/config.json', 'profiles/ada-private/ORTAK_DISPOSABLE_PROFILE.json',
              'profiles/ada-private/ORTAK_RUNTIME_BINDING.json', 'profiles/ada-private/ORTAK_PROVIDER.json'],
    EVIDENCE: ['private-oauth-selection.json', 'private-hermes-controller-selection.json',
              'private-hermes-controller-start.json',
              'native-refresh74-build-352f5cc6a50b42d38f8778833b6322fa/receipt.json',
              'native-refresh74-build-352f5cc6a50b42d38f8778833b6322fa/current-native-owner.json',
              'native76-build-149271a917ae4b5389c163c6706d54e4/receipt.json'],
    HONCHO_ROLLOUT: ['honcho-verified.json','config/worker73-initial.json'],
    CONTROLLER_CONFIG: ['controller/config.json','controller-active.json','receipt.json'],
    STATE / 'recovery-operations/c8b8d10995044751be326de19917cef1': ['resume-verification-1446bb1e1b784068a650c415d0e3dc87/receipt.json'],
    HISTORICAL76_ROLLOUT: ['artifact-selection.json', 'launcher-selection.json', 'current-owners76.json',
        'current-owners76-reply.json', 'current-owners76-mentions.json',
        'current-owners76-d3-luna.json',
        'main-migration76/receipt.json',
        'live76-proof-25a1ac11c7e041778cbe413baa681dcd/receipt.json', 'config/worker76-conversation.json'],
    CURRENT_ROLLOUT: ['current-owners78-final.json', 'recovery-selection78-final/deployment.json', 'recovery-selection78-final/selection-receipt.json', 'recovery-selection78-g/scorer-public78-g.json', 'current-owners77.json','current-owners78.json','current-owners78-timestamp.json',
        'current-owners78-retirement.json',
        'host-launchers78-timestamp/finalized.json','staged-configs/api77.json','staged-configs/worker-final.json',
        'controller/config.json','service-handoff77/controller-created.json','service-handoff77/controller-started.json',
        'service-handoff77/honcho-created.json','service-handoff77/honcho-started.json','service-handoff77/readiness.json',
        'scorer-resume.json','resume-scorer-proof/resumed-owner.json',
        'main-migration77/receipt.json','main-migration78/receipt.json','main-migration78/database-after.json','honcho77/receipt.json',
        'recovery-selection77/deployment.json','recovery-selection77/scorer-public77.json','recovery-selection78/deployment.json',
        'recovery-selection78-timestamp/deployment.json',
        'recovery-selection78-retirement/deployment.json',
        'employee-targets/operations/51e64a88-5295-4087-9a7e-5b0123fec32f/receipt.json','encrypted-pair77/disabled.json'],
    WORKER_ROLLOUT: ['deployed.json'],
    NATIVE_ROLLOUT: ['receipt.json','launch-intent.json'],
    D3_ROOT: ['staged-configs/api.json',
        'semantic-luna-cutover/worker.json','semantic-luna-cutover/semantic.json',
        'recovery-selection-2814ed6ecac8480b88770c95c7f26f8e/controller.json',
        'recovery-selection-2814ed6ecac8480b88770c95c7f26f8e/deployment.json',
        'recovery-selection-2814ed6ecac8480b88770c95c7f26f8e/scorer-public-v2.json'] +
        [employee+'/'+name for employee in ('bora-private','deniz-private') for name in (
            'selection.json','controller-profile.json','oauth-connection.json',
            'profile/ORTAK_DISPOSABLE_PROFILE.json','profile/ORTAK_RUNTIME_BINDING.json','profile/ORTAK_PROVIDER.json',
            'memory/bootstrap.json','memory/prepared-memory.json','memory/worker-memory-prepared.json')],
    BACKEND_ROLLOUT: ['controller/config.json','controller-switched.json','current-owners74.json',
        'current-owners74-volume.json','current-owners74-after-g-volume.json',
        'config/catalog74-files-manual.json','files-manual-active-selection.json',
        'g74-daemon-bind-selection-d10b6943549b4dd1b96133409fa3ec63/receipt.json',
        'files-volume-final-49ee3bf8cdd84b608138c808fbfd6935/receipt.json',
        'office-restore-volume74-6c1eb5e85d274bf9ade2260a493c2ac1/receipt.json',
        'manual-work-cohort/154db79a3fbc412d96caadcfccb070bf/receipt.json',
        'current-health74.json','native-source-launcher-selection.json',
        'main-migration-after-count-correction/receipt.json','main-migration-after-count-correction/database-after.json',
        'workspace-registration-retained-6329201eb08a4252b621523421583133/receipt.json',
        'config/api74.json','config/worker74-retained.json','config/grant.json'] +
        [str(path.relative_to(BACKEND_ROLLOUT) / filename) for path,_,_ in RUNTIME_VARIANTS
            for filename in ['ORTAK_DISPOSABLE_PROFILE.json','ORTAK_RUNTIME_BINDING.json','ORTAK_PROVIDER.json']],
}
SECRET_FILES = {
    STATE: ['identities.json', 'runtime.env', 'secrets/postgres-password',
            'secrets/redis-password', 'secrets/redis.conf', 'object-store/credentials.json',
            'object-store/root-user', 'object-store/root-password',
            'honcho-tests/postgres-password', 'honcho-tests/service.env'],
    RUNTIME: ['controller/service-token', 'oauth/ada-private/ORTAK_OAUTH_IDENTITY.json',
              'oauth/ada-private/oauth-state.json', 'oauth/ada-private/oauth.lock'],
    D3_ROOT: ['bora-private/signer.json','bora-private/office-signer.json',
              'deniz-private/signer.json','deniz-private/office-signer.json',
              'semantic-stage/worker-service-token','semantic-stage/scorer-token/service-token'],
}
SECRET_KEYS = {'access_token', 'refresh_token', 'id_token', 'secret_key', 'password',
               'api_key', 'service_token', 'jwt_secret', 'authorization'}
CONTAINER_FORMAT = ('{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Image}},'
    '"running":{{json .State.Running}},"started_at":{{json .State.StartedAt}},'
    '"mounts":{{json .Mounts}},"networks":{{json .NetworkSettings.Networks}},'
    '"ports":{{json .NetworkSettings.Ports}},"restart":{{json .HostConfig.RestartPolicy}},'
    '"port_bindings":{{json .HostConfig.PortBindings}},'
    '"user":{{json .Config.User}},"project":{{json (index .Config.Labels "com.docker.compose.project")}},'
    '"journal_company":{{json (index .Config.Labels "org.ortak.company")}},'
    '"journal_owner":{{json (index .Config.Labels "org.ortak.journal_owner")}},'
    '"service":{{json (index .Config.Labels "com.docker.compose.service")}}}')


def expected_binds(key):
    """An added mount never becomes recovery authority through inventory discovery."""
    paths = {
        'postgres': [('secrets/postgres-password', '/run/secrets/postgres_password')],
        'redis': [('secrets/redis-password', '/run/secrets/redis_password'),
                  ('secrets/redis.conf', '/run/secrets/redis_config')],
        'minio': [('object-store/root-user', '/run/secrets/minio_root_user'),
                  ('object-store/root-password', '/run/secrets/minio_root_password')],
        'honcho_postgres': [('honcho-tests/postgres-password', '/run/secrets/postgres_password')],
        'honcho_api': [],
    }
    if key == 'controller':
        if DEPLOYMENT76_SELECTION is not None:
            expected_id='4cd42e425ca2a323bc5b603a80093c995fff832f24b324c42d7df9011a4a182d'
            if type(MAIN_SCHEMA_VERSION) is int and MAIN_SCHEMA_VERSION in (77,78):
                expected_id='79d31b8b6fd59377c6a611d02ed1b9c1160f8a212c905f1a7b2deb2db641da3f'
                require(D3_CONTROLLER_CONFIG==CURRENT_ROLLOUT/'controller/config.json',
                    'controller77_config_path_refused')
            require(SERVICES['controller'][0]==expected_id,
                'controller_bind_generation_refused')
            selected = [(RUNTIME/'controller',False),(RUNTIME/'oauth',True),
                (BACKEND_ROLLOUT/'profiles',False),(D3_CONTROLLER_CONFIG.parent,False),
                (D3_ROOT/'bora-private/profile',False),(D3_ROOT/'deniz-private/profile',False)]
            # Exact daemon projection in the pinned root observation. This is
            # not a general /host_mnt prefix equivalence or mount discovery rule.
            return sorted([('/host_mnt'+str(path),str(path),writable) for path,writable in selected]
                + [('/run/host-services/docker.proxy.sock','/var/run/docker.sock',False)])
        if JOURNAL_VOLUME is not None:
            # Exact daemon projection observed for this creation only. These
            # are selected sources, never a prefix-stripping equivalence rule.
            require(SERVICES['controller'][0]=='2ec604cb372a9ca42708d6ca8962b3d66195929f608278cb70b19ba5c339b630',
                'controller_bind_generation_refused')
            return sorted([
                ('/host_mnt/private/tmp/ortak-hermes-v0-private-20260905/controller',str(RUNTIME/'controller'),False),
                ('/host_mnt/private/tmp/ortak-hermes-v0-private-20260905/oauth',str(RUNTIME/'oauth'),True),
                ('/host_mnt/private/tmp/ortak-private-20260905/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6/journal-volume-7d40b392f693427caa2e5e2be61d84d9/controller',str(CONTROLLER_CONFIG/'controller'),False),
                ('/host_mnt/private/tmp/ortak-private-20260905/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6/profiles',str(BACKEND_ROLLOUT/'profiles'),False),
                ('/run/host-services/docker.proxy.sock','/var/run/docker.sock',False)])
        return sorted([(str(RUNTIME / name), str(RUNTIME / name), writable)
                       for name, writable in ([('state', True)] if JOURNAL_VOLUME is None else [])
                           + [('controller', False), ('oauth', True)]]
                      + [('/Users/nambse/.docker/run/docker.sock', '/var/run/docker.sock', False)]
                      + [(str(path), str(path), False) for path in
                         [CONTROLLER_CONFIG/'controller',BACKEND_ROLLOUT/'profiles']])
    return sorted((str(STATE / source), target, False) for source, target in paths[key])


def require(value, code):
    """Refuse with a fixed code, never include the rejected value."""
    if not value:
        raise Refused(code)


def native_writer_set(value):
    """Every current writer must be explicit; an older three-owner registry cannot become capture authority."""
    require(set(value) == set(NATIVE_WRITERS), 'native_writer_inventory_incomplete')


def native_artifact_hash(record, name):
    """Accept a selected consolidated backend receipt or the historical worker-only receipt."""
    require(name in NATIVE_WRITERS, 'native_scope_refused')
    if 'binaries' in record:
        value = record['binaries'].get(name, {}).get('sha256')
    else:
        require(name == 'ortak-worker', 'native_artifact_receipt_refused')
        value = record.get('binary_sha256')
    require(isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value), 'native_artifact_receipt_refused')
    return value


def native_artifact_receipt(record, name=None):
    """Require the selected eight-binary receipt or the exact worker-only76 replacement shape."""
    expected = {*NATIVE_WRITERS, 'buzz-admin', 'ortak-cohort', 'ortak-provision','ortak-workspace-reader'}
    version = record.get('schema')
    if name == 'ortak-worker' and MAIN_SCHEMA_VERSION == 76:
        expected = {'ortak-worker'}
        version = record.get('schema_target')
    require(record.get('status') == 'staged_not_deployed'
        and version == MAIN_SCHEMA_VERSION
        and isinstance(record.get('binaries'), dict) and set(record['binaries']) == expected,
        'native_artifact_receipt_refused')
    for value in record['binaries'].values():
        require(isinstance(value, dict) and set(value) in ({'bytes', 'sha256'}, {'bytes', 'sha256', 'rebuilt'})
            and ('rebuilt' not in value or type(value['rebuilt']) is bool)
            and type(value['bytes']) is int and 0 < value['bytes'] <= 256 * 1024**2
            and isinstance(value['sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', value['sha256']),
            'native_artifact_receipt_refused')


def native_launch_record(name):
    """Normalize only the exact current owners; historical launch receipts cannot replace them."""
    require(name in NATIVE_WRITERS, 'native_scope_refused')
    path = NATIVE_RECEIPTS[name]
    record, metadata = public_json(path.parent, path.name)
    require(metadata['sha256']==CURRENT_OWNERS_SHA and set(record)==set(NATIVE_WRITERS)|{'native'},
        'native_launch_receipt_refused')
    row=record[name]
    require(isinstance(row, dict) and type(row.get('pid')) is int and row['pid'] > 0
        and type(row.get('session_id')) is int and row['session_id'] > 0
        and row.get('uid') == os.getuid() and row.get('cwd') == str(STATE)
        and row.get('executable') == str(NATIVE_BINARIES[name])
        and row.get('launcher') == str(NATIVE_LAUNCHERS[name])
        and isinstance(row.get('started_at'), str)
        and row.get('identity', '').split() == [str(row['pid']), str(os.getuid()), *row['started_at'].split()]
        and all(isinstance(row.get(field), str) and re.fullmatch('[0-9a-f]{64}', row[field])
                for field in ('sha256', 'launcher_sha256')), 'native_launch_receipt_refused')
    return {'status': 'resumed_verified', 'pid': row['pid'], 'session': row['session_id'],
        'binary': row['executable'], 'sha256': row['sha256'], 'identity': row['identity'],
        'launcher': row['launcher'], 'launcher_sha256': row['launcher_sha256'],
        'helper_import_root': str(CURRENT_LAUNCH_HELPERS),
        'provenance': f'actual{MAIN_SCHEMA_VERSION}_root_receipt_with_explicit_reviewed_helper_selection'}, metadata


def directory(path):
    """Require an already-owned real directory; preparation does not adopt it."""
    s = path.lstat()
    require(stat.S_ISDIR(s.st_mode) and s.st_uid == os.getuid()
            and not s.st_mode & 0o077 and path.resolve() == path, 'private_directory_refused')


def file_metadata(root, relative, *, service_readable=False, immutable_public=False):
    """Check every parent and exact leaf without reading or hashing secret bytes."""
    directory(root)
    parts = Path(relative).parts
    require(parts and not Path(relative).is_absolute() and '..' not in parts, 'file_scope_refused')
    for count in range(1, len(parts)):
        directory(root.joinpath(*parts[:count]))
    path = root / relative
    s = path.lstat()
    require(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid()
            and stat.S_IMODE(s.st_mode) in ({0o400,0o600} if immutable_public else {0o600,0o444} if service_readable else {0o600}),
            'private_file_metadata_refused')
    require(s.st_size <= 1024 * 1024, 'configuration_size_refused')
    return {'path': str(path), 'uid': s.st_uid, 'mode': stat.S_IMODE(s.st_mode),
            'bytes': s.st_size, 'device': s.st_dev, 'inode': s.st_ino, 'mtime_ns': s.st_mtime_ns}


def reject_secret_fields(value):
    """Only designated public JSON may be hashed into the preparation manifest."""
    if isinstance(value, dict):
        require(not SECRET_KEYS.intersection(value), 'secret_field_in_public_configuration')
        for child in value.values():
            reject_secret_fields(child)
    elif isinstance(value, list):
        for child in value:
            reject_secret_fields(child)


def public_json(root, relative, *, maximum=65536):
    """Read bounded selected public configuration, with an explicit secret-field guard."""
    metadata = file_metadata(root, relative, immutable_public=True)
    require(type(maximum) is int and 0 < maximum <= 1024 * 1024, 'public_json_bound_refused')
    raw = private_file(root / relative, maximum)
    value = json.loads(raw)
    reject_secret_fields(value)
    require(metadata == file_metadata(root, relative, immutable_public=True), 'configuration_changed_during_read')
    return value, {**metadata, 'sha256': hashlib.sha256(raw.encode()).hexdigest()}


def honcho_url(value):
    """Discard password/URI after resolving only the exact approved database selection."""
    try:
        url = urlsplit(value)
        require(isinstance(value, str) and len(value) <= 2048 and
                url.scheme == 'postgresql+psycopg' and url.hostname == HONCHO_HOST
                and url.port == 5432 and url.username == HONCHO_ROLE
                and url.path == '/' + HONCHO_DATABASE and bool(url.password)
                and not url.query and not url.fragment and not any(ord(c) < 32 for c in value),
                'honcho_database_selection_refused')
    except (ValueError, TypeError, AttributeError):
        raise Refused('honcho_database_selection_refused') from None
    return {'host': HONCHO_HOST, 'port': 5432, 'role': HONCHO_ROLE, 'database': HONCHO_DATABASE}


def saved_honcho_selection():
    """Parse the one explicitly authorized private setting; never return other values."""
    file_metadata(STATE, 'honcho-tests/service.env')
    lines = private_file(STATE / 'honcho-tests/service.env', 8192).splitlines()
    require(all('=' in line for line in lines), 'honcho_setting_file_refused')
    pairs = [line.split('=', 1) for line in lines]
    require(len(pairs) == len(dict(pairs)), 'duplicate_honcho_setting')
    settings = dict(pairs)
    require(set(settings) == {'DB_CONNECTION_URI', 'AUTH_USE_AUTH', 'AUTH_JWT_SECRET',
            'LLM_OPENAI_API_KEY', 'EMBED_MESSAGES', 'CACHE_ENABLED', 'METRICS_ENABLED',
            'TELEMETRY_ENABLED', 'SENTRY_ENABLED'} and settings['AUTH_USE_AUTH'] == 'true',
            'honcho_setting_file_refused')
    return honcho_url(settings['DB_CONNECTION_URI'])


LIVE_SELECTION = """import json,os,sys
from urllib.parse import urlsplit
try:
 u=urlsplit(os.environ['DB_CONNECTION_URI'])
 valid=(u.scheme=='postgresql+psycopg' and u.hostname=='honcho-test-db' and u.port==5432 and u.username=='ortak_honcho' and u.path=='/ortak_honcho_adapter_test' and bool(u.password) and not u.query and not u.fragment)
 if not valid: sys.exit(3)
 print(json.dumps({'host':u.hostname,'port':u.port,'role':u.username,'database':u.path[1:]}))
except Exception:
 sys.exit(3)
"""

HONCHO_METADATA = "WITH live_columns AS (" + COLUMN_ROWS_SQL + "), schema_catalog AS (" + SCHEMA_SQL + ") " + r"""
SELECT jsonb_build_object(
 'database',current_database(),'role',current_user,'server_version',current_setting('server_version'),
 'extensions',(SELECT jsonb_object_agg(extname,extversion) FROM pg_extension),
 'tables',(SELECT jsonb_object_agg(name,n) FROM (
   SELECT format('%I.%I',ns.nspname,c.relname) name,
    ((xpath('/table/row/n/text()',query_to_xml(format('SELECT count(*) n FROM %I.%I',ns.nspname,c.relname),false,false,'')))[1]::text)::bigint n
   FROM pg_class c JOIN pg_namespace ns ON ns.oid=c.relnamespace
   WHERE ns.nspname='public' AND c.relkind IN ('r','p')
 ) counts),
 'schema_sha256',(SELECT encode(sha256(convert_to(document::text,'UTF8')),'hex') FROM schema_catalog),
 'owners',(SELECT jsonb_agg(DISTINCT pg_get_userbyid(c.relowner)) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p')),
 'database_encoding',pg_encoding_to_char(d.encoding),'collation',d.datcollate,'ctype',d.datctype
) FROM pg_database d WHERE d.datname=current_database();
"""


class Inventory:
    """Read only fixed Docker/public process fields through the existing bounded runner."""

    def __init__(self, output):
        self.commands = Commands(output)
        self.counter = 0

    def run(self, args, *, sql=None, limit=1024 * 1024):
        """Every local child has bounded output/time; diagnostics stay private."""
        self.counter += 1
        return self.commands.run(f'inventory-{self.counter}', args, sql=sql, ceiling=limit)

    def docker(self, *args):
        """The selected daemon endpoint cannot come from ambient configuration."""
        return self.commands.docker(*args)

    def container(self, key):
        """Bind an immutable ID/image to its named volume and loopback-only exposure."""
        identifier, name, image, volume, target = SERVICES[key]
        row = json.loads(self.run(self.docker('inspect', '--format', CONTAINER_FORMAT, identifier)))
        require(row['id'] == identifier and row['name'] == '/' + name and row['image'] == image,
                'container_identity_refused')
        require(isinstance(row['running'], bool) and len(row['mounts']) <= 16,
                'container_metadata_refused')
        # Docker does not promise array order for mounts or network aliases.
        # Preserve every field while comparing their stable semantic identity.
        row['mounts'] = sorted(row['mounts'], key=lambda mount: (mount['Destination'], mount['Source']))
        require(all(m['Type'] in ['bind', 'volume'] for m in row['mounts'])
                and sorted((m['Source'], m['Destination'], m['RW']) for m in row['mounts']
                           if m['Type'] == 'bind') == expected_binds(key), 'unapproved_mount_refused')
        if key=='controller' and JOURNAL_VOLUME is not None:
            row['volume']=selected_journal.verify_volume(self,row,JOURNAL_VOLUME,COMPANY,RUNTIME/'state')
        elif not volume:
            require(not any(m['Type'] == 'volume' for m in row['mounts']), 'unapproved_volume_refused')
        row['networks'] = {name: {'id': n['NetworkID'], 'aliases': sorted(n.get('Aliases') or [])}
                           for name, n in row['networks'].items()}
        row['ports'] = {port: sorted(bindings or [], key=lambda binding: (binding['HostIp'], binding['HostPort']))
                        for port, bindings in row['ports'].items()}
        if key=='controller' and DEPLOYMENT76_SELECTION is not None:
            observed, metadata=public_json(D3_CONTROLLER_OBSERVATION.parent,D3_CONTROLLER_OBSERVATION.name)
            require(metadata['sha256']==D3_CONTROLLER_OBSERVATION_SHA
                and row['mounts']==sorted(observed['mounts'],key=lambda m:(m['Destination'],m['Source']))
                and row['user']==observed['user'] and row['restart']==observed['restart']
                and row['port_bindings']==observed['port_bindings']
                and (not row['running'] or row['ports']==observed['port_bindings']),
                'current_controller_projection_changed')
        require(all(binding.get('HostIp') == '127.0.0.1' for bindings in row['ports'].values()
                    for binding in bindings or []), 'non_loopback_service_refused')
        if volume:
            mounts = [m for m in row['mounts'] if m['Type'] == 'volume']
            require(len(mounts) == 1 and mounts[0]['Name'] == volume
                    and mounts[0]['Destination'] == target and mounts[0]['RW'] is True,
                    'volume_binding_refused')
            v = json.loads(self.run(self.docker('volume', 'inspect', '--format',
                '{"name":{{json .Name}},"driver":{{json .Driver}},"mountpoint":{{json .Mountpoint}},'
                '"project":{{if .Labels}}{{json (index .Labels "com.docker.compose.project")}}{{else}}null{{end}},'
                '"volume":{{if .Labels}}{{json (index .Labels "com.docker.compose.volume")}}{{else}}null{{end}}}', volume)))
            require(v['name'] == volume and v['driver'] == 'local'
                    and v['mountpoint'] == mounts[0]['Source'], 'volume_ownership_refused')
            if key != 'honcho_postgres':
                require(v['project'] == 'ortak-private-20260905' and v['volume'] == key + '_data'
                        and row['project'] == v['project'] and row['service'] == key,
                        'compose_ownership_refused')
            else:
                require(HONCHO_NETWORK in row['networks']
                        and HONCHO_HOST in row['networks'][HONCHO_NETWORK]['aliases'],
                        'honcho_database_network_refused')
                v['authority'] = 'exact_retained_container_image_mount_and_root_selection'
            row['volume'] = v
        return row

    def honcho(self, containers):
        """Match saved selection, narrowly parsed live API setting and read-only SQL identity."""
        saved = saved_honcho_selection()
        api, database = containers['honcho_api'], containers['honcho_postgres']
        require(api['running'] and database['running'] and HONCHO_NETWORK in api['networks']
                and api['networks'][HONCHO_NETWORK]['id'] == database['networks'][HONCHO_NETWORK]['id'],
                'honcho_live_binding_refused')
        live = json.loads(self.run(self.docker('exec', '--user', 'app', api['id'],
            '/app/.venv/bin/python', '-c', LIVE_SELECTION), limit=1024))
        require(live == saved, 'honcho_saved_live_mismatch')
        psql = self.docker('exec', '-i', '--user', 'postgres', database['id'],
            'timeout', '-s', 'KILL', '35', 'env', '-i', 'PATH=/usr/local/bin:/usr/bin:/bin',
            'LC_ALL=C', 'PGOPTIONS=-c statement_timeout=30000 -c lock_timeout=2000 -c idle_in_transaction_session_timeout=30000',
            'psql', '--no-psqlrc', '--quiet', '--no-align', '--tuples-only', '--no-password',
            '--set', 'ON_ERROR_STOP=1', '-h', '/var/run/postgresql', '-U', HONCHO_ROLE, '-d', HONCHO_DATABASE)
        sql = ("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n"
            "DO $$BEGIN IF current_database()<>'ortak_honcho_adapter_test' OR current_user<>'ortak_honcho'"
            " OR (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace"
            " WHERE n.nspname='public' AND c.relkind IN ('r','p'))>2048 THEN RAISE EXCEPTION 'scope refused'; END IF; END$$;\n"
            + HONCHO_METADATA + '\nROLLBACK;\n')
        catalog = json.loads(self.run(psql, sql=sql))
        require(catalog['database'] == HONCHO_DATABASE and catalog['role'] == HONCHO_ROLE
                and 'vector' in catalog['extensions'] and catalog['owners'] == [HONCHO_ROLE]
                and re.fullmatch(r'[0-9a-f]{64}', catalog['schema_sha256'])
                and all(type(n) is int and n >= 0 for n in catalog['tables'].values())
                and all('public.' + name in catalog['tables'] for name in
                    ['ortak_resource_receipts', 'ortak_session_ownership', 'ortak_write_receipts']),
                'honcho_catalog_refused')
        obligations.honcho_contract(catalog)
        return {'saved_selection': saved, 'live_api_selection': live, 'catalog': catalog,
                'scope_consistent_read': True, 'cross_store_snapshot': False}

    def main_database(self):
        """Reuse the proven exact main-database read-only metadata guard without a dump."""
        source = self.commands.inspect()
        require(source['container_id'] == SERVICES['postgres'][0], 'main_database_identity_refused')
        metadata = self.commands.metadata('ortak', 'main-database-observation')
        require(metadata['migration_checksums'][-1][0] == MAIN_SCHEMA_VERSION, 'main_schema_review_required')
        obligations.main_contract(metadata)
        return metadata

    def native(self, name):
        """Rediscover exact private cwd and verify the loaded immutable artifact receipt."""
        require(name in NATIVE_WRITERS, 'native_scope_refused')
        pids = self.run(['/usr/bin/pgrep', '-x', name], limit=1024).decode().split()
        require(len(pids) <= 8 and all(p.isdigit() for p in pids), 'native_inventory_refused')
        selected = []
        for pid in pids:
            cwd = self.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'cwd', '-Fn'], limit=4096).decode().splitlines()
            if 'n' + str(STATE) not in cwd:
                continue
            uid = self.run(['/bin/ps', '-p', pid, '-o', 'uid='], limit=128).decode().strip()
            executable = Path(self.run(['/bin/ps', '-p', pid, '-o', 'comm='], limit=4096).decode().strip())
            require(uid == str(os.getuid()) and executable.name == name
                    and executable == NATIVE_BINARIES[name], 'native_artifact_scope_refused')
            directory(executable.parent)
            record, metadata = public_json(executable.parent, 'receipt.json')
            native_artifact_receipt(record, name)
            expected = native_artifact_hash(record, name)
            fd = os.open(executable, os.O_RDONLY | os.O_NOFOLLOW)
            try:
                s = os.fstat(fd)
                require(stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid() and s.st_nlink == 1
                        and stat.S_IMODE(s.st_mode) == 0o500 and s.st_size <= 256 * 1024 * 1024,
                        'native_binary_metadata_refused')
                require(s.st_size == record['binaries'][name]['bytes'], 'native_binary_size_mismatch')
                with os.fdopen(fd, 'rb', closefd=False) as f:
                    digest = hashlib.file_digest(f, 'sha256').hexdigest()
            finally:
                os.close(fd)
            require(digest == expected, 'native_binary_hash_mismatch')
            loaded = self.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'txt', '-Fni'], limit=8192).decode().splitlines()
            found = any(loaded[i] == 'i' + str(s.st_ino) and loaded[i+1] == 'n' + str(executable)
                        for i in range(len(loaded)-1))
            require(found, 'native_loaded_inode_mismatch')
            started = self.run(['/bin/ps', '-p', pid, '-o', 'lstart='], limit=128).decode().strip()
            selected.append({'pid': int(pid), 'uid': int(uid), 'started_at': started, 'cwd': str(STATE),
                             'executable': str(executable), 'inode': s.st_ino, 'bytes': s.st_size,
                             'sha256': digest, 'artifact_receipt': metadata})
        require(len(selected) == 1, 'native_exact_owner_required')
        owner, _ = native_launch_record(name)
        process = selected[0]
        require(owner['pid'] == process['pid'] and owner['binary'] == process['executable']
            and owner['sha256'] == process['sha256']
            and owner['identity'].split() == [str(process['pid']), str(process['uid']), *process['started_at'].split()],
            'native_selected_owner_changed')
        return selected[0]

    def children(self):
        """Observe only this company's labeled worker identities; never recover or stop them."""
        fmt = '{"id":{{json .ID}},"name":{{json .Names}},"key":{{json (.Label "org.ortak.start_key")}}}'
        raw = self.run(self.docker('container', 'ls', '--all', '--no-trunc', '--filter',
            'label=org.ortak.company=' + COMPANY, '--filter', 'label=org.ortak.start_key', '--format', fmt), limit=16384)
        rows = [json.loads(line) for line in raw.decode().splitlines()]
        require(len(rows) <= 8, 'contained_inventory_bound')
        for row in rows:
            require(re.fullmatch('ortak-run:' + COMPANY + r':[0-9a-f-]{36}', row['key'])
                    and row['name'] == 'ortak-run-' + hashlib.sha256(row['key'].encode()).hexdigest(),
                    'contained_identity_refused')
            d = json.loads(self.run(self.docker('inspect', '--format',
                '{"image":{{json .Image}},"running":{{json .State.Running}}}', row['id']), limit=1024))
            require(d['image'] == WORKER_IMAGE, 'contained_image_refused')
            row.update(d)
        return sorted(rows, key=lambda row: row['id'])
