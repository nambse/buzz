"""Provider-free regressions bind the synthetic launcher to production transport."""
import unittest
from email.message import Message
from unittest.mock import Mock, patch
from checks.synthetic_http_check import FixtureEngine, remove_owned, http
from checks.synthetic_provider import ANSWER, Handler, TOKEN, events, response
from ortak_hermes_bridge.docker_executor import DockerEngine


class SyntheticContract(unittest.TestCase):
    def test_redirect_preserves_every_container_argument_and_stdin(self):
        args = ['run', '--read-only', '--user', '10001:10001', '--network', 'fixture-private',
                'sha256:' + 'a' * 64, '-m', 'ortak_hermes_bridge.worker',
                '--journal', '/ortak-state/journal.sqlite']
        original = args.copy()
        payload = b'fixed input not in arguments'
        engine = FixtureEngine('/usr/bin/docker', 'ortak-synthetic-' + 'a' * 32 + '.api.openai.com.invalid')
        with patch.object(DockerEngine, 'launch') as launch:
            engine.launch(args, payload)
        forwarded, forwarded_payload = launch.call_args.args
        self.assertEqual(args, original)
        self.assertEqual(forwarded[:7], args[:7])
        self.assertEqual(forwarded[-2:], args[-2:])
        self.assertEqual(forwarded[7:9], ['-c', engine.launcher])
        self.assertIs(forwarded_payload, payload)
        compile(engine.launcher, '<synthetic-launcher>', 'exec')
        self.assertIn('base = original()', engine.launcher)
        self.assertIn('worker.main()', engine.launcher)
        self.assertNotIn('_interruptible_api_call', engine.launcher)

    def test_only_fresh_qualified_invalid_endpoint_is_accepted(self):
        for host in ('api.openai.com', 'localhost', 'ortak-synthetic-' + 'a' * 32,
                     'ortak-synthetic-' + 'g' * 32 + '.invalid'):
            with self.subTest(host=host), self.assertRaises(ValueError):
                FixtureEngine('/usr/bin/docker', host)

    def test_response_and_stream_complete_same_normal_or_tool_object(self):
        for tool in (False, True):
            value = response(tool)
            stream = list(events(value))
            self.assertEqual(stream[-1], {'type': 'response.completed', 'response': value})
            self.assertEqual(stream[-2]['item'], value['output'][0])
            self.assertEqual(value['usage']['total_tokens'], 2)
            if tool:
                self.assertEqual(value['output'][0]['name'], 'terminal')
                self.assertFalse(any(e['type'] == 'response.output_text.delta' for e in stream))
            else:
                self.assertEqual(next(e['delta'] for e in stream if e['type'] == 'response.output_text.delta'), ANSWER)

    def test_http_transport_uses_bounded_loopback_connection(self):
        with patch('checks.synthetic_http_check.HTTPConnection') as connection:
            response = connection.return_value.getresponse.return_value
            response.status = 200
            response.read.return_value = b'{"healthy":true}'
            self.assertEqual(http(8650, 'fixed-test-bearer', 'GET', '/v1/capabilities'), (200, {'healthy': True}))
            connection.assert_called_once_with('127.0.0.1', 8650, timeout=10)
            response.read.assert_called_once_with(262145)
            connection.return_value.close.assert_called_once()

    def test_unsupported_http_method_cannot_escape_traffic_counter(self):
        counters = {'normal': 0, 'tool': 0, 'slow': 0, 'invalid': 0}
        with patch('checks.synthetic_provider.COUNTS', counters), \
             patch('checks.synthetic_provider.Path.write_text'), \
             patch('http.server.BaseHTTPRequestHandler.send_error') as send:
            Handler.__new__(Handler).send_error(501, 'Unsupported method')
        self.assertEqual(counters['invalid'], 1)
        send.assert_called_once_with(501, 'Unsupported method', None)

    def test_catalog_lookup_is_authenticated_counted_404_never_fake_metadata(self):
        counters = {'normal': 0, 'tool': 0, 'slow': 0, 'metadata': 0, 'invalid': 0}
        handler = Handler.__new__(Handler)
        handler.headers = Message()
        handler.headers['Authorization'] = 'Bearer ' + TOKEN
        handler.send_response = Mock()
        handler.send_header = Mock()
        handler.end_headers = Mock()
        handler.send_error = Mock()
        with patch('checks.synthetic_provider.COUNTS', counters), patch('checks.synthetic_provider.Path.write_text'):
            for path in ('/v1/models', '/models'):
                handler.path = path
                handler.do_GET()
            handler.path = '/health'
            handler.do_GET()
        self.assertEqual(counters['metadata'], 2)
        self.assertEqual([call.args for call in handler.send_response.call_args_list], [(404,), (404,)])
        handler.send_error.assert_called_once_with(404)

    def test_cleanup_proves_absence_or_exact_label_and_image(self):
        command = Mock(side_effect=['fixture', 'owner|sha256:abc', '', ''])
        self.assertTrue(remove_owned(command, 'container', 'fixture', 'owner', 'sha256:abc'))
        self.assertEqual(command.call_args_list[2].args[0], ['container', 'rm', '--force', 'fixture'])
        self.assertIn('--all', command.call_args_list[0].args[0])
        absent = Mock(return_value='')
        self.assertTrue(remove_owned(absent, 'network', 'fixture', 'owner', 'sha256:abc'))
        self.assertEqual(absent.call_count, 1)

    def test_cleanup_refuses_unknown_owner_partial_name_and_daemon_failure(self):
        for results in (['fixture', 'other|sha256:abc'], ['fixture', 'owner|sha256:other'], ['fixture-extra']):
            command = Mock(side_effect=results)
            self.assertFalse(remove_owned(command, 'container', 'fixture', 'owner', 'sha256:abc'))
            self.assertFalse(any(c.args[0][1] == 'rm' for c in command.call_args_list))
        with self.assertRaises(RuntimeError):
            remove_owned(Mock(side_effect=RuntimeError('daemon failed')), 'network', 'fixture', 'owner', 'sha256:abc')


if __name__ == '__main__': unittest.main()
