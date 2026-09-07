import { useCallback, useEffect, useRef, useState } from "react";
import { nativeDm } from "./native";
import type { Authority, Context, NativeDm, OpenView, Pending } from "./types";

interface Owner {
  key: string;
  context: Context;
  active: boolean;
  busy: boolean;
  sealing: boolean;
  loaded: boolean;
  revision: number;
  scope: string;
  draftVersion: number;
  text: string;
  dirty: boolean;
  pending: Pending | null;
  authority: Authority | null;
  heartbeat: ReturnType<typeof setTimeout> | null;
  expiry: ReturnType<typeof setTimeout> | null;
  readTimer: ReturnType<typeof setTimeout> | null;
  saveTimer: ReturnType<typeof setTimeout> | null;
}
interface State {
  key: string;
  view: OpenView | null;
  text: string;
  busy: boolean;
  sealing: boolean;
  error: string | null;
  note: string | null;
}
const empty = (key: string, error: string | null = null): State => ({
  key,
  view: null,
  text: "",
  busy: false,
  sealing: false,
  error,
  note: null,
});
function within(text: string) {
  if (text.length > 8192) return false;
  for (let index = 0; index < text.length; index += 1) {
    const code = text.charCodeAt(index);
    if (
      (code < 32 && code !== 9 && code !== 10 && code !== 13) ||
      (code >= 127 && code <= 159)
    )
      return false;
  }
  return new TextEncoder().encode(text).length <= 8192;
}

/** Standalone participant view. No ordinary draft hooks, event bus, localStorage,
 * React Query cache, notification, telemetry or optimistic message insertion. */
export function useConfidentialDm(
  selected: { channelId: string; human: string; relay: string } | null,
  native: NativeDm = nativeDm,
) {
  const channel = selected?.channelId ?? null;
  const human = selected?.human ?? null;
  const relay = selected?.relay ?? null;
  const identity =
    channel !== null && human !== null && relay !== null
      ? JSON.stringify([channel, human, relay])
      : "closed";
  const transportGeneration = useRef({ native, generation: 0 });
  if (transportGeneration.current.native !== native) {
    transportGeneration.current = {
      native,
      generation: transportGeneration.current.generation + 1,
    };
  }
  const [refresh, setRefresh] = useState(0);
  const key = `${identity}:${refresh}:${transportGeneration.current.generation}`;
  const latest = useRef(key);
  latest.current = key;
  const owner = useRef<Owner | null>(null);
  const [state, setState] = useState<State>(() => empty(key));
  const current = useCallback(
    (o: Owner) => o.active && owner.current === o && latest.current === o.key,
    [],
  );

  const clear = useCallback(
    (o: Owner, error: string | null) => {
      o.active = false;
      o.text = "";
      o.authority = null;
      o.pending = null;
      for (const timer of [o.heartbeat, o.expiry, o.readTimer, o.saveTimer])
        if (timer !== null) clearTimeout(timer);
      // Native close invalidates unfinished decrypt/send admission. Its result
      // cannot restore any volatile frontend state.
      void native<void>("encrypted_dm_close", {
        viewId: o.context.view_id,
      }).catch(() => {});
      if (latest.current === o.key) setState(empty(o.key, error));
    },
    [native],
  );
  const fresh = useCallback(
    (o: Owner, value: Authority) => {
      if (!current(o)) return false;
      const remaining = Date.parse(value.pair.valid_before) - Date.now();
      if (
        value.pair.channel_id !== o.context.channel_id ||
        value.pair.human_public_key !== o.context.expected_human ||
        !Number.isFinite(remaining) ||
        remaining <= 0 ||
        remaining > 15000 ||
        (o.scope && value.scope !== o.scope)
      ) {
        clear(o, "Access changed. Refresh to check this pair again.");
        return false;
      }
      o.scope = value.scope;
      o.authority = value;
      if (o.expiry !== null) clearTimeout(o.expiry);
      o.expiry = setTimeout(
        () =>
          clear(
            o,
            "Current access could not be confirmed. Refresh to continue.",
          ),
        remaining,
      );
      return true;
    },
    [clear, current],
  );
  const failure = useCallback(
    (o: Owner) => {
      if (current(o))
        clear(
          o,
          "This private operation could not be confirmed. Refresh to recover its protected draft or retained send.",
        );
    },
    [clear, current],
  );

  useEffect(() => {
    setState(empty(key));
    if (channel === null || human === null || relay === null) return;
    const o: Owner = {
      key,
      context: {
        view_id: crypto.randomUUID(),
        channel_id: channel,
        expected_human: human,
        expected_relay: relay,
      },
      active: true,
      busy: false,
      sealing: false,
      loaded: false,
      revision: 0,
      scope: "",
      draftVersion: 0,
      text: "",
      dirty: false,
      pending: null,
      authority: null,
      heartbeat: null,
      expiry: null,
      readTimer: null,
      saveTimer: null,
    };
    owner.current = o;
    const heartbeat = async () => {
      if (!current(o)) return;
      try {
        const value = await native<Authority>("encrypted_dm_authority", {
          context: o.context,
        });
        if (fresh(o, value)) o.heartbeat = setTimeout(heartbeat, 2000);
      } catch {
        failure(o);
      }
    };
    let reads = 0;
    const read = async () => {
      if (!current(o)) return;
      if (o.busy || o.sealing) {
        o.readTimer = setTimeout(read, 1000);
        return;
      }
      try {
        const revision = o.revision;
        const value = await native<OpenView>("encrypted_dm_open", {
          context: o.context,
          expectedScope: o.scope,
        });
        if (!current(o)) return;
        if (o.busy || o.sealing || revision !== o.revision) {
          o.readTimer = setTimeout(read, 1000);
          return;
        }
        if (value.scope !== o.scope) {
          failure(o);
          return;
        }
        // A read started before an edit must never overwrite that edit. Dirty
        // input is kept only in this scoped volatile owner until native sealing.
        if (!o.loaded) {
          o.text = value.draft.text;
          o.draftVersion = value.draft.version;
          o.loaded = true;
        }
        o.pending = value.pending;
        setState({
          key,
          view: { ...value, draft: { ...value.draft, text: "" } },
          text: o.text,
          busy: false,
          sealing: false,
          error: null,
          note: null,
        });
        reads += 1;
        if (reads < 12) o.readTimer = setTimeout(read, 5000);
        else
          setState((s) =>
            s.key === key
              ? {
                  ...s,
                  note: "Live message refresh paused. Refresh messages to continue.",
                }
              : s,
          );
      } catch {
        failure(o);
      }
    };
    void native<Authority>("encrypted_dm_begin", { context: o.context })
      .then((value) => {
        if (!fresh(o, value)) return;
        o.heartbeat = setTimeout(heartbeat, 2000);
        void read();
      })
      .catch(() => failure(o));
    const hidden = () => {
      if (document.visibilityState === "hidden")
        clear(o, "Private view hidden. Refresh to reopen.");
    };
    const blur = () => clear(o, "Private view locked. Refresh to reopen.");
    document.addEventListener("visibilitychange", hidden);
    window.addEventListener("blur", blur);
    return () => {
      clear(o, null);
      document.removeEventListener("visibilitychange", hidden);
      window.removeEventListener("blur", blur);
    };
  }, [key, native, channel, human, relay, clear, current, fresh, failure]);

  async function save(o: Owner) {
    o.revision += 1;
    const exact = o.text;
    const version = await native<number>("encrypted_dm_save_draft", {
      context: o.context,
      expectedScope: o.scope,
      version: o.draftVersion,
      text: exact,
    });
    if (!current(o)) return;
    o.draftVersion = version;
    if (o.text === exact) o.dirty = false;
  }
  function scheduleSave(o: Owner) {
    if (o.saveTimer !== null) clearTimeout(o.saveTimer);
    o.saveTimer = setTimeout(async () => {
      if (!current(o) || o.busy || o.sealing) return;
      o.sealing = true;
      setState((s) => (s.key === key ? { ...s, sealing: true } : s));
      try {
        await save(o);
      } catch {
        failure(o);
      } finally {
        o.sealing = false;
        if (current(o)) {
          setState((s) => (s.key === key ? { ...s, sealing: false } : s));
          if (o.dirty) scheduleSave(o);
        }
      }
    }, 400);
  }
  function edit(text: string) {
    const o = owner.current;
    if (!o || !current(o) || o.busy || o.pending || !within(text)) return;
    o.text = text;
    o.dirty = true;
    setState((s) => (s.key === key ? { ...s, text } : s));
    scheduleSave(o);
  }
  async function send(retry: boolean) {
    const o = owner.current;
    if (!o || !current(o) || o.busy || o.sealing || !o.scope) return;
    if (retry ? !o.pending : o.pending || !o.text.trim() || !within(o.text))
      return;
    if (o.pending && o.pending.scope !== o.scope) return;
    o.busy = true;
    o.revision += 1;
    if (o.saveTimer !== null) clearTimeout(o.saveTimer);
    setState((s) => (s.key === key ? { ...s, busy: true, error: null } : s));
    try {
      if (!retry) {
        if (o.dirty || o.draftVersion === 0) await save(o);
        if (!current(o)) return;
        o.pending = await native<Pending>("encrypted_dm_prepare", {
          context: o.context,
          expectedScope: o.scope,
          operationId: crypto.randomUUID(),
          draftVersion: o.draftVersion,
          text: o.text,
        });
      }
      if (!current(o) || !o.pending) return;
      const receipt = await native<Pending>("encrypted_dm_publish", {
        context: o.context,
        expectedScope: o.scope,
        operationId: o.pending.operation_id,
      });
      if (!current(o)) return;
      if (receipt.retired_at !== null || !receipt.acknowledged.every(Boolean))
        throw new Error("Unconfirmed encrypted send");
      o.pending = null;
      o.text = "";
      o.dirty = false;
      o.draftVersion = 0;
      setState((s) =>
        s.key === key
          ? {
              ...s,
              text: "",
              view: s.view ? { ...s.view, pending: null } : null,
              note: "Both encrypted copies were acknowledged.",
            }
          : s,
      );
    } catch {
      failure(o);
    } finally {
      o.busy = false;
      if (current(o))
        setState((s) => (s.key === key ? { ...s, busy: false } : s));
    }
  }
  async function retire() {
    const o = owner.current;
    if (!o || !current(o) || o.busy || o.sealing || !o.pending || !o.scope)
      return;
    const pending = o.pending;
    o.busy = true;
    o.revision += 1;
    if (o.saveTimer !== null) clearTimeout(o.saveTimer);
    setState((s) => (s.key === key ? { ...s, busy: true, error: null } : s));
    try {
      const receipt = await native<Pending>("encrypted_dm_retire", {
        context: o.context,
        expectedScope: o.scope,
        originalScope: pending.scope,
        operationId: pending.operation_id,
      });
      if (!current(o)) return;
      if (
        receipt.operation_id !== pending.operation_id ||
        receipt.scope !== pending.scope ||
        receipt.rumor_id !== pending.rumor_id ||
        receipt.outer_ids.some((id, i) => id !== pending.outer_ids[i]) ||
        receipt.acknowledged.some(
          (ack, i) => ack !== pending.acknowledged[i],
        ) ||
        receipt.retired_at === null ||
        !Number.isSafeInteger(receipt.retired_at)
      )
        throw new Error("Unconfirmed encrypted retirement");
      // Reopen the current protected draft from native after exact retirement.
      // A lost response follows the same refresh path; neither path auto-sends.
      clear(o, null);
      setRefresh((v) => v + 1);
    } catch {
      failure(o);
    } finally {
      o.busy = false;
      if (current(o))
        setState((s) => (s.key === key ? { ...s, busy: false } : s));
    }
  }
  const visible = state.key === key && selected !== null;
  return {
    ...(visible ? state : empty(key)),
    edit,
    send: () => send(false),
    retry: () => send(true),
    retire,
    refresh: () => setRefresh((v) => v + 1),
  };
}
