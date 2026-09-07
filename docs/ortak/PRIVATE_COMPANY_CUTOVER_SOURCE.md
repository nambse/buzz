# Private company UI cutover source

The source handoff below is historical. The integration owner built and deployed
this boundary on 2026-09-06, including actual private graphs, native UI and
live relay route checks. See the [current checkpoint](CONTINUATION_PROGRESS_2026-09-05.md)
and [usage notes](PRIVATE_V0_RUNBOOK.md) for the selected artifacts and limits.

Source handoff for Milestone 8, following the one-company boundary in
`ARCHITECTURE_V0.md` and the community/catalog disposition in `BUZZ_BASELINE.md`.
This does not claim a rebuilt or installed artifact. No test, native operation,
credential read, or recovery inventory change was performed for this handoff.

The private profile menu presents a static Company row. The community rail,
add dialog, community onboarding transaction, join-link listener, and community
change actions in connection and membership recovery are unavailable. Settings
retains authorized company member administration. Initial profile setup, retry,
identity import, key backup, and machine identity recovery remain available.
Ordinary builds retain their existing community UI and onboarding behavior.

The real community provider exposes only a saved entry matching the single
compiled relay/API binding. It refuses add, remove, clear, reorder, switching,
and destination/token changes before persistence or cleanup. Reconnecting the
selected entry and updating its identity/display metadata remain possible.
Older saved entries and suspended onboarding data are retained. Fixed-company
bootstrap appends its selected entry without erasing those older entries.

Native `apply_workspace` checks the compiled company relay before state access
or mutation and refuses an `nsec` replacement. Existing `import_identity` owns
key changes and recovery. The private default relay also comes from the compiled
pin, so runtime environment changes cannot redirect bootstrap. A single trailing
slash is equivalent; a different host, port, path or query is not. The existing
private build recipe provides this pin and the single matching frontend binding.

## Changed production paths

- `desktop/src/features/ortak/privateCompany.ts`
- `desktop/src/features/communities/{useCommunities.tsx,communityStorage.ts,useCommunityInit.ts}`
- `desktop/src/features/communities/ui/CommunityApplyErrorScreen.tsx`
- `desktop/src/features/onboarding/communityOnboarding.tsx`
- `desktop/src/features/onboarding/ui/{OnboardingFlow,MembershipDenied}.tsx`
- `desktop/src/features/sidebar/ui/{AppSidebar,SidebarProfileCard}.tsx`
- `desktop/src/features/settings/ui/SettingsView.tsx`
- `desktop/src/app/{App,AppShell}.tsx`
- `desktop/src-tauri/src/{private_native,relay}.rs`
- `desktop/src-tauri/src/commands/workspace.rs`

## Integration owner gates

The three React cases bind actual provider/storage callbacks and the membership
recovery component: alternative destinations cannot mutate retained storage;
fixed bootstrap preserves existing entries; retry and validated key import work
while community/invite actions are absent. The native identity harness calls the
production destination guard; the existing source guard binds its placement
before workspace mutation. These commands are prepared, not executed here:

```sh
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/privateCompany.test.mjs
```

From the repository root with the pinned toolchain activated:

```sh
node --test desktop/scripts/ortak-private-native.test.mjs desktop/scripts/ortak-private-native-guards.test.mjs
node desktop/scripts/ortak-private-native.mjs verify-identity
```

The `verify-identity` recipe passes the same compiled relay pin to its existing
small Rust harness. Build with the retained `ortak-private-native.mjs build`
recipe after root-owned type/format checks and the other Milestone 8 source
cutoffs. No setup, adoption, OAuth, identity, API binding, or permissions change
is needed for the already selected company.
