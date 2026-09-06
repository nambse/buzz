import assert from "node:assert/strict";
import { test } from "node:test";
import {
  setup,
  KEY,
  TEXT,
  deferred,
} from "./useMentionSendFlow.test-support.mjs";

function noLegacyEffects(s) {
  for (const event of ["add", "persona", "local-policy", "native"])
    assert.deepEqual(s.events(event), [], event);
}

test("private mention publishes through both authorization boundaries without legacy preparation, huddle or wake", async () => {
  const s = await setup({ privateMode: true, autoPrompt: false });
  // Cached local ownership and stale membership must not trigger attachment or
  // invitation. Fresh membership is owned by the real revalidation hook.
  s.query.data = [{ pubkey: KEY, name: "Ada", status: "stopped" }];
  s.options.onPrepareSendChannel = async () => {
    assert.fail("private mentions must not create or expand channels");
  };
  s.rerender();
  await s.prompt();
  assert.equal(s.result.current.nonMemberPromptProps.open, false);
  assert.equal(s.events("prepare").length, 1);
  assert.equal(s.events("publish").length, 1);
  assert.deepEqual(s.events("SEND")[0].slice(1, 3), [TEXT, [KEY]]);
  noLegacyEffects(s);
});

for (const scenario of ["dm", "unknown", "missing", "persona"])
  test(`private ${scenario} mention fails before preparation side effects`, async () => {
    const s = await setup({ privateMode: true, autoPrompt: false });
    if (scenario === "dm") s.options.channelType = "dm";
    if (scenario === "unknown") s.options.channelType = null;
    if (scenario === "missing") s.options.channelId = null;
    if (scenario === "persona")
      s.options.mentions.extractMentionPersonas = () => [
        { displayName: "Ada", persona: { id: "legacy" } },
      ];
    s.options.onPrepareSendChannel = async () =>
      assert.fail("must not prepare a DM");
    s.rerender();
    await s.prompt();
    assert.equal(s.events("SEND").length, 0);
    assert.equal(s.events("error").length, 1);
    assert.equal(s.options.contentRef.current, TEXT);
    noLegacyEffects(s);
  });

test("private publication revocation retains the draft and cannot enqueue a wake", async () => {
  const s = await setup({ privateMode: true, autoPrompt: false });
  const gate = deferred();
  s.control.publish = gate;
  const pending = s.prompt();
  await s.flush();
  assert.equal(s.events("prepare").length, 1);
  assert.equal(s.events("publish").length, 1);
  gate.reject(new Error("Office membership was revoked"));
  await pending;
  assert.equal(s.events("SEND").length, 0);
  assert.equal(s.options.contentRef.current, TEXT);
  noLegacyEffects(s);
});
