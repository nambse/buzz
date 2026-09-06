"""Infrastructure-free checks against the production reviewed request schema."""

import hashlib
import unittest
from datetime import datetime, timedelta, timezone
from uuid import uuid4

from pydantic import ValidationError
from ortak_honcho.reviewed_schemas import (
    ReviewedInspect,
    ReviewedPublish,
    ReviewedRecall,
    ReviewedSelectedRecall,
)


class ReviewedContract(unittest.TestCase):
    def publication(self, content="Human reviewed fact"):
        return {
            "company_id": str(uuid4()),
            "employee_id": "cem",
            "idempotency_key": "reviewed_1",
            "content": content,
            "content_hash": hashlib.sha256(content.encode()).hexdigest(),
            "source_hash": "a" * 64,
            "approval_id": str(uuid4()),
            "approved_by": "b" * 64,
            "expires_at": (datetime.now(timezone.utc) + timedelta(days=1)).isoformat(),
        }

    def test_production_schema_binds_exact_reviewed_text_digest(self):
        body = self.publication()
        self.assertEqual(ReviewedPublish.model_validate(body).content, body["content"])
        with self.assertRaises(ValidationError):
            ReviewedPublish.model_validate(
                {**body, "content": "unreviewed changed text"}
            )

    def test_unknown_scope_and_native_configuration_are_never_admitted(self):
        for field in ("scope", "workspace_id", "deriver", "configuration", "run_id"):
            with self.assertRaises(ValidationError):
                ReviewedPublish.model_validate({**self.publication(), field: "foreign"})

    def test_utf8_and_control_bounds_are_not_character_count_only(self):
        for content in ("é" * 2049, " ", "fact\x00", "fact\u0085"):
            with self.assertRaises(ValidationError):
                ReviewedPublish.model_validate(self.publication(content))
        ReviewedPublish.model_validate(self.publication("line one\n\tline two"))

    def test_inspection_and_query_bounds_are_explicit(self):
        audience = {"company_id": str(uuid4()), "employee_id": "cem"}
        for limit in (0, 26):
            with self.assertRaises(ValidationError):
                ReviewedInspect.model_validate({**audience, "limit": limit})
        for query in (" ", "é" * 513, "text\x00"):
            with self.assertRaises(ValidationError):
                ReviewedRecall.model_validate({**audience, "query": query})

    def test_selected_recall_requires_finite_unique_nonzero_explicit_ids(self):
        audience = {"company_id": str(uuid4()), "employee_id": "cem", "query": "fact"}
        one = str(uuid4())
        for ids in ([], [one, one], ["00000000-0000-0000-0000-000000000000"],
                    [str(uuid4()) for _ in range(33)]):
            with self.assertRaises(ValidationError):
                ReviewedSelectedRecall.model_validate({**audience, "record_ids": ids})
        with self.assertRaises(ValidationError):
            ReviewedSelectedRecall.model_validate(audience)
        with self.assertRaises(ValidationError):
            ReviewedRecall.model_validate({**audience, "record_ids": [one]})
        self.assertEqual(len(ReviewedSelectedRecall.model_validate(
            {**audience, "record_ids": [one]}
        ).record_ids), 1)


if __name__ == "__main__":
    unittest.main()
