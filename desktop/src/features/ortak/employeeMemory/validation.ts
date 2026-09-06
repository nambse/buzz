import type {
  EmployeeAudience,
  EmployeeFact,
  EmployeeFactPage,
  EmployeePreview,
  EmployeePreviewRequest,
  EmployeeReceipt,
  EmployeeSource,
  MemoryOperation,
} from "./types";

const hex = (value: unknown): value is string =>
  typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
export const uuid = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    value,
  ) &&
  value !== "00000000-0000-0000-0000-000000000000";
const date = (value: unknown) =>
  typeof value === "string" && Number.isFinite(Date.parse(value));
function require(value: unknown): asserts value {
  if (!value)
    throw new Error("Employee memory response could not be verified.");
}
function audience(value: EmployeeAudience, employee: string, actor: string) {
  require(
    value &&
      value.format === "ortak-reviewed-employee-audience/1" &&
      value.employee_id === employee &&
      uuid(value.company_id) &&
      uuid(value.destination_community_id) &&
      uuid(value.destination_channel_id),
  );
  require(
    value.kind === "experience"
      ? value.human_public_key === null
      : value.kind === "relationship" && value.human_public_key === actor,
  );
}
function source(value: EmployeeSource, actor: string) {
  require(
    value &&
      value.author_public_key === actor &&
      uuid(value.channel_id) &&
      uuid(value.community_id) &&
      hex(value.event_id) &&
      hex(value.evidence_hash) &&
      date(value.event_created_at),
  );
}
/** Verify the returned destination and original source before presenting approval. */
export function assertPreview(
  value: EmployeePreview,
  employee: string,
  actor: string,
  channel: string,
  request: EmployeePreviewRequest,
) {
  require(value && value.employee_id === employee);
  audience(value.audience, employee, actor);
  source(value.source, actor);
  require(
    value.audience.destination_channel_id === request.destination_channel_id &&
      value.audience.kind === request.kind &&
      value.audience.human_public_key === request.human_public_key &&
      value.source.event_id === request.source_event_id &&
      value.source.channel_id === channel &&
      value.source.community_id === value.audience.destination_community_id,
  );
  require(
    hex(value.audience_hash) &&
      hex(value.source_hash) &&
      date(value.observed_at) &&
      date(value.max_expires_at) &&
      (value.valid_before === null || date(value.valid_before)),
  );
  require(Date.parse(value.max_expires_at) > Date.parse(value.observed_at));
}
export function assertFact(
  value: EmployeeFact,
  employee: string,
  actor: string,
) {
  require(
    value &&
      uuid(value.id) &&
      value.employee_id === employee &&
      ["experience", "relationship"].includes(value.kind),
  );
  require(
    date(value.approved_at) &&
      date(value.expires_at) &&
      (value.revoked_at === null || date(value.revoked_at)) &&
      typeof value.source_current === "boolean",
  );
  require(
    value.version === 1
      ? ["approved", "expired"].includes(value.status) &&
          value.revoked_at === null &&
          value.can_stop === true
      : value.version === 2 &&
          value.status === "stopped" &&
          date(value.revoked_at) &&
          value.can_stop === false,
  );
  if (!value.source_current) {
    require(
      [
        value.content,
        value.audience,
        value.audience_hash,
        value.source,
        value.source_hash,
        value.provenance,
        value.sharing_hash,
      ].every((field) => field === null),
    );
    return;
  }
  require(
    value.audience &&
      value.source &&
      typeof value.content === "string" &&
      value.content.trim() &&
      new TextEncoder().encode(value.content).length <= 4096 &&
      value.provenance &&
      typeof value.provenance === "object" &&
      !Array.isArray(value.provenance),
  );
  audience(value.audience, employee, actor);
  source(value.source, actor);
  require(
    value.kind === value.audience.kind &&
      value.source.community_id === value.audience.destination_community_id &&
      hex(value.audience_hash) &&
      hex(value.source_hash) &&
      hex(value.sharing_hash),
  );
}
export function assertPage(
  value: EmployeeFactPage,
  employee: string,
  actor: string,
  after?: string,
) {
  require(
    value &&
      typeof value.can_approve === "boolean" &&
      Array.isArray(value.facts) &&
      value.facts.length <= 16 &&
      (value.next_after === null || uuid(value.next_after)),
  );
  let previous = after ?? "";
  for (const fact of value.facts) {
    assertFact(fact, employee, actor);
    require(fact.id > previous);
    previous = fact.id;
  }
  require(
    value.next_after === null ||
      (value.facts.length === 16 && value.next_after === previous),
  );
}
export function assertReceipt(
  value: EmployeeReceipt,
  employee: string,
  actor: string,
  operation: MemoryOperation,
) {
  require(
    value &&
      value.operation_id === operation.operationId &&
      typeof value.created === "boolean" &&
      value.effect &&
      value.effect.action === operation.action &&
      value.effect.result_version === (operation.action === "approve" ? 1 : 2),
  );
  assertFact(value.fact, employee, actor);
  require(
    value.effect.fact_id === value.fact.id &&
      (!operation.factId || operation.factId === value.fact.id),
  );
  if (operation.action === "stop") require(value.fact.version === 2);
  else {
    const draft = JSON.parse(operation.body).fact;
    require(
      value.fact.kind === draft.kind &&
        Date.parse(value.fact.expires_at) === Date.parse(draft.expires_at),
    );
    if (value.fact.source_current) {
      require(value.fact.audience && value.fact.source);
      require(
        value.fact.content === draft.content &&
          value.fact.audience_hash === draft.expected_audience_hash &&
          value.fact.audience.destination_channel_id ===
            draft.destination_channel_id &&
          value.fact.audience.human_public_key === draft.human_public_key &&
          value.fact.source.event_id === draft.source_event_id &&
          value.fact.source.event_created_at === draft.source_event_created_at,
      );
    }
  }
}
