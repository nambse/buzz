"""Named-volume gates bind the real CLI, executor mount and pre-Journal seams."""
import copy
import json
from pathlib import Path
import re
from unittest.mock import Mock, patch
import unittest
from uuid import uuid4

from ortak_hermes_bridge.__main__ import configured_bridge, configured_journal
from ortak_hermes_bridge.docker_executor import DockerExecutor
from ortak_hermes_bridge.journal import BridgeError
from ortak_hermes_bridge import journal_volume
import test_docker_executor as fixtures

IMAGE = fixtures.IMAGE

CONTAINER = 'abcdef012345' + '0' * 52


def render_volume_projection(template, volume):
    """Substitute Docker values while preserving every production JSON delimiter."""
    values = {'.Name': volume['name'], '.CreatedAt': volume['created_at'],
        '.Driver': volume['driver'], '.Scope': volume['scope'],
        '.Options': volume['options'], '.Mountpoint': volume['source'],
        '(index .Labels "org.ortak.company")': volume['company'],
        '(index .Labels "org.ortak.journal_owner")': volume['owner']}
    rendered = re.sub(r'\{\{json (.*?)\}\}',
                      lambda match: json.dumps(values[match.group(1)]), template)
    if '{{' in rendered or '}}' in rendered:
        raise AssertionError('unsupported Docker template action')
    return rendered


class VolumeEngine(fixtures.Engine):
    def __init__(self, company, parent, chosen):
        super().__init__()
        self.inspections = []
        self.volume_format_mutation = lambda value: value
        source = '/var/lib/docker/volumes/' + chosen['name'] + '/_data'
        self.volume = dict(chosen, driver='local', scope='local', options=None, source=source,
                           company=company, owner=chosen['owner_id'])
        self.volume.pop('owner_id')
        self.controller = {'id': CONTAINER, 'hostname': CONTAINER[:12], 'running': True, 'pid': 42,
            'company': company, 'owner': chosen['owner_id'], 'mounts': [{'type': 'volume',
            'name': chosen['name'], 'source': source, 'destination': str(parent), 'rw': True}]}

    def command(self, args):
        self.inspections.append(args)
        if args[0] == 'volume':
            template = self.volume_format_mutation(args[args.index('--format') + 1])
            raw = render_volume_projection(template, self.volume)
        else:
            raw = json.dumps(self.controller)
        if len(raw.encode()) > 1024:
            raise AssertionError('fixture escaped production output bound')
        return 0, raw


class VolumeTests(unittest.TestCase):
    # Reuse setup only; existing containment tests independently retain the
    # legacy engine that implements no volume inspection.
    def setUp(self):
        fixtures.Containment.setUp(self)
        self.selected = {'name': 'ortak-journal-fixture-' + uuid4().hex,
                         'created_at': '2026-09-06T04:00:00Z', 'owner_id': str(uuid4())}
        self.engine = VolumeEngine(self.company, self.root / 'state', self.selected)
        self.hostname = patch('ortak_hermes_bridge.journal_volume.socket.gethostname', return_value=CONTAINER[:12])
        self.hostname.start(); self.addCleanup(self.hostname.stop)

    def selected_executor(self):
        executor = DockerExecutor(self.journal, self.company, [self.profile], IMAGE,
            'ortak-private-test', self.engine, validated_digest=IMAGE, journal_volume=self.selected)
        self.addCleanup(executor.close)
        return executor

    def test_volume_launch_uses_same_store_and_no_bind_fallback(self):
        executor = self.selected_executor()
        self.journal.reserve(self.spec)
        executor.start(self.spec, self.journal)
        _, argv, payload = self.engine.calls[-1]
        mounts = [argv[i+1] for i, arg in enumerate(argv) if arg == '--mount']
        self.assertEqual(mounts, [f'type=bind,src={self.profile_dir},dst=/profile,readonly',
            f"type=volume,src={self.selected['name']},dst=/ortak-state,volume-nocopy"])
        self.assertEqual(json.loads(payload)['spec'], self.spec)
        self.assertEqual(len(self.engine.inspections), 4)  # Before ownership and launch.
        self.assertEqual(argv[-1], '/ortak-state/journal.sqlite')

    def test_actual_volume_projection_renders_complete_json_before_journal(self):
        config = {'company_id': self.company, 'executor': {'journal_volume': self.selected}}
        with patch('ortak_hermes_bridge.docker_executor.DockerEngine', return_value=self.engine), \
             patch('ortak_hermes_bridge.__main__.Journal') as constructor:
            configured_journal(config, self.journal.path, True)
            constructor.assert_called_once_with(self.journal.path)
        command = self.engine.inspections[0]
        template = command[command.index('--format') + 1]
        rendered = render_volume_projection(template, self.engine.volume)
        self.assertEqual(json.loads(rendered), self.engine.volume)

        # Reproduce the real startup defect: remove only the outer JSON brace,
        # leaving the final Go template action intact. Admission must fail
        # before Journal can open or initialize the selected SQLite store.
        self.assertTrue(template.endswith('}}}'))
        self.engine.volume_format_mutation = lambda value: value[:-1]
        self.engine.inspections.clear()
        with patch('ortak_hermes_bridge.docker_executor.DockerEngine', return_value=self.engine), \
             patch('ortak_hermes_bridge.__main__.Journal') as constructor:
            with self.assertRaisesRegex(BridgeError, 'journal_volume_ownership_required'):
                configured_journal(config, self.journal.path, True)
            constructor.assert_not_called()
        self.assertEqual(len(self.engine.inspections), 1)
        self.assertFalse((self.root / 'state' / 'executor.lock').exists())
        self.assertEqual(self.engine.calls, [])

    def test_each_wrong_volume_generation_or_owner_refuses_before_lock(self):
        original = copy.deepcopy(self.engine.volume)
        for name, value in [('name', 'another'), ('created_at', '2026-09-06T04:00:01Z'),
            ('driver', 'remote'), ('scope', 'global'), ('options', {'device': '/old'}),
            ('company', str(uuid4())), ('owner', str(uuid4()))]:
            with self.subTest(name=name):
                self.engine.volume = {**original, name: value}
                with self.assertRaisesRegex(BridgeError, 'journal_volume_ownership_required'):
                    self.selected_executor()
                self.assertFalse((self.root / 'state' / 'executor.lock').exists())
                self.assertEqual(self.engine.calls, [])

    def test_controller_identity_and_mount_mismatches_refuse(self):
        original = copy.deepcopy(self.engine.controller)
        variants = [{**original, key: value} for key, value in [('id', 'b' * 64),
            ('hostname', 'alias'), ('running', False), ('pid', 0), ('owner', str(uuid4())),
            ('company', str(uuid4())), ('mounts', [])]]
        for key, value in [('type', 'bind'), ('name', 'another'), ('source', '/old'),
                           ('destination', '/other'), ('rw', False)]:
            variants.append({**original, 'mounts': [{**original['mounts'][0], key: value}]})
        variants.append({**original, 'mounts': original['mounts'] + [
            {**original['mounts'][0], 'destination': str(self.root / 'state' / 'journal.sqlite')}]})
        for value in variants:
            self.engine.controller = value
            with self.assertRaisesRegex(BridgeError, 'journal_volume_ownership_required'):
                self.selected_executor()
        self.assertEqual(self.engine.calls, [])

    def test_generation_change_at_launch_refuses_before_profile_or_credentials(self):
        executor = self.selected_executor()
        self.journal.reserve(self.spec)
        self.engine.volume['created_at'] = '2026-09-06T04:00:01Z'
        with patch.object(executor, 'validate_profile', side_effect=AssertionError('profile accessed')):
            with self.assertRaisesRegex(BridgeError, 'journal_volume_ownership_required'):
                executor.start(self.spec, self.journal)
        self.assertEqual(self.engine.calls, [])

    def test_invalid_config_and_alias_never_inspect(self):
        variants = [False, {}, dict(self.selected, extra=True), dict(self.selected, name='../old'),
                    dict(self.selected, owner_id='0'*32), dict(self.selected, created_at='yesterday')]
        for value in variants:
            with self.assertRaises(BridgeError):
                journal_volume.mount(self.engine, value, self.company, self.journal.path)
        with patch('ortak_hermes_bridge.journal_volume.socket.gethostname', return_value='custom-controller'):
            with self.assertRaises(BridgeError):
                journal_volume.mount(self.engine, self.selected, self.company, self.journal.path)
        self.assertEqual(self.engine.inspections, [])

    def test_unknown_daemon_response_is_never_success_or_fallback(self):
        for reply in [(1, ''), (0, 'not-json'), (0, '[]')]:
            with patch.object(self.engine, 'command', return_value=reply):
                with self.assertRaises(BridgeError): self.selected_executor()
        self.assertEqual(self.engine.calls, [])

    def test_cli_passes_selection_and_verifies_before_journal_creation(self):
        config = {'company_id': self.company, 'profiles': [self.profile], 'executor': {
            'image': IMAGE, 'validated_digest': IMAGE, 'network': 'ortak-private-test',
            'journal_volume': self.selected}}
        with patch('ortak_hermes_bridge.docker_executor.DockerExecutor') as constructor:
            configured_bridge(config, self.journal, True)
            self.assertEqual(constructor.call_args.kwargs['journal_volume'], self.selected)
        with patch('ortak_hermes_bridge.docker_executor.DockerEngine', return_value=self.engine), \
             patch('ortak_hermes_bridge.__main__.Journal') as constructor:
            configured_journal(config, self.journal.path, True)
            constructor.assert_called_once_with(self.journal.path)
        self.engine.controller['mounts'][0]['type'] = 'bind'
        with patch('ortak_hermes_bridge.docker_executor.DockerEngine', return_value=self.engine), \
             patch('ortak_hermes_bridge.__main__.Journal') as constructor:
            with self.assertRaises(BridgeError): configured_journal(config, self.journal.path, True)
            constructor.assert_not_called()

    def test_named_volume_needs_explicit_docker_opt_in_before_journal(self):
        config = {'company_id': self.company, 'executor': {'journal_volume': self.selected}}
        with patch('ortak_hermes_bridge.__main__.Journal') as constructor:
            with self.assertRaisesRegex(BridgeError, 'executor_validation_required'):
                configured_journal(config, self.journal.path)
            constructor.assert_not_called()

    def test_legacy_journal_does_not_query_docker(self):
        with patch('ortak_hermes_bridge.docker_executor.DockerEngine', side_effect=AssertionError('unexpected daemon')):
            journal = configured_journal({}, self.journal.path)
        self.assertEqual(journal.path, self.journal.path)

    def test_inspect_template_projects_only_selected_or_nested_mounts(self):
        value = journal_volume.controller_format('/private/selected/state')
        self.assertNotIn('{{json .Config}}', value)
        self.assertNotIn('{{json .Mounts}}', value)
        self.assertIn('(eq .Destination "/private/selected/state")', value)
        self.assertIn('(slice .Destination 0 24)', value)
        self.assertIn('"/private/selected/state/"', value)


if __name__ == '__main__':
    unittest.main()
