"""Credential-free harness binds production DockerEngine argument transport."""
import unittest
from unittest.mock import patch
from checks.containment_check import ProbeEngine, PROBE
from ortak_hermes_bridge.docker_executor import DockerEngine

class ProbeCommand(unittest.TestCase):
    def test_fixed_probe_preserves_every_containment_argument_and_stdin(self):
        args = ['run', '--read-only', '--user', '10001:10001', '--network', 'fixture-private',
                'sha256:' + 'a' * 64, '-m', 'ortak_hermes_bridge.worker',
                '--journal', '/ortak-state/journal.sqlite']
        original = list(args)
        payload = b'fixture stdin is never copied into arguments'
        with patch.object(DockerEngine, 'launch') as launch:
            ProbeEngine().launch(args, payload)
        forwarded, forwarded_payload = launch.call_args.args
        self.assertEqual(args, original)
        self.assertEqual(forwarded[:7], args[:7])
        self.assertEqual(forwarded[-2:], args[-2:])
        self.assertEqual(forwarded[7:9], ['-c', PROBE])
        self.assertIs(forwarded_payload, payload)
        self.assertNotIn(payload.decode(), repr(forwarded))
        compile(PROBE, '<contained-fixed-probe>', 'exec')

if __name__ == '__main__':
    unittest.main()
