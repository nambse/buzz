import type {
  EmployeeExportRecord,
  EmployeeExportReceipt,
  MemoryOperation,
} from "./types";
import { uuid } from "./validation";

function require(value: unknown): asserts value {
  if (!value) throw new Error("Publication status could not be verified.");
}
function keys(value: unknown, expected: string[]) {
  require(value && typeof value === "object" && !Array.isArray(value));
  require(Object.keys(value).sort().join(",") === expected.sort().join(","));
}
const integer = (value: number, max: number) =>
  Number.isInteger(value) && value >= 0 && value <= max;

/** Reject mismatched facts, extra content and unconfirmed success projections. */
export function assertExport(value: EmployeeExportRecord, fact: string) {
  keys(value, ["fact_id", "export"]);
  require(uuid(fact) && value.fact_id === fact);
  if (value.export === null) return;
  const saved = value.export;
  keys(saved, ["target_id", "created_at", "jobs"]);
  require(
    uuid(saved.target_id) &&
      typeof saved.created_at === "string" &&
      Number.isFinite(Date.parse(saved.created_at)),
  );
  require(Array.isArray(saved.jobs) && saved.jobs.length === 2);
  for (const [index, job] of saved.jobs.entries()) {
    keys(job, [
      "action",
      "state",
      "attempt_count",
      "total_attempts",
      "retry_version",
      "last_error_code",
      "acknowledged",
    ]);
    require(job.action === (index === 0 ? "publish" : "withdraw"));
    require(["pending", "acknowledged", "failed"].includes(job.state));
    require(
      integer(job.attempt_count, 20) &&
        integer(job.total_attempts, 180) &&
        integer(job.retry_version, 8),
    );
    require(
      job.total_attempts >= job.attempt_count &&
        job.total_attempts <= 20 * (job.retry_version + 1),
    );
    require(
      typeof job.acknowledged === "boolean" &&
        job.acknowledged === (job.state === "acknowledged"),
    );
    require(
      job.last_error_code === null ||
        (typeof job.last_error_code === "string" &&
          /^[a-z0-9_]{1,64}$/.test(job.last_error_code)),
    );
    if (job.acknowledged)
      require(job.total_attempts > 0 && job.last_error_code === null);
  }
}

/** Bind the immutable command receipt and allow later metadata on exact replay. */
export function assertExportReceipt(
  value: EmployeeExportReceipt,
  operation: MemoryOperation,
) {
  keys(value, ["operation_id", "created", "result_version", "record"]);
  require(
    value.operation_id === operation.operationId &&
      typeof value.created === "boolean" &&
      operation.factId,
  );
  require(
    ["publish", "retry_publish", "retry_withdraw"].includes(operation.action),
  );
  const version =
    operation.action === "publish"
      ? 0
      : JSON.parse(operation.body).expected_version + 1;
  require(integer(version, 8) && value.result_version === version);
  assertExport(value.record, operation.factId);
  require(value.record.export);
  const action = operation.action === "retry_withdraw" ? "withdraw" : "publish";
  require(
    value.record.export.jobs.some(
      (job) => job.action === action && job.retry_version >= version,
    ),
  );
}
