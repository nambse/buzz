"""Bounded extension wire schema; no caller-controlled Honcho configuration."""

from typing import Annotated, Literal
from uuid import UUID

from pydantic import AwareDatetime, BaseModel, ConfigDict, Field, model_validator

Name = Annotated[str, Field(min_length=1, max_length=128, pattern=r"^[A-Za-z0-9_-]+$")]
Employee = Annotated[
    str, Field(min_length=1, max_length=64, pattern=r"^[a-z0-9][a-z0-9_-]*$")
]
Key = Annotated[str, Field(min_length=1, max_length=200, pattern=r"^[\x21-\x7e]+$")]


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class Scope(Strict):
    scope: Literal[
        "company_truth",
        "project_context",
        "employee_experience",
        "relationship",
        "run_scratch",
    ]
    project_id: UUID | None = None
    run_id: UUID | None = None

    @model_validator(mode="after")
    def exact_shape(self):
        if (self.project_id is not None) != (self.scope == "project_context"):
            raise ValueError("project_id is required only for project_context")
        if (self.run_id is not None) != (self.scope == "run_scratch"):
            raise ValueError("run_id is required only for run_scratch")
        return self


class Provenance(Strict):
    employee_id: Employee
    run_id: UUID | None = None
    source: Annotated[str, Field(min_length=1, max_length=128)]
    recorded_at: AwareDatetime


class Fact(Strict):
    content: Annotated[str, Field(min_length=1, max_length=16384)]
    provenance: Provenance

    @model_validator(mode="after")
    def bounded_content(self):
        if (
            not self.content.strip()
            or "\x00" in self.content
            or len(self.content.encode()) > 16384
        ):
            raise ValueError(
                "content must be nonempty UTF-8 text of at most 16384 bytes without NUL"
            )
        if not self.provenance.source.strip() or "\x00" in self.provenance.source:
            raise ValueError("source must be nonempty without NUL")
        return self


class InspectResources(Strict):
    company_id: UUID
    employee_id: Employee
    user_peer: Name
    employee_peer: Name


class CreateResources(Strict):
    idempotency_key: Key
    company_id: UUID
    employee_id: Employee
    workspace_id: Name
    user_peer: Name
    employee_peer: Name

    @model_validator(mode="after")
    def distinct_peers(self):
        if self.user_peer == self.employee_peer:
            raise ValueError("user and employee peers must differ")
        return self


class Remember(Strict):
    idempotency_key: Key
    company_id: UUID
    employee_id: Employee
    scope: Scope
    facts: Annotated[list[Fact], Field(min_length=1, max_length=64)]

    @model_validator(mode="after")
    def consistent_provenance(self):
        for fact in self.facts:
            if fact.provenance.employee_id != self.employee_id:
                raise ValueError("fact employee differs from binding")
            if (
                self.scope.scope == "run_scratch"
                and fact.provenance.run_id != self.scope.run_id
            ):
                raise ValueError("fact run differs from scratch scope")
        return self


class Recall(Strict):
    company_id: UUID
    employee_id: Employee
    scope: Scope
    query: Annotated[str, Field(min_length=1, max_length=4096)]
    max_records: Annotated[int, Field(ge=1, le=100)] = 32
    max_bytes: Annotated[int, Field(ge=1, le=131072)] = 16384

    @model_validator(mode="after")
    def bounded_query(self):
        if (
            not self.query.strip()
            or "\x00" in self.query
            or len(self.query.encode()) > 4096
        ):
            raise ValueError(
                "query must be nonempty and at most 4096 UTF-8 bytes without NUL"
            )
        return self
