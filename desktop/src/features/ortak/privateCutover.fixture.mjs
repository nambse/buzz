import assert from "node:assert/strict";
import { register } from "node:module";
import test, { after, afterEach } from "node:test";
import { JSDOM } from "jsdom";

// The real entry components, policy, action hook, shortcuts and terminal store
// are retained. Only child visuals and external I/O are replaced. Each mode
// runs in its own test process, like Vite's immutable build-time selection.
export async function cutoverSuite(privateMode) {
  const leaves = {
    "/features/chat/ui/ChatHeader.tsx": `export const ChatHeader = ({actions}) => actions;`,
    "/features/channels/ui/ChannelMembersBar.tsx": `export const ChannelMembersBar = () => null;`,
    "/features/channels/ui/ChannelGlyph.tsx": `export const ChannelGlyph = () => null;`,
    "/features/channels/ui/ChannelHeaderStatusBadge.tsx": `export const ChannelHeaderStatusBadge = () => null;`,
    "/features/profile/ui/ProfileAvatarWithStatus.tsx": `
      export const ProfileAvatarWithStatus = () => null;
      export const DEFAULT_HOVER_PROFILE_STATUS_GEOMETRY = {};
      export const scaleProfileAvatarStatusGeometry = () => ({});`,
    "/features/profile/ui/UserProfilePopover.tsx": `export const UserProfilePopover = ({children}) => children;`,
    "/features/agents/ui/OtherSetupAgentMarker.tsx": `export const AgentManagementMarker = () => null;`,
    "/features/user-status/ui/UserNameIndicators.tsx": `export const UserNameIndicators = () => null;`,
    "/shared/ui/UserAvatar.tsx": `export const UserAvatar = () => null;`,
    "/features/terminal/TerminalSubstrate.tsx": `
      import { createElement, useEffect } from 'react';
      export function TerminalSubstrate() {
        useEffect(() => { globalThis.__ortakCutover.substrates++; }, []);
        return createElement('div', {'data-testid': 'terminal-substrate'});
      }`,
    "/app/navigation/useAppNavigation.ts": `
      const goChannel = async (id) => { globalThis.__ortakCutover.actions.push(['navigate', id]); };
      export const useAppNavigation = () => ({goChannel});`,
    "/features/channels/hooks.ts": `
      const data = [];
      const mutateAsync = async (input) => {
        globalThis.__ortakCutover.actions.push(['dm', input]); return {id: 'dm-fixture'};
      };
      export const channelsQueryKey = ['channels'];
      export const useChannelsQuery = () => ({data});
      export const useOpenDmMutation = () => ({mutateAsync});`,
    "/features/huddle/index.ts": `
      const startHuddle = async (...args) => { globalThis.__ortakCutover.actions.push(['huddle', ...args]); };
      export const useHuddle = () => ({isStarting: false, startHuddle});`,
    "/features/messages/hooks.ts": `
      export const createOptimisticMessage = () => { throw new Error('unexpected wave'); };
      export const mergeTimelineCacheMessages = () => { throw new Error('unexpected wave'); };`,
    "/features/profile/hooks.ts": `export const useProfileQuery = () => ({data: null});`,
    "/shared/api/hooks.ts": `export const useIdentityQuery = () => ({data: null});`,
    "/features/community-members/hooks.ts": `
      export const useMyRelayMembershipLookupQuery = () => ({data: undefined});`,
    "/shared/features/index.ts": `
      const snapshot = {};
      export const useFeatureSnapshot = () => snapshot;
      export const getFeature = () => undefined;
      export const resolveEnabled = () => true;`,
    "/shared/ui/sidebar.tsx": `
      import {createElement} from 'react';
      const Box = ({children}) => createElement('div', null, children);
      export const Sidebar = Box, SidebarContent = Box, SidebarFooter = Box,
        SidebarGroup = Box, SidebarGroupContent = Box, SidebarGroupLabel = Box,
        SidebarHeader = Box, SidebarInset = Box, SidebarMenu = Box, SidebarMenuItem = Box;
      export const SidebarMenuButton = ({children, onClick, type, 'data-testid': id}) =>
        createElement('button', {onClick, type, 'data-testid': id}, children);
      const setOpen = () => {};
      export const useSidebar = () => ({isMobile: false, open: true, setOpen});`,
    "/features/settings/ui/SettingsPanels.tsx": `
      import {createElement, useEffect} from 'react';
      export const DEFAULT_SETTINGS_SECTION = 'appearance';
      export const settingsSections = ['appearance', 'voice', 'mobile'].map(value =>
        ({value, label: value, icon: () => null}));
      function Panel({section}) {
        useEffect(() => { globalThis.__ortakCutover.settings.push(section); }, [section]);
        return createElement('p', null, 'Mounted ' + section);
      }
      export const renderSettingsSection = section => createElement(Panel, {section});`,
  };
  register(
    `data:text/javascript,${encodeURIComponent(`
      const leaves = ${JSON.stringify(leaves)};
      export async function load(url, context, nextLoad) {
        const leaf = Object.entries(leaves).find(([suffix]) => url.endsWith(suffix));
        if (leaf) return {format:'module', shortCircuit:true, source:leaf[1]};
        const result = await nextLoad(url, context);
        if (url.endsWith('/features/ortak/privateMode.ts')) result.source = String(result.source)
          .replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE', '${JSON.stringify(String(privateMode))}');
        return result;
      }
    `)}`,
    import.meta.url,
  );
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: "http://localhost",
    pretendToBeVisual: true,
  });
  const calls = [];
  const observed = { actions: [], settings: [], substrates: 0 };
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    isTauri: false,
    __ortakCutover: observed,
  });
  window.__TAURI_INTERNALS__ = {
    invoke: async (command) => {
      calls.push(command);
      assert.equal(
        command,
        "plugin:app|version",
        "no terminal/provider native invocation",
      );
      return "fixture";
    },
  };
  const { createElement: h } = await import("react");
  const { act, render, renderHook, cleanup, fireEvent } = await import(
    "@testing-library/react"
  );
  const panel = await import("../terminal/terminalPanelStore.ts");
  afterEach(() => {
    cleanup();
    panel.resetTerminalPanelForTests();
    observed.actions.length = 0;
    observed.settings.length = 0;
    observed.substrates = 0;
    calls.length = 0;
  });
  after(() => dom.window.close());
  const mode = privateMode ? "private" : "legacy";

  test(`${mode}: real channel header terminal button follows product selection`, async () => {
    const { ChannelScreenHeader } = await import(
      "../channels/ui/ChannelScreenHeader.tsx"
    );
    const view = render(
      h(ChannelScreenHeader, {
        activeChannel: {
          id: "stream",
          channelType: "stream",
          name: "Office",
          isMember: false,
          visibility: "open",
          archivedAt: null,
        },
        activeChannelEphemeralDisplay: null,
        activeChannelTitle: "Office",
        activeDmAvatarUrl: null,
        activeDmHeaderParticipants: [],
        activeDmPresenceStatus: null,
        onJoinChannel: async () => {},
        onManageChannel: () => {},
        onToggleMembers: () => {},
      }),
    );
    assert(view.getByRole("button", { name: "Join" }));
    const terminal = view.queryByRole("button", { name: "Open Buzz Term" });
    if (privateMode) assert.equal(terminal, null);
    else {
      assert(terminal);
      fireEvent.click(terminal);
      assert.equal(panel.getTerminalPanelSnapshotForTests().mode, "docked");
    }
    assert.deepEqual(calls, []);
  });

  test(`${mode}: actual terminal bootstrap owns Cmd-J only in legacy and cannot revive private stale state`, async () => {
    const { TerminalBootstrap } = await import(
      "../terminal/TerminalBootstrap.tsx"
    );
    const props = {
      channelId: "stream",
      channelName: "Office",
      threadId: null,
      npub: "fixture",
      relayUrl: "ws://fixture.invalid",
    };
    const view = render(h(TerminalBootstrap, props));
    for (const type of ["keydown", "keyup"]) {
      const event = new window.KeyboardEvent(type, {
        code: "KeyJ",
        key: "j",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      });
      act(() => window.dispatchEvent(event));
      assert.equal(event.defaultPrevented, !privateMode);
    }
    assert.equal(
      panel.getTerminalPanelSnapshotForTests().mode,
      privateMode ? "closed" : "docked",
    );
    if (privateMode) {
      view.unmount();
      panel.setTerminalPanelMode("maximized");
      const stale = render(h(TerminalBootstrap, props));
      assert.equal(stale.queryByTestId("terminal-substrate"), null);
      assert.equal(observed.substrates, 0);
    } else assert(view.getByTestId("terminal-substrate"));
    assert.deepEqual(calls, []);
  });

  test(`${mode}: real profile Huddle handler gates before DM creation and preserves Message`, async () => {
    const { QueryClient, QueryClientProvider } = await import(
      "@tanstack/react-query"
    );
    const { useProfileInteractionActions } = await import(
      "../profile/ui/useProfileInteractionActions.ts"
    );
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }) =>
      h(QueryClientProvider, { client }, children);
    const hook = renderHook(
      () =>
        useProfileInteractionActions({
          availability: { huddle: true, message: true, wave: false },
          effectivePubkey: "a".repeat(64),
          enabled: true,
          isBot: false,
          isSelf: false,
          viewerIsOwner: false,
          onClose: () => observed.actions.push(["close"]),
        }),
      { wrapper },
    );
    assert.equal(hook.result.current.canHuddle, !privateMode);
    assert.equal(hook.result.current.canMessage, true);
    await act(async () => hook.result.current.handleHuddle());
    assert.deepEqual(
      observed.actions.map((row) => row[0]),
      privateMode ? [] : ["dm", "navigate", "huddle", "close"],
    );
    observed.actions.length = 0;
    await act(async () => hook.result.current.handleMessage());
    assert.deepEqual(
      observed.actions.map((row) => row[0]),
      ["dm", "navigate", "close"],
    );
    hook.unmount();
    client.clear();
  });

  test(`${mode}: saved Voice and Mobile settings never mount excluded private panels`, async () => {
    const { SettingsView } = await import("../settings/ui/SettingsView.tsx");
    for (const section of ["voice", "mobile"]) {
      observed.settings.length = 0;
      const selections = [];
      let view;
      await act(async () => {
        view = render(
          h(SettingsView, {
            section,
            onClose: () => {},
            onSectionChange: (value) => selections.push(value),
          }),
        );
      });
      assert.deepEqual(observed.settings, [
        privateMode ? "appearance" : section,
      ]);
      assert.equal(
        view.queryByTestId(`settings-nav-${section}`) === null,
        privateMode,
      );
      assert.deepEqual(selections, privateMode ? ["appearance"] : []);
      view.unmount();
    }
  });

  test(`${mode}: shortcut card omits only unsupported huddle promises`, async () => {
    const { KeyboardShortcutsCard } = await import(
      "../settings/ui/KeyboardShortcutsCard.tsx"
    );
    const view = render(h(KeyboardShortcutsCard));
    for (const text of ["Start or leave huddle", "Push to talk"])
      assert.equal(view.queryByText(text) === null, privateMode);
    assert(view.getByText("Send message"));
    assert(view.getByText("New direct message"));
  });
}
