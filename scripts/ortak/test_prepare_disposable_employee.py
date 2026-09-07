"""Real preparation/file/process/HTTP seams with isolated synthetic resources."""
import copy
from contextlib import redirect_stdout
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
from uuid import uuid4

import disposable_employee as contract
import disposable_employee_memory as memory
import prepare_disposable_employee as subject
from test_bootstrap_private_memory import Service as OriginalService


class Service(OriginalService):
    """Reuse independent wire fixture; native identities follow this selected pair."""
    def request(self, method, path, body=None):
        if path.endswith('/resources/inspect'):
            self.calls.append((method, path, copy.deepcopy(body)))
            if self.missing: raise TimeoutError('synthetic missing resource')
            return {**self.resource(), 'company_id': self.create['company_id'],
                    'employee_id': self.create['employee_id'], 'request_hash': memory_hash(self.create),
                    'native_ids': {'workspace': 'replaced' if self.replace_identity else 'native-workspace',
                        'peers': {self.create['user_peer']: 'native-human', self.create['employee_peer']: 'native-employee'}}}
        return super().request(method, path, body)


def memory_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name).resolve()
        self.parent.chmod(0o700)
        self.secret = '1' * 64
        self.public = '2' * 64
        binary = self.parent / 'synthetic-generator'
        binary.write_text('#!/bin/sh\nprintf "Public key: ' + self.public + '\\nSecret key: ' + self.secret + '\\n"\n')
        binary.chmod(0o500)
        self.selection = {'format': contract.FORMAT, 'company_id': str(uuid4()), 'employee_id': 'second-private',
            'output_directory': str(self.parent / 'second'), 'signer_ref': 'secret://second/signer',
            'signer_env': 'ORTAK_SECOND_SIGNER', 'oauth_directory': str(self.parent / 'oauth-second'),
            'worker_image': 'sha256:' + 'a' * 64,
            'key_generator': {'path': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()},
            'runtime_binding': {'adapter': 'hermes', 'profile_ref': 'second-profile', 'workspace_ref': 'none',
                'model': 'gpt-5.6-sol', 'credential_refs': ['secret://second/oauth'], 'options': {'reasoning_effort': 'high'}},
            'memory': {'deployment_id': str(uuid4()), 'origin': 'http://127.0.0.1:8009',
                'token_ref': 'secret://memory/admin', 'token_env': 'ORTAK_HONCHO_SELECTED',
                'binding': {'adapter': 'honcho', 'endpoint_ref': 'service://memory/selected', 'workspace': 'second_memory',
                    'user_peer': 'operator', 'employee_peer': 'second-private', 'options': {}},
                'creation_key': 'memory-second-original', 'validation_run_id': str(uuid4()),
                'validation_recorded_at': '2026-09-06T00:00:00Z'}}
        self.root = Path(self.selection['output_directory'])

    def prepared(self):
        return subject.prepare(self.selection)

    def selection_file(self):
        leaf = self.parent / 'input.json'
        contract.save(leaf, self.selection)
        return leaf

    def test_default_cli_plan_has_no_key_token_output_or_network_effects(self):
        leaf = self.selection_file()
        before = sorted(p.name for p in self.parent.iterdir())
        output = io.StringIO()
        with (patch.object(subject, 'identity', side_effect=AssertionError('key lookup')),
                patch.object(subject, 'Http', side_effect=AssertionError('network')),
                redirect_stdout(output)):
            subject.main(['--selection', str(leaf)])
        value = json.loads(output.getvalue())
        self.assertEqual(value['action'], 'plan')
        self.assertEqual(sorted(p.name for p in self.parent.iterdir()), before)
        self.assertIn('second-profile', value['oauth_enrollment_argv'])
        self.assertFalse(value['oauth_enrolled'] or value['employee_activated'])

    def test_real_bounded_generator_only_publishes_private_signer_and_public_profile(self):
        output = io.StringIO()
        with redirect_stdout(output): subject.main(['--selection', str(self.selection_file()), '--action', 'prepare'])
        self.assertNotIn(self.secret, output.getvalue())
        self.assertEqual(contract.read(self.root / 'signer.json')['secret_key'], self.secret)
        self.assertEqual((self.root / 'signer.json').stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.root / 'profile').stat().st_mode & 0o777, 0o555)
        self.assertEqual({p.name for p in (self.root / 'profile').iterdir()},
                         {'ORTAK_DISPOSABLE_PROFILE.json', 'ORTAK_RUNTIME_BINDING.json', 'ORTAK_PROVIDER.json'})
        for path in (self.root / 'profile').iterdir(): self.assertNotIn(self.secret, path.read_text())
        self.assertFalse(Path(self.selection['oauth_directory']).exists())
        self.assertFalse(contract.read(self.root / 'oauth-enrollment.json')['oauth_enrolled'])

    def test_restart_retains_signer_and_missing_derived_export_without_regeneration(self):
        self.prepared()
        before = (self.root / 'signer.json').read_bytes()
        # Missing derived export is recoverable from the original signer/selection.
        (self.root / 'controller-profile.json').unlink()
        with patch.object(subject, 'identity', side_effect=AssertionError('regenerated signer')):
            self.prepared()
        self.assertEqual((self.root / 'signer.json').read_bytes(), before)
        self.assertTrue((self.root / 'controller-profile.json').exists())

    def test_changed_selection_refuses_before_keys_or_memory_credentials(self):
        self.prepared()
        original = (self.root / 'signer.json').read_bytes()
        self.selection['runtime_binding']['model'] = 'gpt-6-astra'
        with (patch.object(subject, 'identity', side_effect=AssertionError('key lookup')),
                self.assertRaisesRegex(contract.Refused, 'selection_changed')): self.prepared()
        with self.assertRaisesRegex(contract.Refused, 'selection_changed'):
            memory.prepare_memory(self.selection, lambda: self.fail('credential lookup'))
        self.assertEqual((self.root / 'signer.json').read_bytes(), original)

    def test_unmarked_root_existing_oauth_wrong_digest_and_links_refuse(self):
        self.root.mkdir(mode=0o700); (self.root / 'old-data').write_text('preserved')
        with self.assertRaisesRegex(contract.Refused, 'unmarked'): self.prepared()
        self.assertEqual({p.name for p in self.root.iterdir()}, {'old-data'})
        (self.root / 'old-data').unlink()
        self.selection['key_generator']['sha256'] = 'b' * 64
        with self.assertRaisesRegex(contract.Refused, 'digest_changed'): self.prepared()
        # A new operation cannot relabel an existing OAuth directory.
        other = copy.deepcopy(self.selection); other['output_directory'] = str(self.parent / 'third')
        Path(other['oauth_directory']).mkdir(mode=0o700)
        with self.assertRaisesRegex(contract.Refused, 'fresh_oauth'): subject.prepare(other)
        leaf = self.parent / 'linked'; contract.save(leaf, {'x': 1})
        os.link(leaf, self.parent / 'alias')
        with self.assertRaisesRegex(contract.Refused, 'leaf_changed'): contract.read(leaf)

    def test_separate_second_and_third_intents_never_replace_each_other(self):
        self.prepared(); before = (self.root / 'selection.json').read_bytes()
        third = copy.deepcopy(self.selection)
        third.update(employee_id='third-private', output_directory=str(self.parent / 'third'),
                     oauth_directory=str(self.parent / 'oauth-third'), signer_ref='secret://third/signer', signer_env='ORTAK_THIRD_SIGNER')
        third['runtime_binding'].update(profile_ref='third-profile', credential_refs=['secret://third/oauth'])
        third['memory']['binding'].update(workspace='third_memory', employee_peer='third-private')
        third['memory'].update(creation_key='memory-third-original', validation_run_id=str(uuid4()))
        subject.prepare(third)
        self.assertEqual((self.root / 'selection.json').read_bytes(), before)
        self.assertEqual(contract.read(Path(third['output_directory']) / 'controller-profile.json')['employee_id'], 'third-private')

    def test_full_memory_receipts_and_completed_export_are_same_identity(self):
        self.prepared(); service = Service(self.root)
        result = memory.prepare_memory(self.selection, lambda: service)
        self.assertEqual(result['roundtrip'], 'verified_now')
        prepared = contract.read(self.root / 'memory/prepared-memory.json')
        worker = contract.read(self.root / 'memory/worker-memory-prepared.json')
        self.assertEqual(prepared['creation_receipt'], worker['employees'][0]['creation_receipt'])
        self.assertEqual(prepared['creation_receipt']['employee_id'], 'second-private')
        service.calls.clear()
        (self.root / 'memory/prepared-memory.json').unlink()
        result = memory.prepare_memory(self.selection, lambda: service, export_only=True)
        self.assertEqual(result['roundtrip'], 'previously_verified')
        self.assertEqual(len(service.calls), 2)
        self.assertFalse(any(path.endswith(('/create', '/remember')) for _, path, _ in service.calls))

    def test_lost_create_and_write_ack_reuse_original_durable_intent(self):
        self.prepared(); service = Service(self.root); service.lose_create_reply = True
        with self.assertRaises(TimeoutError): memory.prepare_memory(self.selection, lambda: service)
        state = contract.read(self.root / 'memory/bootstrap.json')
        service.lose_write_reply = True
        with self.assertRaises(TimeoutError): memory.prepare_memory(self.selection, lambda: service)
        memory.prepare_memory(self.selection, lambda: service)
        self.assertEqual(contract.read(self.root / 'memory/bootstrap.json')['intent'], state['intent'])
        self.assertEqual(len(service.writes), 1)
        self.assertEqual(sum(path.endswith('/create') for _, path, _ in service.calls), 2)

    def test_incomplete_export_and_changed_receipt_fail_before_token_lookup(self):
        self.prepared()
        with self.assertRaisesRegex(contract.Refused, 'completed_memory'):
            memory.prepare_memory(self.selection, lambda: self.fail('credential lookup'), export_only=True)
        service = Service(self.root); memory.prepare_memory(self.selection, lambda: service)
        leaf = self.root / 'memory/bootstrap.json'; state = contract.read(leaf)
        state['resource_identity']['employee_id'] = 'different'
        contract.save(leaf, state)
        with self.assertRaises(contract.Refused): memory.prepare_memory(self.selection, lambda: self.fail('credential lookup'))

    def test_missing_or_replaced_native_memory_never_recreates_or_claims_success(self):
        self.prepared(); service = Service(self.root); memory.prepare_memory(self.selection, lambda: service)
        before = (self.root / 'memory/prepared-memory.json').read_bytes()
        service.calls.clear(); service.replace_identity = True
        with self.assertRaisesRegex(contract.Refused, 'native_memory_identity_changed'):
            memory.prepare_memory(self.selection, lambda: service)
        self.assertEqual((self.root / 'memory/prepared-memory.json').read_bytes(), before)
        self.assertFalse(any(path.endswith(('/create', '/remember')) for _, path, _ in service.calls))

    def test_real_loopback_http_uses_selected_bearer_and_never_redirects(self):
        seen = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self):
                seen.append((self.path, self.headers.get('Authorization')))
                self.send_response(302 if self.path == '/redirect' else 200)
                self.send_header('Content-Length', '2'); self.end_headers(); self.wfile.write(b'{}')
        server = HTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever); thread.start()
        try:
            http = memory.Http(f'http://127.0.0.1:{server.server_port}', 'synthetic-memory-token')
            self.assertEqual(http.request('GET', '/inspect'), {})
            with self.assertRaisesRegex(contract.Refused, 'service_refused'): http.request('GET', '/redirect')
            self.assertEqual(seen, [('/inspect', 'Bearer synthetic-memory-token'), ('/redirect', 'Bearer synthetic-memory-token')])
        finally:
            server.shutdown(); server.server_close(); thread.join(2)

    def crash_prepare(self, selection, target, phase='before'):
        leaf = self.parent / 'crash-selection.json'; contract.save(leaf, selection)
        code = """
import os,sys
from pathlib import Path
import prepare_disposable_employee as subject
original=os.replace
def replace(source,target):
    if Path(target).name==sys.argv[2] and sys.argv[3]=='before': os._exit(81)
    original(source,target)
    if Path(target).name==sys.argv[2] and sys.argv[3]=='after': os._exit(81)
os.replace=replace
subject.main(['--selection',sys.argv[1],'--action','prepare'])
"""
        result = subprocess.run([sys.executable, '-c', code, str(leaf), target, phase],
            env={'PATH': '/usr/bin:/bin', 'PYTHONPATH': str(Path(__file__).parent.resolve())},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=8)
        self.assertEqual(result.returncode, 81, result.stderr[:512])
        self.assertEqual(result.stdout, b'')
        self.assertNotIn(self.secret.encode(), result.stderr)

    def test_actual_process_death_at_selection_signer_and_profile_checkpoint_recovers(self):
        for target in ('selection.json', 'signer.json', 'ORTAK_PROVIDER.json', 'oauth-enrollment.json'):
            with self.subTest(target=target):
                selection = copy.deepcopy(self.selection)
                selection['output_directory'] = str(self.parent / target.replace('.', '-'))
                root = Path(selection['output_directory'])
                self.crash_prepare(selection, target)
                staged = (root / 'profile' if target.startswith('ORTAK_') else root) / ('.pending-' + target)
                self.assertTrue(staged.is_file())
                if target == 'selection.json':
                    self.assertFalse((root / 'selection.json').exists())
                    subject.prepare(selection)
                else:
                    with patch.object(subject, 'identity', side_effect=AssertionError('regenerated signer')):
                        subject.prepare(selection)
                self.assertFalse(staged.exists())
                self.assertEqual(contract.read(root / 'signer.json')['secret_key'], self.secret)
                self.assertEqual(contract.read(root / 'controller-profile.json')['binding']['workspace_ref'], 'none')
                self.assertEqual({p.name for p in (root / 'profile').iterdir()},
                    {'ORTAK_DISPOSABLE_PROFILE.json', 'ORTAK_RUNTIME_BINDING.json', 'ORTAK_PROVIDER.json'})

    def test_actual_process_death_after_profile_rename_replays_without_new_signer(self):
        self.crash_prepare(self.selection, 'ORTAK_PROVIDER.json', 'after')
        before = (self.root / 'signer.json').read_bytes()
        with patch.object(subject, 'identity', side_effect=AssertionError('regenerated signer')): self.prepared()
        self.assertEqual((self.root / 'signer.json').read_bytes(), before)
        self.assertEqual((self.root / 'profile').stat().st_mode & 0o777, 0o555)

    def test_changed_partial_or_unowned_checkpoint_is_retained_before_side_effects(self):
        self.root.mkdir(mode=0o700)
        stage = contract.pending(self.root / 'selection.json')
        changed = copy.deepcopy(self.selection); changed['employee_id'] = 'other-private'
        contract.save(stage, changed)
        with patch.object(subject, 'identity', side_effect=AssertionError('key lookup')):
            with self.assertRaisesRegex(contract.Refused, 'selection_changed'): self.prepared()
        self.assertEqual({p.name for p in self.root.iterdir()}, {stage.name})
        self.assertEqual(contract.read(stage), changed)
        stage.write_text('{"incomplete":'); stage.chmod(0o600)
        with patch.object(subject, 'identity', side_effect=AssertionError('key lookup')):
            with self.assertRaises(ValueError): self.prepared()
        self.assertEqual(stage.read_text(), '{"incomplete":')
        stage.unlink(); contract.save(stage, self.selection)
        os.link(stage, self.parent / 'checkpoint-alias')
        with self.assertRaisesRegex(contract.Refused, 'leaf_changed'): self.prepared()
        self.assertEqual(stage.stat().st_nlink, 2)

    def test_unknown_or_changed_profile_checkpoint_never_gets_deleted(self):
        self.crash_prepare(self.selection, 'ORTAK_PROVIDER.json')
        stage = self.root / 'profile/.pending-ORTAK_PROVIDER.json'
        stage.chmod(0o600); stage.write_text(contract.canonical({'provider': 'changed'}) + '\n')
        with self.assertRaisesRegex(contract.Refused, 'pending_leaf_changed'): self.prepared()
        self.assertTrue(stage.exists())
        unknown = stage.with_name('.pending-unrecognized'); stage.rename(unknown)
        with self.assertRaisesRegex(contract.Refused, 'unexpected_profile_file'): self.prepared()
        self.assertEqual(contract.read(unknown), {'provider': 'changed'})

    def test_memory_completed_checkpoint_recovers_before_credentials_without_new_write(self):
        self.prepared(); service = Service(self.root)
        replace = os.replace
        def interrupted(source, target):
            if Path(target).name == 'bootstrap.json' and contract.read(source, staged=True)['completed']:
                raise OSError('controlled interrupted publish')
            replace(source, target)
        with patch.object(contract.os, 'replace', side_effect=interrupted), self.assertRaises(OSError):
            memory.prepare_memory(self.selection, lambda: service)
        target = self.root / 'memory/bootstrap.json'; staged = contract.pending(target)
        self.assertFalse(contract.read(target)['completed'])
        self.assertTrue(contract.read(staged, staged=True)['completed'])
        service.calls.clear()
        result = memory.prepare_memory(self.selection, lambda: service, export_only=True)
        self.assertEqual(result['roundtrip'], 'previously_verified')
        self.assertEqual(len(service.calls), 2)
        self.assertEqual(len(service.writes), 1)
        self.assertFalse(staged.exists())
        bad = contract.read(target); bad['resource_identity']['employee_id'] = 'changed'
        contract.save(staged, bad)
        with self.assertRaises(contract.Refused):
            memory.prepare_memory(self.selection, lambda: self.fail('credential lookup'))
        self.assertTrue(staged.exists())

    def test_invalid_public_selection_rejects_without_output_tree(self):
        for change in ('extra', 'remote', 'workspace', 'null_workspace', 'overlap', 'credential', 'nil'):
            value = copy.deepcopy(self.selection)
            if change == 'extra': value['oauth_token'] = 'never-read'
            elif change == 'remote': value['memory']['origin'] = 'http://example.com:8009'
            elif change == 'workspace': value['runtime_binding']['workspace_ref'] = 'unprepared'
            elif change == 'null_workspace': value['runtime_binding']['workspace_ref'] = None
            elif change == 'overlap': value['oauth_directory'] = value['output_directory']
            elif change == 'credential': value['signer_ref'] = value['runtime_binding']['credential_refs'][0]
            else: value['company_id'] = '00000000-0000-0000-0000-000000000000'
            with self.subTest(change=change), self.assertRaises((contract.Refused, ValueError)): subject.plan(value)
        self.assertFalse(self.root.exists())

    def shared_selection(self):
        self.selection['oauth_owner'] = {'format': 'ortak-oauth-identity/1',
            'company_id': self.selection['company_id'], 'employee_id': 'original-owner',
            'profile_ref': 'original-profile',
            'credential_ref': self.selection['runtime_binding']['credential_refs'][0]}
        return self.selection

    def test_shared_connection_prepares_only_public_grant_and_never_opens_store(self):
        selection = self.shared_selection()
        oauth = Path(selection['oauth_directory']); oauth.mkdir(mode=0o700)
        marker = oauth / 'private-fixture'; marker.write_text('must remain unread and unchanged')
        before = marker.read_bytes()
        original_open = os.open
        def checked_open(path, *args, **kwargs):
            self.assertFalse(str(path).startswith(str(oauth) + '/'), 'helper opened shared OAuth contents')
            return original_open(path, *args, **kwargs)
        with patch.object(contract.os, 'open', side_effect=checked_open):
            result = subject.plan(selection)
            self.assertIsNone(result['oauth_enrollment_argv'])
            self.assertFalse(result['shared_connection']['ownership_verified'])
            self.assertNotIn('root_interactive_oauth_enrollment', result['next_actions'])
            self.prepared()
        self.assertEqual(marker.read_bytes(), before)
        self.assertEqual(contract.read(self.root / 'controller-profile.json')['oauth_owner'], selection['oauth_owner'])
        receipt = contract.read(self.root / 'oauth-connection.json')
        self.assertFalse(receipt['ownership_verified'] or receipt['oauth_enrolled'] or receipt['employee_activated'])
        self.assertFalse((self.root / 'oauth-enrollment.json').exists())
        self.assertNotIn('oauth_owner', contract.read(self.root / 'profile/ORTAK_RUNTIME_BINDING.json', public=True))

    def test_shared_connection_checkpoint_retries_exact_grant_without_signer_regeneration(self):
        self.shared_selection()
        self.crash_prepare(self.selection, 'oauth-connection.json')
        original = (self.root / 'signer.json').read_bytes()
        with patch.object(subject, 'identity', side_effect=AssertionError('regenerated signer')):
            self.prepared()
        self.assertEqual((self.root / 'signer.json').read_bytes(), original)
        self.selection['oauth_owner']['profile_ref'] = 'different-owner-profile'
        with self.assertRaisesRegex(contract.Refused, 'selection_changed'):
            self.prepared()
        self.assertEqual((self.root / 'signer.json').read_bytes(), original)

    def test_shared_connection_malformed_company_reference_and_identity_are_rejected(self):
        original = copy.deepcopy(self.shared_selection())
        for field, value in [('company_id', str(uuid4())), ('credential_ref', 'secret://other/oauth'),
                             ('employee_id', self.selection['employee_id']), ('format', 'unknown'),
                             ('profile_ref', None), ('extra', True)]:
            selected = copy.deepcopy(original); selected['oauth_owner'][field] = value
            with self.subTest(field=field), self.assertRaises(contract.Refused):
                subject.plan(selected)
        selected = copy.deepcopy(original); selected['oauth_owner'] = None
        with self.assertRaises(contract.Refused): subject.plan(selected)
        self.assertFalse(self.root.exists())

    def test_uri_profile_references_preserve_shared_oauth_ownership(self):
        self.shared_selection()
        self.selection['runtime_binding']['profile_ref'] = 'profile://private/second'
        self.selection['oauth_owner']['profile_ref'] = 'profile://private/original'
        self.prepared()
        profile = contract.read(self.root / 'controller-profile.json')
        self.assertEqual(profile['binding']['profile_ref'], 'profile://private/second')
        self.assertEqual(profile['oauth_owner'], self.selection['oauth_owner'])
        for invalid in ('bad profile', 'x' * 257, 'profile://private/\noriginal'):
            self.selection['oauth_owner']['profile_ref'] = invalid
            with self.assertRaises(contract.Refused):
                subject.plan(self.selection)


if __name__ == '__main__': unittest.main()
