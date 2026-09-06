import { privateOrtakMode } from "@/features/ortak/privateMode";
import { Alert, AlertTitle, AlertDescription } from "@/shared/ui/alert";
import * as React from "react";
import { createFileRoute } from "@tanstack/react-router";

import {
  parseProfilePanelTab,
  parseProfilePanelView,
  type ProfilePanelTab,
  type ProfilePanelView,
} from "@/features/profile/ui/UserProfilePanelUtils";
import { useOrtakOrigin } from "@/features/ortak/useOrtakOrigin";
import { workSelection } from "@/features/ortak/work/selection";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

type AgentsRouteSearch = {
  profile?: string;
  profilePersona?: string;
  profileTab?: ProfilePanelTab;
  profileView?: ProfilePanelView;
  workProject?: string;
  workItem?: string;
};

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function validateAgentsSearch(
  search: Record<string, unknown>,
): AgentsRouteSearch {
  const work = workSelection(search.workProject, search.workItem);
  return {
    profile: nonEmptyString(search.profile),
    profilePersona: nonEmptyString(search.profilePersona),
    profileTab: parseProfilePanelTab(search.profileTab) ?? undefined,
    profileView: parseProfilePanelView(search.profileView) ?? undefined,
    workProject: work?.project,
    workItem: work?.item,
  };
}

const AgentsScreen = React.lazy(async () => {
  const module = await import("@/features/agents/ui/AgentsScreen");
  return { default: module.AgentsScreen };
});

const OrtakScreen = React.lazy(async () => {
  const module = await import("@/features/ortak/OrtakScreen");
  return { default: module.OrtakScreen };
});

export const Route = createFileRoute("/agents")({
  validateSearch: validateAgentsSearch,
  component: AgentsRouteComponent,
});

function AgentsRouteComponent() {
  const origin = useOrtakOrigin();
  const search = Route.useSearch();
  const work = workSelection(search.workProject, search.workItem);
  if (origin === undefined) return <ViewLoadingFallback kind="agents" />;
  if (privateOrtakMode && !origin)
    return (
      <Alert variant="destructive" className="m-6 w-auto">
        <AlertTitle>Ortak connection unavailable</AlertTitle>
        <AlertDescription>
          This community has no configured Ortak API. Select the configured
          private community.
        </AlertDescription>
      </Alert>
    );
  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="agents" />}>
      {origin ? (
        <OrtakScreen
          key={`${origin}:${work?.project ?? ""}:${work?.item ?? ""}`}
          origin={origin}
          initialWork={work}
        />
      ) : (
        <AgentsScreen />
      )}
    </React.Suspense>
  );
}
