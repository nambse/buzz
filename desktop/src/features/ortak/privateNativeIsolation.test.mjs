import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { register } from "node:module";
import test, { after, afterEach } from "node:test";
import { JSDOM } from "jsdom";

register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  if (url.endsWith('/features/messages/ui/ComposerEmojiPicker.tsx')) return {
    format: 'module', shortCircuit: true,
    source: 'export const ComposerEmojiPicker = () => null;'
  };
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/privateMode.ts')) result.source = String(result.source)
    .replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE', '"true"');
  return result;
}`)}`,
  import.meta.url,
);

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const nativeCalls = [];
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  HTMLElement: dom.window.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: true,
});
const policy = readFileSync(
  new URL("../../../src-tauri/src/private_native.rs", import.meta.url),
  "utf8",
);
const allowlist = new Set(
  [
    ...policy
      .split("const OFFICE_COMMANDS")[1]
      .split("];", 1)[0]
      .matchAll(/"([a-z_]+)"/g),
  ].map((m) => m[1]),
);
window.__TAURI_INTERNALS__ = {
  invoke: async (command, input) => {
    nativeCalls.push(command);
    assert(
      allowlist.has(command),
      `native Office admission missing ${command}`,
    );
    if (command === "get_relay_http_url") return "http://localhost:3038";
    if (command === "get_media_proxy_port") return 0;
    assert.equal(
      command,
      "sign_event",
      "voice/provider hooks must not invoke native commands",
    );
    return JSON.stringify({
      ...input,
      id: "fixture",
      pubkey: "fixture",
      sig: "fixture",
    });
  },
};
const { createElement: h } = await import("react");
const { act, render, renderHook, cleanup } = await import(
  "@testing-library/react"
);
afterEach(() => {
  cleanup();
  nativeCalls.length = 0;
});
after(() => dom.window.close());

test("private channel huddle and wave affordances mount without audio, relay, or provider hooks", async () => {
  const { HuddleProvider, useHuddle, useHuddleLevels } = await import(
    "../huddle/HuddleContext.tsx"
  );
  const { HuddlePresenceProvider } = await import(
    "../huddle/HuddlePresenceContext.tsx"
  );
  const { HuddleIndicator } = await import(
    "../huddle/components/HuddleIndicator.tsx"
  );
  const { WaveMessageAttachment } = await import(
    "../messages/ui/WaveMessageAttachment.tsx"
  );
  let state;
  function Consumer() {
    state = useHuddle();
    assert.equal(useHuddleLevels().micLevel, 0);
    return h("p", null, "Office retained");
  }
  const view = render(
    h(
      HuddlePresenceProvider,
      null,
      h(
        HuddleProvider,
        null,
        h(Consumer),
        h(HuddleIndicator, { channelId: "selected", onStart: assert.fail }),
        h(WaveMessageAttachment, {
          channelId: "selected",
          fallbackText: "Merhaba",
        }),
      ),
    ),
  );
  assert(view.getByText("Office retained"));
  assert(view.getByText("Merhaba"));
  assert.equal(view.queryByRole("button", { name: /huddle/i }), null);
  assert.equal(state.activeEphemeralChannelId, null);
  assert.equal(state.micConnected, false);
  await assert.rejects(state.startHuddle("selected", []), /unavailable/);
  await assert.rejects(state.joinHuddle("selected", "old"), /unavailable/);
  assert.equal(await state.leaveHuddle(), true);
  assert(
    nativeCalls.every((command) =>
      ["get_relay_http_url", "get_media_proxy_port"].includes(command),
    ),
  );
});

test("private Ctrl Shift Space is not captured or converted into a huddle command", async () => {
  const { useAppShellKeyboardShortcuts } = await import(
    "../../app/useAppShellKeyboardShortcuts.ts"
  );
  const events = [];
  const listener = (event) => events.push(event);
  window.addEventListener("buzz:huddle-shortcut", listener);
  const noop = () => {};
  renderHook(() =>
    useAppShellKeyboardShortcuts({
      activeChannelId: "selected",
      canSearchCurrentChannel: true,
      disabled: false,
      onBrowseChannels: noop,
      onCreateChannel: noop,
      onGoHome: noop,
      onNewMessage: noop,
      onSearchCurrentChannel: noop,
      onSearchEverything: noop,
    }),
  );
  const event = new window.KeyboardEvent("keydown", {
    ctrlKey: true,
    shiftKey: true,
    code: "Space",
    key: " ",
    bubbles: true,
    cancelable: true,
  });
  window.dispatchEvent(event);
  assert.equal(event.defaultPrevented, false);
  assert.deepEqual(events, []);
  window.removeEventListener("buzz:huddle-shortcut", listener);
});

test("private Office omits voice recording and direct recorder admission never requests a microphone", async () => {
  const { MessageComposerToolbar } = await import(
    "../messages/ui/MessageComposerToolbar.tsx"
  );
  const { TooltipProvider } = await import("../../shared/ui/tooltip.tsx");
  const { useVoiceNoteRecorder } = await import(
    "../messages/lib/useVoiceNoteRecorder.ts"
  );
  const noop = () => {};
  const view = render(
    h(
      TooltipProvider,
      null,
      h(MessageComposerToolbar, {
        composerDisabled: false,
        editor: null,
        formattingDisabled: false,
        gifMediaController: { setPendingImeta: noop },
        isEmojiPickerOpen: false,
        isFormattingOpen: false,
        isSending: false,
        isUploading: false,
        onCaptureSelection: noop,
        onEmojiPickerOpenChange: noop,
        onEmojiSelect: noop,
        onFormattingToggle: noop,
        onLinkButton: noop,
        onOpenMentionPicker: noop,
        onPaperclip: noop,
        onVoiceNote: assert.fail,
        sendDisabled: false,
      }),
    ),
  );
  assert.equal(view.queryByRole("button", { name: "Record voice note" }), null);
  assert(view.getByRole("button", { name: "Attach file" }));
  const original = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const originalRecorder = window.MediaRecorder;
  window.MediaRecorder = class FixtureRecorder {};
  let microphoneRequests = 0;
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      mediaDevices: {
        getUserMedia: async () => {
          microphoneRequests++;
          throw new Error("fixture microphone");
        },
      },
    },
  });
  const recorder = renderHook(() => useVoiceNoteRecorder());
  try {
    await act(async () => {
      await recorder.result.current.start();
    });
    assert.equal(recorder.result.current.status, "idle");
    assert.equal(
      recorder.result.current.error,
      "Voice recording is unavailable in this Ortak build.",
    );
    assert.equal(microphoneRequests, 0);
    assert(nativeCalls.every((command) => command === "get_media_proxy_port"));
  } finally {
    window.MediaRecorder = originalRecorder;
    if (original) Object.defineProperty(globalThis, "navigator", original);
    else delete globalThis.navigator;
  }
});

test("actual Ortak client uses the admitted native signer for signed management and run queries", async () => {
  const { signRelayEvent } = await import("@/shared/api/tauri.ts");
  const { createOrtakClient } = await import("./client.ts");
  const requests = [];
  const client = createOrtakClient(
    "http://127.0.0.1:8787",
    signRelayEvent,
    async (url, init) => {
      requests.push({
        url,
        authorization: new Headers(init.headers).get("Authorization"),
      });
      return Response.json([]);
    },
  );
  const signal = new AbortController().signal;
  await client.employees(signal);
  await client.preparedEmployees(signal);
  await client.runs(signal);
  await client.detail("run-fixture", signal);
  assert.equal(requests.length, 4);
  assert(
    requests.every((request) => request.authorization?.startsWith("Nostr ")),
  );
  assert.deepEqual(nativeCalls, Array(4).fill("sign_event"));
});
