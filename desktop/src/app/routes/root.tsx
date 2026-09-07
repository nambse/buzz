import { privateRouteAllowed } from "@/features/ortak/privateMode";
import { createRootRoute, redirect } from "@tanstack/react-router";

import { AppShell } from "@/app/AppShell";
import { HuddlePresenceProvider } from "@/features/huddle/HuddlePresenceContext";
import { UserStatusLookupProvider } from "@/features/user-status/UserStatusLookupContext";

function RootRoute() {
  return (
    <HuddlePresenceProvider>
      <UserStatusLookupProvider>
        <AppShell />
      </UserStatusLookupProvider>
    </HuddlePresenceProvider>
  );
}

export const Route = createRootRoute({
  beforeLoad: ({ location }) => {
    if (!privateRouteAllowed(location.pathname))
      throw redirect({ to: "/agents" });
  },
  component: RootRoute,
});
