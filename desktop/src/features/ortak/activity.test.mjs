import assert from "node:assert/strict";
import test from "node:test";
import { appendActivity, describeActivity } from "./activity.ts";

const entry = (sequence) => ({
  sequence,
  event_type: "run.queued",
  occurred_at: "2026-09-05T00:00:00Z",
  activity: { kind: "lifecycle", phase: { phase: "queued" } },
  redacted: false,
  truncated: false,
});
const page = (entries, next, has_more = false) => ({
  entries,
  next_after_sequence: next,
  gap: null,
  has_more,
});

test("initial sequence zero survives reconnect, duplicate replay, and an empty page", () => {
  const first = appendActivity([], null, page([entry(0)], 0));
  assert.equal(first.cursor, 0);
  const replay = appendActivity(
    first.entries,
    first.cursor,
    page([entry(0), entry(1)], 1),
  );
  assert.deepEqual(
    replay.entries.map((item) => item.sequence),
    [0, 1],
  );
  assert.deepEqual(
    appendActivity(replay.entries, replay.cursor, page([], 1)),
    replay,
  );
});

test("gaps and invalid cursors cannot advance past unseen events", () => {
  const existing = [entry(0)];
  assert.throws(
    () =>
      appendActivity(existing, 0, {
        ...page([entry(2)], 2),
        gap: { expected: 1, found: 2 },
      }),
    /missing/,
  );
  assert.throws(
    () => appendActivity(existing, 0, page([entry(2)], 2)),
    /out of order/,
  );
  assert.throws(
    () => appendActivity(existing, 0, page([entry(1)], 2)),
    /inconsistent/,
  );
  assert.throws(
    () =>
      appendActivity(
        [],
        null,
        page([entry(Number.MAX_SAFE_INTEGER + 1)], null),
      ),
    /invalid/,
  );
  assert.deepEqual(existing, [entry(0)]);
});

test("the display is bounded while the reconnect cursor keeps the full durable position", () => {
  const result = appendActivity(
    Array.from({ length: 500 }, (_, sequence) => entry(sequence)),
    499,
    page([entry(500), entry(501)], 501),
  );
  assert.equal(result.entries.length, 500);
  assert.equal(result.entries[0].sequence, 2);
  assert.equal(result.cursor, 501);
});

test("the rendered timeline uses semantic tool, terminal, and silent-completion activity", () => {
  assert.deepEqual(
    describeActivity({
      ...entry(0),
      activity: {
        kind: "tool_call",
        phase: { phase: "completed", result: { text: "3 files" } },
      },
    }),
    { title: "Tool completed", detail: "3 files" },
  );
  assert.deepEqual(
    describeActivity({
      ...entry(0),
      activity: {
        kind: "terminal",
        phase: { phase: "output", chunk: { text: "validated" } },
      },
    }),
    { title: "Command output", detail: "validated" },
  );
  assert.match(
    describeActivity({
      ...entry(0),
      activity: {
        kind: "lifecycle",
        phase: { phase: "completed", delivery_intent: "silent" },
      },
    }).detail,
    /without an Office reply/,
  );
});
