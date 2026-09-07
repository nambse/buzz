"""Production lifecycle ownership, failure propagation and process containment."""

import copy
import json
import os
from pathlib import Path
import plistlib
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

import private_stack_launch as launch
import private_stack_services as services
import private_stack_state as state


class PrivateStackTests(unittest.TestCase):
    def test_command_bounds_output_and_time(self):
        with self.assertRaises(ValueError):
            state.command([sys.executable, "-c", "print('x'*100000)"], maximum=1024)
        started = time.monotonic()
        with self.assertRaises(ValueError):
            state.command([sys.executable, "-c", "import time; time.sleep(20)"], timeout=0.1)
        self.assertLess(time.monotonic() - started, 3)

    def test_nonzero_never_becomes_success(self):
        with self.assertRaises(ValueError):
            state.command([sys.executable, "-c", "raise SystemExit(7)"])
        self.assertEqual(state.command([sys.executable, "-c", "raise SystemExit(7)"], allow_failure=True)[0], 7)

    def test_supervisor_ends_descendants_after_parent_exits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = "import subprocess,sys; subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'])"
            started = time.monotonic()
            self.assertEqual(launch.supervise([sys.executable, "-c", script], dict(os.environ), root, root / "log"), 0)
            self.assertLess(time.monotonic() - started, 4)

    def test_logs_are_bounded_without_newlines(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = "import sys; sys.stdout.write('x'*(20*1024**2))"
            self.assertEqual(launch.supervise([sys.executable, "-c", script], dict(os.environ), root, root / "log"), 0)
            logs = list(root.glob("log*"))
            self.assertLessEqual(len(logs), 4)
            self.assertTrue(all(p.stat().st_size <= 4 * 1024**2 for p in logs))

    def test_fingerprint_refuses_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "file").write_text("selected")
            (root / "alias").symlink_to(root / "file")
            with self.assertRaises(ValueError):
                state.fingerprint(root / "alias")

    def test_replaced_container_refuses_before_any_mutation(self):
        installation = object.__new__(state.Installation)
        original = {"Id": "old-id", "Name": "/selected", "Image": "sha256:pinned", "Config": {"Labels": {}},
                    "Mounts": [], "HostConfig": {"PortBindings": {}, "RestartPolicy": {}},
                    "NetworkSettings": {"Networks": {}}, "State": {"Running": True}}
        installation.manifest = {"containers": {"hermes": state.container_contract(original)}}
        replaced = copy.deepcopy(original)
        replaced["Id"] = "new-id"
        with patch.object(state, "inspect_container", return_value=replaced):
            with self.assertRaises(ValueError):
                installation.verify()

    def test_mount_order_does_not_change_ownership_but_permissions_do(self):
        base = {"Id": "id", "Name": "/selected", "Image": "sha256:pinned", "Config": {},
                "Mounts": [{"Destination": "/b", "RW": False}, {"Destination": "/a", "RW": True}],
                "HostConfig": {"PortBindings": {}, "RestartPolicy": {}}, "NetworkSettings": {"Networks": {}}}
        reordered = copy.deepcopy(base)
        reordered["Mounts"].reverse()
        self.assertEqual(state.container_contract(base), state.container_contract(reordered))
        reordered["Mounts"][0]["RW"] = False
        self.assertNotEqual(state.container_contract(base), state.container_contract(reordered))

    def test_unboot_waits_for_launchd_retirement(self):
        with patch.object(services, "job", side_effect=[{"loaded": True}, {"loaded": True}, {"loaded": False}]), \
                patch.object(services, "command") as command, patch.object(services.time, "sleep") as sleep:
            services.unboot(Mock(), "worker")
        self.assertEqual(command.call_count, 2)
        sleep.assert_called_once_with(0.2)

    def test_actual_loaded_arguments_must_match_recorded_job(self):
        known = {"ProgramArguments": ["python", "selected-launcher", "worker"]}
        installation = Mock(manifest={"plists": {"worker": known}, "previous_plists": {"worker": known}})
        with patch.object(services, "private_file", return_value=plistlib.dumps(known).decode()), \
                patch.object(services, "command", return_value=(0, "\n arguments = {\n python\n foreign-launcher\n worker\n }")):
            with self.assertRaises(ValueError):
                services.job(installation, "worker")

    def test_pending_work_keeps_worker_and_stores_available(self):
        installation = Mock()
        installation.verify.return_value = {"postgres-1": {"Running": True}}
        with patch.object(services, "job"), patch.object(services, "unboot") as unboot, \
                patch.object(services, "pending", return_value={"runs": 1}), \
                patch.object(services.time, "monotonic", side_effect=[0, 46]), \
                patch.object(services, "docker") as docker:
            with self.assertRaises(ValueError):
                services.stop(installation)
        self.assertEqual([call.args[1] for call in unboot.call_args_list], ["api", "relay", "management"])
        docker.assert_not_called()
        installation.record.assert_called_with("stop", "pending_work_reopen_with_start")

    def test_stopped_installation_never_queries_unavailable_database(self):
        installation = Mock()
        installation.verify.return_value = {"postgres-1": {"Running": False, "Status": "exited"}}
        with patch.object(services, "job", return_value={"running": False}), \
                patch.object(services, "probes", return_value={}), patch.object(services, "pending") as pending:
            result = services.status(installation)
        pending.assert_not_called()
        self.assertIsNone(result["pending"])

    def test_checkpoint_keeps_durable_failed_phase(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "operation.json"
            state.save_json(path, {"phase": "stopping_stores"})
            self.assertEqual(json.loads(path.read_text()), {"phase": "stopping_stores"})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_scorer_failed_credential_owner_join_keeps_other_stores_running(self):
        installation = Mock()
        states = {"postgres-1": {"Running": False}, "hermes": {"Running": True},
                  "semantic": {"Running": True}}
        installation.manifest = {"company_id": "company", "containers": {
            name: {"id": name + "-owned"} for name in states}}
        failed = {**states, "semantic": {"Running": False, "ExitCode": 137, "OOMKilled": False}}
        installation.verify.side_effect = [states, states, failed]
        with patch.object(services, "job"), patch.object(services, "unboot"), \
                patch.object(services, "docker", return_value=(0, "")) as docker:
            with self.assertRaises(ValueError):
                services.stop(installation)
        stops = [call for call in docker.call_args_list if call.args[0] == "stop"]
        self.assertEqual(len(stops), 1)
        self.assertEqual(stops[0].args, ("stop", "--time", "45", "semantic-owned"))

    def test_scorer_contract_preserves_shutdown_and_resource_limits(self):
        row = {"Id": "scorer", "Name": "/selected", "Image": "image", "Mounts": [],
               "Config": {"Labels": {"org.ortak.role": "semantic-scorer"}, "User": "10001:10001",
                          "Entrypoint": ["python"], "Cmd": [], "StopTimeout": 45},
               "HostConfig": {"PortBindings": {}, "RestartPolicy": {}, "Memory": 512},
               "NetworkSettings": {"Networks": {}}}
        original = state.container_contract(row)
        row["Config"]["StopTimeout"] = 10
        self.assertNotEqual(original, state.container_contract(row))
        row["Config"]["StopTimeout"] = 45
        row["HostConfig"]["Memory"] = 0
        self.assertNotEqual(original, state.container_contract(row))


if __name__ == "__main__":
    unittest.main()
