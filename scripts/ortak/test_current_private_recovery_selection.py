"""Selected76 worker/native receipt chain with synthetic identities; no live stack access."""

import copy
import os
import unittest
from unittest.mock import patch

import private_recovery_inventory as inventory
import prepare_private_recovery as prepare
import register_private_recovery as register
from test_prepare_private_recovery import fixture_hashes, runtime_fixture


def records():
    values = runtime_fixture()
    return {path: values[str(path)] for path in (inventory.WORKER_OWNERS, inventory.CURRENT_OWNERS)}


class CurrentSelectionTests(unittest.TestCase):
    def setUp(self):
        for name in ('DEPLOYMENT76_SELECTION','SCORER_SELECTION'):
            context=patch.object(inventory,name,None);context.start();self.addCleanup(context.stop)

    def test_normalization_selects_reply_worker_from_mentions_registry_and_preserves_sessions(self):
        selected = records()
        def read(root, name):
            return selected[root / name], {'path': str(root / name), 'sha256': fixture_hashes()[str(root / name)]}
        with patch.object(inventory, 'public_json', side_effect=read):
            rows = {name: inventory.native_launch_record(name)[0] for name in inventory.NATIVE_WRITERS}
            self.assertEqual(rows['ortak-worker']['pid'], 200)
            self.assertEqual(rows['ortak-worker']['session'], 201)
            self.assertEqual(rows['buzz-relay']['pid'], 100)
            self.assertEqual(rows['ortak-management']['pid'], 103)
            observation = {'native_processes': {name: {'pid': row['pid'], 'uid': os.getuid(),
                'started_at': 'Sun Sep 6 03:53:10 2026', 'executable': row['binary'],
                'sha256': row['sha256']} for name, row in rows.items()}}
            sessions = register.sessions(observation)
            self.assertEqual(sessions['ortak-worker']['session_id'], 201)
            observation['native_processes']['ortak-worker']['pid'] = 102
            with self.assertRaisesRegex(inventory.Refused, 'session_process_receipt_mismatch'):
                register.sessions(observation)

    def test_old_worker_unknown_owner_and_changed_launcher_are_not_current_authority(self):
        for case in ['old_worker', 'wrong_uid', 'cwd', 'old_artifact', 'identity', 'hash', 'bool_pid', 'unknown_owner']:
            selected = records(); row = selected[inventory.NATIVE_RECEIPTS['ortak-worker']]['ortak-worker']
            if case == 'old_worker': row['launcher'] = str(inventory.BACKEND_ROLLOUT / 'final-launchers/launch-ortak-worker.py')
            elif case == 'wrong_uid': row['uid'] = os.getuid() + 1
            elif case == 'cwd': row['cwd'] = '/unrelated'
            elif case == 'old_artifact': row['executable'] = '/unrelated/ortak-worker'
            elif case == 'identity': row['identity'] = 'old generation'
            elif case == 'hash': row['sha256'] = 'invalid'
            elif case == 'bool_pid': row['pid'] = True
            else: selected[inventory.NATIVE_RECEIPTS['buzz-relay']]['unrelated'] = {}
            def read(root, name):
                return selected[root / name], {'path': str(root / name), 'sha256': fixture_hashes()[str(root / name)]}
            with self.subTest(case=case), patch.object(inventory, 'public_json', side_effect=read):
                with self.assertRaises(inventory.Refused):
                    inventory.native_launch_record('buzz-relay' if case == 'unknown_owner' else 'ortak-worker')

    def test_intermediate_worker_receipt_hash_cannot_authorize_current_native_registry(self):
        selected = records()
        def read(root, name):
            return selected[root / name], {'path': str(root / name), 'sha256': inventory.WORKER_OWNERS_SHA}
        with patch.object(inventory, 'public_json', side_effect=read):
            with self.assertRaisesRegex(inventory.Refused, 'native_launch_receipt_refused'):
                inventory.native_launch_record('ortak-worker')

    def test_owner_replacement_chain_requires_both_proofs_and_exact_unchanged_owners(self):
        self.assertEqual(prepare.deployment_bindings(runtime_fixture())['schema'], inventory.MAIN_SCHEMA_VERSION)
        for case in ('worker_repointed', 'worker_hash', 'worker_row', 'worker_other_owner',
                     'native_repointed', 'native_hash', 'native_row', 'native_backend',
                     'previous_binary', 'native_build', 'native_launcher', 'native_unchanged_claim'):
            values = runtime_fixture()
            worker = values[str(inventory.WORKER_ROLLOUT/'deployed.json')]
            native = values[str(inventory.NATIVE_ROLLOUT/'deployed.json')]
            built = values[str(prepare.native_ingress.BUILD_RECEIPT)]
            if case == 'worker_repointed': worker['current_owners'] = str(inventory.CURRENT_OWNERS)
            elif case == 'worker_hash': worker['current_owners_sha256'] = inventory.CURRENT_OWNERS_SHA
            elif case == 'worker_row': worker['worker']['pid'] += 1
            elif case == 'worker_other_owner': values[str(inventory.CURRENT_ROLLOUT/'current-owners76.json')]['buzz-relay']['pid'] += 1
            elif case == 'native_repointed': native['current_owners'] = str(inventory.WORKER_OWNERS)
            elif case == 'native_hash': native['current_owners_sha256'] = inventory.WORKER_OWNERS_SHA
            elif case == 'native_row': native['native']['pid'] += 1
            elif case == 'native_backend': values[str(inventory.CURRENT_OWNERS)]['ortak-worker']['pid'] += 1
            elif case == 'previous_binary': built['previous_sha256'] = 'f' * 64
            elif case == 'native_build': native['build_receipt'] = '/unrelated/receipt.json'
            elif case == 'native_launcher': built['launcher_sha256'] = 'f' * 64
            else: native['four_backend_owners_unchanged'] = False
            expected = 'current_worker_selection_refused' if case.startswith('worker_') else 'current_native_launcher_refused'
            with self.subTest(case=case), self.assertRaisesRegex(inventory.Refused, expected):
                prepare.deployment_bindings(values)

    def test_config_and_both_owner_hashes_refuse_before_any_secret_metadata_access(self):
        for target in (inventory.WORKER_CONFIG, inventory.WORKER_OWNERS, inventory.CURRENT_OWNERS):
            values = runtime_fixture(); hashes = fixture_hashes(); hashes[str(target)] = 'unreviewed'
            def read(root, name):
                path = str(root / name)
                return copy.deepcopy(values[path]), {'path': path, 'sha256': hashes.get(path, 'a' * 64)}
            with self.subTest(target=target), patch.object(inventory, 'public_json', side_effect=read), \
                    patch.object(inventory, 'file_metadata', side_effect=AssertionError('secret metadata access')):
                with self.assertRaisesRegex(inventory.Refused, 'current_service_configuration_changed'):
                    prepare.files()


if __name__ == '__main__':
    unittest.main()
