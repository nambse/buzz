import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() => {
  for (const name of [
    "document",
    "HTMLElement",
    "HTMLTextAreaElement",
    "Element",
    "Node",
    "Event",
    "MouseEvent",
  ])
    Object.defineProperty(globalThis, name, {
      value: dom.window[name],
      configurable: true,
    });
  globalThis.window = dom.window;
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
});
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const selected = {
  channelId: "11111111-1111-4111-8111-111111111111",
  human: "a".repeat(64),
  relay: "ws://127.0.0.1:8080",
};
function authority(context) {
  return {
    scope: `scope-${context.channel_id}`,
    pair: {
      format: "ortak-native-encrypted-dm-authority/1",
      channel_id: context.channel_id,
      human_public_key: context.expected_human,
      employee_id: "deniz-private",
      employee_public_key: "b".repeat(64),
      valid_before: new Date(Date.now() + 5000).toISOString(),
      observed_at: new Date().toISOString(),
      selection_generation: "9223372036854775807",
      office_generation: "0",
      authority_epoch: "0",
      key_version: "0",
    },
  };
}
async function setup({
  delayOpen = false,
  lostAck = false,
  oldScope = false,
  lostRetirement = false,
} = {}) {
  const React = await import("react");
  const { render, fireEvent, waitFor, act } = await import(
    "@testing-library/react"
  );
  const { ConfidentialDm } = await import("./ConfidentialDm.tsx");
  const calls = [];
  let pending = oldScope
    ? {
        operation_id: "22222222-2222-4222-8222-222222222222",
        scope: "old-authority-scope",
        rumor_id: "d".repeat(64),
        outer_ids: ["e".repeat(64), "f".repeat(64)],
        acknowledged: [true, false],
        retired_at: null,
      }
    : null;
  const retired = [];
  let draft = { version: 0, text: "" };
  let unblock;
  let selectedNow = selected;
  const native = async (name, args) => {
    calls.push({ name, args: structuredClone(args) });
    if (name === "encrypted_dm_close") return;
    const current = authority(args.context);
    if (name === "encrypted_dm_begin" || name === "encrypted_dm_authority")
      return current;
    if (name === "encrypted_dm_open") {
      if (delayOpen)
        await new Promise((resolve) => {
          unblock = resolve;
        });
      return {
        ...current,
        draft,
        pending,
        retired: [...retired],
        limited: false,
        withheld_count: 0,
        messages: [
          {
            rumor_id: "c".repeat(64),
            sender: "b".repeat(64),
            text: `private ${args.context.channel_id}`,
            created_at: 1,
            reply_to: null,
          },
        ],
      };
    }
    if (name === "encrypted_dm_save_draft") {
      assert.equal(args.version, draft.version);
      draft = { version: draft.version + 1, text: args.text };
      return draft.version;
    }
    if (name === "encrypted_dm_prepare") {
      assert.equal(args.draftVersion, draft.version);
      assert.equal(args.text, draft.text);
      assert.equal(pending, null);
      pending = {
        operation_id: args.operationId,
        scope: args.expectedScope,
        rumor_id: "d".repeat(64),
        outer_ids: ["e".repeat(64), "f".repeat(64)],
        acknowledged: [false, false],
        retired_at: null,
      };
      return pending;
    }
    if (name === "encrypted_dm_publish") {
      assert.equal(args.operationId, pending.operation_id);
      assert.equal("text" in args, false);
      if (lostAck) {
        lostAck = false;
        pending.acknowledged = [true, false];
        throw new Error("synthetic private exception must never render");
      }
      const result = { ...pending, acknowledged: [true, true] };
      pending = null;
      draft = { version: 0, text: "" };
      return result;
    }
    if (name === "encrypted_dm_retire") {
      assert.equal(args.operationId, pending.operation_id);
      assert.equal(args.originalScope, pending.scope);
      assert.equal(args.expectedScope, current.scope);
      assert.equal("text" in args, false);
      const receipt = { ...pending, retired_at: 1_800_000_000 };
      retired.push(receipt);
      pending = null;
      draft = { version: 0, text: "" };
      if (lostRetirement) {
        lostRetirement = false;
        throw new Error("synthetic retirement response loss");
      }
      return receipt;
    }
    throw new Error(`Unexpected purpose command ${name}`);
  };
  const component = () =>
    React.createElement(ConfidentialDm, {
      selected: selectedNow,
      employeeName: "Deniz",
      native,
    });
  const view = render(component());
  return {
    view,
    calls,
    fireEvent,
    act,
    waitFor,
    unblock: () => {
      delayOpen = false;
      unblock?.();
    },
    select: (value) => {
      selectedNow = value;
      view.rerender(component());
    },
  };
}

test("real composer freezes protected draft before send; no ordinary cache or plaintext optimistic message", async () => {
  const x = await setup();
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("textbox", { name: "Encrypted message" })),
  );
  await x.act(async () =>
    x.fireEvent.change(x.view.getByRole("textbox"), {
      target: { value: "only volatile text" },
    }),
  );
  for (const code of [0, 8, 11, 12, 14, 31, 127, 159]) {
    await x.act(async () =>
      x.fireEvent.change(x.view.getByRole("textbox"), {
        target: { value: `refused${String.fromCharCode(code)}text` },
      }),
    );
    assert.equal(x.view.getByRole("textbox").value, "only volatile text");
  }
  await x.act(async () =>
    x.fireEvent.submit(
      x.view.getByRole("form", { name: "Send encrypted message" }),
    ),
  );
  await x.waitFor(() =>
    assert.ok(x.view.getByText("Both encrypted copies were acknowledged.")),
  );
  assert.deepEqual(
    x.calls
      .filter((c) => /save_draft|prepare|publish/.test(c.name))
      .map((c) => c.name),
    ["encrypted_dm_save_draft", "encrypted_dm_prepare", "encrypted_dm_publish"],
  );
  assert.equal(x.view.queryByText("only volatile text"), null);
  assert.equal(window.localStorage.length, 0);
});

test("equivalent selected props keep the native view and draft; a changed human retires it", async () => {
  const x = await setup();
  await x.waitFor(() => assert.ok(x.view.getByRole("textbox")));
  await x.act(async () =>
    x.fireEvent.change(x.view.getByRole("textbox"), {
      target: { value: "same scoped draft" },
    }),
  );
  await x.act(async () => x.select({ ...selected }));
  assert.equal(x.view.getByRole("textbox").value, "same scoped draft");
  assert.equal(
    x.calls.filter((c) => c.name === "encrypted_dm_begin").length,
    1,
  );
  assert.equal(
    x.calls.filter((c) => c.name === "encrypted_dm_close").length,
    0,
  );
  const previous = x.calls.find((c) => c.name === "encrypted_dm_begin").args
    .context;
  const nextHuman = "9".repeat(64);
  await x.act(async () => x.select({ ...selected, human: nextHuman }));
  await x.waitFor(() =>
    assert.equal(
      x.calls.filter((c) => c.name === "encrypted_dm_begin").length,
      2,
    ),
  );
  const replacement = x.calls.filter((c) => c.name === "encrypted_dm_begin")[1]
    .args.context;
  assert.equal(replacement.expected_human, nextHuman);
  assert.notEqual(replacement.view_id, previous.view_id);
  assert.ok(
    x.calls.some(
      (c) =>
        c.name === "encrypted_dm_close" && c.args.viewId === previous.view_id,
    ),
  );
});

test("lost ACK clears text and recovers same retained operation without a new prepare", async () => {
  const x = await setup({ lostAck: true });
  await x.waitFor(() => assert.ok(x.view.getByRole("textbox")));
  await x.act(async () =>
    x.fireEvent.change(x.view.getByRole("textbox"), {
      target: { value: "uncertain private text" },
    }),
  );
  await x.act(async () => x.fireEvent.submit(x.view.getByRole("form")));
  await x.waitFor(() => assert.ok(x.view.getByRole("alert")));
  assert.equal(x.view.queryByRole("textbox"), null);
  assert.equal(x.view.queryByText(/synthetic private exception/), null);
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("button", { name: "Refresh messages" })),
  );
  await x.waitFor(() =>
    assert.ok(
      x.view.getByRole("button", { name: "Retry retained encrypted send" }),
    ),
  );
  assert.equal(x.view.queryByRole("form"), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry retained encrypted send" }),
    ),
  );
  await x.waitFor(() =>
    assert.ok(x.view.getByText("Both encrypted copies were acknowledged.")),
  );
  const sends = x.calls.filter((c) => c.name === "encrypted_dm_publish");
  assert.equal(sends.length, 2);
  assert.equal(sends[0].args.operationId, sends[1].args.operationId);
  assert.equal(
    x.calls.filter((c) => c.name === "encrypted_dm_prepare").length,
    1,
  );
});

test("late decrypted read after account/pair removal never reappears and closes native view", async () => {
  const x = await setup({ delayOpen: true });
  await x.waitFor(() =>
    assert.ok(x.calls.some((c) => c.name === "encrypted_dm_open")),
  );
  await x.act(async () => x.select(null));
  await x.act(async () => x.unblock());
  assert.equal(x.view.queryByText(`private ${selected.channelId}`), null);
  assert.equal(x.view.queryByRole("textbox"), null);
  assert.ok(x.calls.some((c) => c.name === "encrypted_dm_close"));
});

test("blur removes decrypted messages and draft without ordinary draft persistence", async () => {
  const x = await setup();
  await x.waitFor(() => assert.ok(x.view.getByRole("textbox")));
  await x.act(async () =>
    x.fireEvent.change(x.view.getByRole("textbox"), {
      target: { value: "unsaved volatile text" },
    }),
  );
  await x.act(async () => window.dispatchEvent(new window.Event("blur")));
  assert.equal(x.view.queryByText(`private ${selected.channelId}`), null);
  assert.equal(x.view.queryByRole("textbox"), null);
  assert.equal(window.localStorage.length, 0);
  assert.ok(x.calls.some((c) => c.name === "encrypted_dm_close"));
});

test("old-scope retirement is explicit, keeps receipt through response loss, and never retries the old send", async () => {
  for (const lostRetirement of [false, true]) {
    const x = await setup({ oldScope: true, lostRetirement });
    await x.waitFor(() =>
      assert.ok(
        x.view.getByRole("button", {
          name: "Keep old send and start new draft",
        }),
      ),
    );
    assert.equal(x.view.queryByRole("form"), null);
    assert.equal(
      x.view.queryByRole("button", { name: "Retry retained encrypted send" }),
      null,
    );
    assert.ok(x.view.getByText(/This send may already have been delivered/));
    assert.equal(
      x.calls.some((c) => /prepare|publish|retire/.test(c.name)),
      false,
    );
    await x.act(async () =>
      x.fireEvent.click(
        x.view.getByRole("button", {
          name: "Keep old send and start new draft",
        }),
      ),
    );
    if (lostRetirement) {
      await x.waitFor(() => assert.ok(x.view.getByRole("alert")));
      assert.equal(x.view.queryByRole("textbox"), null);
      await x.act(async () =>
        x.fireEvent.click(
          x.view.getByRole("button", {
            name: "Refresh messages",
          }),
        ),
      );
    }
    await x.waitFor(() => assert.ok(x.view.getByRole("textbox")));
    assert.equal(x.view.getByRole("textbox").value, "");
    const history = x.view.getByRole("list", {
      name: "Retained encrypted sends",
    });
    assert.match(history.textContent, /22222222-2222-4222-8222-222222222222/);
    assert.match(history.textContent, /acknowledged copies 1\/2/);
    assert.equal(
      x.calls.filter((c) => c.name === "encrypted_dm_retire").length,
      1,
    );
    assert.equal(
      x.calls.some((c) => /prepare|publish|save_draft/.test(c.name)),
      false,
    );
    await x.act(async () =>
      x.fireEvent.change(x.view.getByRole("textbox"), {
        target: { value: "explicit fresh draft" },
      }),
    );
    await x.act(async () => x.fireEvent.submit(x.view.getByRole("form")));
    await x.waitFor(() =>
      assert.ok(x.view.getByText("Both encrypted copies were acknowledged.")),
    );
    const newSend = x.calls.find((c) => c.name === "encrypted_dm_prepare");
    assert.notEqual(
      newSend.args.operationId,
      "22222222-2222-4222-8222-222222222222",
    );
    assert.equal(newSend.args.expectedScope, `scope-${selected.channelId}`);
    assert.equal(
      x.calls.filter((c) => c.name === "encrypted_dm_publish").length,
      1,
    );
    await x.act(async () => x.view.unmount());
  }
});
