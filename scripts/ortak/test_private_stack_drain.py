"""Exercise the real drain SELECT on disposable PostgreSQL with isolated temporary rows."""

import json
import os
import unittest
from unittest.mock import Mock, patch

import private_stack_services as services
from private_stack_state import docker, inspect_container

TABLES = ("runs", "office_inbox", "outbox", "runtime_work_outputs", "runtime_memory_writes",
          "provisioning_operations", "reviewed_memory_export_jobs", "employee_reviewed_memory_export_jobs",
          "provisioning_runtime_probes", "workspace_tool_actions", "workspace_reader_executions",
          "encrypted_dm_decrypt_jobs", "confidential_dm_receipts", "confidential_runs",
          "confidential_run_dispatches", "confidential_execution_leases", "confidential_reply_outbox")


@unittest.skipUnless(os.environ.get("ORTAK_LIFECYCLE_TEST_POSTGRES") == "disposable", "explicit disposable PostgreSQL selection required")
class DrainPostgresTests(unittest.TestCase):
    def observe(self, inserts):
        row = inspect_container("buzz-postgres")
        self.assertEqual(row["Config"]["Labels"].get("dev.ortak.purpose"), "disposable-integration-20260907")
        identifier = row["Id"]
        installation = Mock(manifest={"schema_version": 80, "containers": {"postgres-1": {"id": identifier}}})
        prefix = "BEGIN; SET LOCAL statement_timeout='5s'; "
        # CTAS deliberately removes production row constraints, so each vector
        # can isolate one unsettled field without mutating any persistent table.
        prefix += " ".join(f"CREATE TEMP TABLE {name} AS SELECT * FROM public.{name} WITH NO DATA;" for name in TABLES)
        prefix += inserts

        def selected_transport(*args, **kwargs):
            self.assertEqual(args[:3], ("exec", "-i", identifier))
            query = kwargs.pop("input_bytes").decode()
            self.assertTrue(query.startswith("BEGIN READ ONLY;"))
            query = query.replace("BEGIN READ ONLY;", prefix, 1)
            return docker("exec", "-i", identifier, "psql", "-U", "ortak", "-d", "ortak_context80",
                          "-XAtq", "-v", "ON_ERROR_STOP=1", input_bytes=query.encode(), **kwargs)

        with patch.object(services, "docker", side_effect=selected_transport):
            return services.pending(installation)

    def test_future_unattempted_withdrawals_do_not_block_but_due_or_uncertain_jobs_do(self):
        future = """INSERT INTO reviewed_memory_export_jobs(state,action,next_attempt_at,total_attempts)
          VALUES('pending','withdraw',clock_timestamp()+interval '1 hour',0);"""
        self.assertFalse(any(self.observe(future).values()))
        for change in ("next_attempt_at=clock_timestamp()-interval '1 second'", "total_attempts=1", "state='failed'"):
            with self.subTest(change=change):
                result = self.observe(future + "UPDATE reviewed_memory_export_jobs SET " + change + ";")
                self.assertEqual(result["reviewed_exports"], 1)

    def test_retained_extension_work_is_visible_to_the_production_stop_gate(self):
        fixture = """
        INSERT INTO employee_reviewed_memory_export_jobs(state,action) VALUES('pending','publish');
        INSERT INTO provisioning_runtime_probes(state) VALUES('succeeded');
        INSERT INTO workspace_tool_actions(state) VALUES('result_ready');
        INSERT INTO workspace_reader_executions(state) VALUES('stopped');
        INSERT INTO encrypted_dm_decrypt_jobs(state) VALUES('verified');
        INSERT INTO confidential_runs(run_id) VALUES(gen_random_uuid());
        INSERT INTO confidential_reply_outbox(state) VALUES('pending');
        """
        result = self.observe(fixture)
        for key in ("employee_exports", "runtime_probes", "workspace_actions", "workspace_readers",
                    "encrypted_decrypt", "encrypted_execution", "encrypted_replies"):
            self.assertEqual(result[key], 1, key)


if __name__ == "__main__":
    unittest.main()
