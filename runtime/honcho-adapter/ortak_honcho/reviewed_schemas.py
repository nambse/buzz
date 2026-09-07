"""Closed reviewed-project requests; no native Honcho configuration or broad scope."""

from typing import Annotated
from uuid import UUID

from pydantic import AwareDatetime, Field, model_validator

from .schemas import Employee, Key, Strict

Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]


class ReviewedAudience(Strict):
    company_id: UUID
    employee_id: Employee


class ReviewedPublish(ReviewedAudience):
    idempotency_key: Key
    content: Annotated[str, Field(min_length=1, max_length=4096)]
    content_hash: Digest
    source_hash: Digest
    approval_id: UUID
    approved_by: Digest
    expires_at: AwareDatetime

    @model_validator(mode="after")
    def reviewed_text(self):
        import hashlib
        import unicodedata

        if (
            not self.content.strip()
            or len(self.content.encode()) > 4096
            or any(
                unicodedata.category(char) == "Cc" and char not in "\n\t"
                for char in self.content
            )
            or "\x7f" in self.content
            or hashlib.sha256(self.content.encode()).hexdigest() != self.content_hash
        ):
            raise ValueError("invalid reviewed text or content hash")
        return self


class ReviewedMutation(ReviewedAudience):
    idempotency_key: Key


class ReviewedInspect(ReviewedAudience):
    after: UUID | None = None
    limit: Annotated[int, Field(ge=1, le=25)] = 25


class ReviewedRecall(ReviewedAudience):
    query: Annotated[str, Field(min_length=1, max_length=1024)]

    @model_validator(mode="after")
    def bounded_query(self):
        if (
            not self.query.strip()
            or len(self.query.encode()) > 1024
            or "\x00" in self.query
        ):
            raise ValueError("invalid reviewed query")
        return self


class ReviewedSelectedRecall(ReviewedRecall):
    record_ids: Annotated[list[UUID], Field(min_length=1, max_length=32)]

    @model_validator(mode="after")
    def explicit_selection(self):
        if any(record.int == 0 for record in self.record_ids) or len(
            set(self.record_ids)
        ) != len(self.record_ids):
            raise ValueError("invalid reviewed selection")
        return self
