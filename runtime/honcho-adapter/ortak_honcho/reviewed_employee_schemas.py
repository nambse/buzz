"""Closed employee-family requests; no project/session or runtime authority."""

import hashlib
import unicodedata
from typing import Annotated, Literal
from uuid import UUID

from pydantic import AfterValidator, Field, model_validator

from .schemas import Employee, Name, Strict


def nonnil(value):
    parsed = UUID(value)
    if not parsed.int or str(parsed) != value:
        raise ValueError("invalid canonical UUID")
    return value


Id = Annotated[str, AfterValidator(nonnil)]
Hash = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
NativeId = Annotated[str, Field(min_length=1, max_length=128, pattern=r"^[\x21-\x7e]+$")]


class Binding(Strict):
    adapter: Literal["honcho"]
    endpoint_ref: Annotated[str, Field(min_length=1, max_length=256, pattern=r"^[\x21-\x7e]+$")]
    workspace: Name
    user_peer: Name
    employee_peer: Name
    options: dict

    @model_validator(mode="after")
    def closed(self):
        if self.options or self.user_peer == self.employee_peer:
            raise ValueError("invalid employee binding")
        return self


class NativeIds(Strict):
    workspace: NativeId
    peers: Annotated[dict[Name, NativeId], Field(min_length=2, max_length=2)]


class Ownership(Strict):
    request_hash: Hash
    native_ids: NativeIds


class Common(Strict):
    company_id: Id
    employee_id: Employee
    deployment_id: Id
    binding: Binding
    ownership: Ownership

    @model_validator(mode="after")
    def peers(self):
        if set(self.ownership.native_ids.peers) != {
            self.binding.user_peer, self.binding.employee_peer
        }:
            raise ValueError("invalid employee peer selection")
        return self


class Mutation(Common):
    target_id: Id
    destination_channel_id: Id
    idempotency_key: Annotated[str, Field(min_length=1, max_length=200)]
    content_hash: Hash
    source_hash: Hash
    sharing_hash: Hash


class Publish(Mutation):
    content: Annotated[str, Field(min_length=1, max_length=4096)]
    provenance: Annotated[str, Field(min_length=1, max_length=4096)]

    @model_validator(mode="after")
    def content_commitment(self):
        if (not self.content.strip() or len(self.content.encode()) > 4096
            or any(unicodedata.category(c) == "Cc" and c not in "\n\t" for c in self.content)
            or hashlib.sha256(self.content.encode()).hexdigest() != self.content_hash):
            raise ValueError("invalid employee reviewed text")
        from .reviewed_employee_provenance import validate
        validate(self)
        return self


class Selected(Common):
    destination_channel_id: Id
    human_public_key: Hash | None
    record_ids: Annotated[list[Id], Field(min_length=1, max_length=8)]

    @model_validator(mode="after")
    def unique(self):
        if len(set(self.record_ids)) != len(self.record_ids):
            raise ValueError("duplicate selected records")
        return self


class Diagnostic(Common):
    employee_revision_id: Id
    employee_lifecycle_epoch: Annotated[int, Field(strict=True, ge=0, le=9223372036854775807)]


class DiagnosticWrite(Diagnostic):
    challenge: Hash


class DiagnosticRead(Diagnostic):
    challenge_hash: Hash
