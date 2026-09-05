"""Status must observe fixed surfaces without claiming authority or using secrets."""

import http.client
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import bootstrap_private_memory as bootstrap
import private_status as subject
from test_bootstrap_private_memory import COMPANY, DEPLOYMENT, Service, TOKEN_ENV


class LocalStatusTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.owner = "a" * 64
        self.api = {"origin": "http://127.0.0.1:8787",
                    "community_id": "d34d8968-c8ab-4791-992f-990137e0c470",
                    "humans": [{"public_key": self.owner, "role": "operator",
                                "employee_ids": ["ada-private"],
                                "channel_ids": ["81381e63-fc56-41b5-8406-e7e23820906a"]}],
                    "allowed_web_origins": ["http://localhost:1427", "tauri://localhost"]}
        self.write("api-config.json", self.api)

    def write(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value))
        path.chmod(0o600)

    def test_config_presence_never_claims_activation_or_live_authority(self):
        with patch.dict(os.environ, {"ORTAK_HONCHO_PRIVATE_TOKEN": "fixture-secret-never-read"}):
            result = subject.collect(self.root, self.owner)
        self.assertEqual(result["local_configuration"]["api_audience"]["expected_owner"], "matches")
        self.assertTrue(all(value == "not_checked" for value in result["control_authority"].values()))
        self.assertTrue(all(value == "not_checked" for value in result["workflow"].values()))
        self.assertEqual(result["services"]["hermes_bridge_auth"], {"observation": "not_configured"})
        self.assertTrue(all(value == {"observation": "not_checked"}
                            for name, value in result["services"].items() if name != "hermes_bridge_auth"))
        self.assertEqual(result["actions"], {"activation": False, "mutation": False, "credentials_loaded": False})
        self.assertNotIn("fixture-secret-never-read", json.dumps(result))

    def test_explicit_public_owner_mismatch_and_foreign_config_refuse(self):
        self.assertEqual(subject.inspect_api(self.root, "b" * 64), {"observation": "expected_owner_mismatch"})
        self.api["humans"][0]["employee_ids"] = ["cem"]
        self.write("api-config.json", self.api)
        self.assertEqual(subject.observed(lambda: subject.inspect_api(self.root, None)),
                         {"observation": "invalid_or_unreadable"})

    def test_numeric_public_uuid_preserves_the_complete_status_report(self):
        self.api["community_id"] = 123
        self.write("api-config.json", self.api)
        result = subject.collect(self.root)
        encoded = json.dumps(result)
        self.assertEqual(result["local_configuration"]["api_audience"],
                         {"observation": "invalid_or_unreadable"})
        self.assertEqual(result["services"]["hermes_bridge_auth"], {"observation": "not_configured"})
        self.assertEqual(result["control_authority"]["employee_activation"], "not_checked")
        self.assertIn('"format": "ortak-private-status/1"', encoded)

    def test_optional_project_creation_boolean_does_not_claim_live_authority(self):
        for value in (False, True, 1, None, "true"):
            with self.subTest(value=value):
                self.api["humans"][0]["can_create_projects"] = value
                self.write("api-config.json", self.api)
                result = subject.collect(self.root)
                expected = "valid_local_config" if isinstance(value, bool) else "invalid_or_unreadable"
                self.assertEqual(result["local_configuration"]["api_audience"]["observation"], expected)
                self.assertTrue(all(value == "not_checked" for value in result["control_authority"].values()))
                self.assertFalse(result["actions"]["activation"])

    def test_receipts_are_historical_and_rechecked_without_http(self):
        self.write("identities.json", {"project": subject.PROJECT, "company_id": COMPANY,
                                       "employee_id": "ada-private"})
        bootstrap.bootstrap(self.root, DEPLOYMENT, TOKEN_ENV, Service(self.root))
        result = subject.inspect_memory(self.root, COMPANY)
        self.assertEqual(result["roundtrip"], "historically_verified")
        self.assertEqual(result["current_resource_identity"], "not_checked")
        self.assertEqual(result["current_execution_witness"], "not_checked")
        state = json.loads((self.root / "memory/bootstrap.json").read_text())
        state["resource_identity"]["request_hash"] = "0" * 64
        self.write("memory/bootstrap.json", state)
        self.assertEqual(subject.observed(lambda: subject.inspect_memory(self.root, COMPANY)),
                         {"observation": "invalid_or_unreadable"})

    def test_private_mode_symlinks_and_size_are_enforced(self):
        path = self.root / "api-config.json"
        path.chmod(0o644)
        self.assertEqual(subject.observed(lambda: subject.inspect_api(self.root, None)),
                         {"observation": "invalid_or_unreadable"})
        path.unlink()
        target = self.root / "public-fixture.json"
        target.write_text(json.dumps(self.api))
        target.chmod(0o600)
        path.symlink_to(target)
        self.assertEqual(subject.observed(lambda: subject.inspect_api(self.root, None)),
                         {"observation": "invalid_or_unreadable"})
        path.unlink()
        path.write_text("x" * 16385)
        path.chmod(0o600)
        self.assertEqual(subject.observed(lambda: subject.inspect_api(self.root, None)),
                         {"observation": "invalid_or_unreadable"})

    def test_unknown_or_floating_image_is_not_a_running_pin(self):
        directory = self.root / "object-store"
        directory.mkdir(mode=0o700)
        image = directory / "image.env"
        image.write_text("ORTAK_MINIO_IMAGE=minio:latest\n")
        image.chmod(0o600)
        self.assertEqual(subject.observed(lambda: subject.inspect_image(self.root)),
                         {"observation": "invalid_or_unreadable"})
        image.write_text("ORTAK_MINIO_IMAGE=sha256:" + "a" * 64 + "\n")
        self.assertEqual(subject.inspect_image(self.root)["running_image"], "not_checked")

    def test_cli_refuses_unmarked_state_before_any_probe(self):
        with patch("sys.argv", ["private_status.py", "--state-dir", str(self.root)]), \
                patch.object(subject, "HttpProbe") as probe:
            with self.assertRaises(ValueError):
                subject.main()
            probe.assert_not_called()

    def test_hermes_probe_requires_an_explicit_exact_private_origin(self):
        probe = Mock(return_value={"observation": "fixture_probe"})
        result = subject.collect(self.root, probe=probe)
        self.assertEqual(probe.call_count, 6)
        self.assertEqual(result["services"]["hermes_bridge_auth"]["observation"], "not_configured")
        self.write("worker-config.json", {"company_slug": subject.PROJECT, "bridge_origin": "http://127.0.0.1:8790"})
        probe.reset_mock()
        result = subject.collect(self.root, probe=probe)
        self.assertEqual(probe.call_count, 6)
        self.assertEqual(result["services"]["hermes_bridge_auth"]["observation"], "invalid_or_unreadable")
        self.write("worker-config.json", {"company_slug": subject.PROJECT, "bridge_origin": "http://127.0.0.1:8650"})
        probe.reset_mock()
        subject.collect(self.root, probe=probe)
        self.assertEqual(probe.call_count, 7)

    def test_semantic_selection_is_opaque_and_never_blocks_the_hermes_probe(self):
        valid_selection = {
            "deployment": {"deployment_id": DEPLOYMENT, "origin": "https://fixture.invalid",
                           "model": "fixture-model", "response_model": "fixture-model",
                           "token_ref": "credential://fixture/semantic"},
            "token_env": "ORTAK_SEMANTIC_FIXTURE_TOKEN",
        }
        for selection in (valid_selection, {"deployment": 123}, "malformed", None):
            with self.subTest(selection=selection):
                self.write("worker-config.json", {
                    "company_slug": subject.PROJECT, "bridge_origin": "http://127.0.0.1:8650",
                    "semantic": selection,
                })
                probe = Mock(return_value={"observation": "fixture_probe"})
                with patch.object(subject.os, "environ") as environment, \
                        patch.object(subject.http.client, "HTTPConnection", side_effect=AssertionError("unexpected HTTP")):
                    environment.get.side_effect = AssertionError("unexpected credential lookup")
                    environment.__getitem__.side_effect = AssertionError("unexpected credential lookup")
                    result = subject.collect(self.root, probe=probe)
                    self.assertEqual(environment.mock_calls, [])
                self.assertEqual([call.args[0] for call in probe.call_args_list], list(subject.ENDPOINTS))
                self.assertEqual(result["local_configuration"]["worker_selection"],
                                 {"observation": "local_origin_selected", "execution_configuration": "not_checked"})
                self.assertEqual(result["services"]["hermes_bridge_auth"], {"observation": "fixture_probe"})
                self.assertEqual(result["workflow"]["semantic_scoring"], "not_checked")
                self.assertFalse(result["actions"]["credentials_loaded"])
                self.assertNotIn("ORTAK_SEMANTIC_FIXTURE_TOKEN", json.dumps(result))


class HttpStatusTests(unittest.TestCase):
    def test_probe_never_reads_body_authentication_or_redirect_target(self):
        for status, expected in ((200, "http_health_ok"), (302, "unexpected_http_status")):
            with patch.object(subject.http.client, "HTTPConnection") as connection:
                response = Mock(status=status)
                connection.return_value.getresponse.return_value = response
                result = subject.HttpProbe()(subject.ENDPOINTS[0])
                self.assertEqual(result["observation"], expected)
                self.assertEqual(connection.call_args.args, ("127.0.0.1", 8089))
                headers = connection.return_value.request.call_args.kwargs["headers"]
                self.assertNotIn("Authorization", headers)
                response.read.assert_not_called()
                response.read1.assert_not_called()
                connection.return_value.close.assert_called_once()

    def test_auth_fence_is_distinct_from_service_health(self):
        for status, expected in ((401, "authentication_required"), (403, "authentication_required"),
                                 (200, "unexpected_http_status"), (503, "unexpected_http_status")):
            with patch.object(subject.http.client, "HTTPConnection") as connection:
                connection.return_value.getresponse.return_value.status = status
                self.assertEqual(subject.HttpProbe()(subject.ENDPOINTS[-1])["observation"], expected)

    def test_errors_and_deadline_are_truthful_and_never_echo_response_data(self):
        for error, expected in ((TimeoutError("sensitive fixture"), "timeout"),
                                (OSError("sensitive fixture"), "unreachable_or_invalid_http"),
                                (http.client.BadStatusLine("sensitive fixture"), "unreachable_or_invalid_http")):
            with patch.object(subject.http.client, "HTTPConnection") as connection:
                connection.return_value.getresponse.side_effect = error
                result = subject.HttpProbe()(subject.ENDPOINTS[0])
                self.assertEqual(result, {"observation": expected})
                self.assertNotIn("sensitive", json.dumps(result))
        probe = subject.HttpProbe()
        probe.deadline = 0
        with patch.object(subject.http.client, "HTTPConnection") as connection:
            self.assertEqual(probe(subject.ENDPOINTS[0]), {"observation": "deadline"})
            connection.assert_not_called()
            with self.assertRaises(ValueError):
                probe(("unselected", 443, "/", "health"))
            connection.assert_not_called()


if __name__ == "__main__":
    unittest.main()
