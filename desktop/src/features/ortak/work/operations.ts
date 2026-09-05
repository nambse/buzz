import type { WorkProject, WorkState } from "../types";

export interface WorkOperation {
  readonly path: string;
  readonly body: string;
  readonly label: string;
}

/** Freeze the exact request once; a retry must reuse these bytes and operation ID. */
export function workOperation(
  path: string,
  label: string,
  values: Record<string, unknown>,
): WorkOperation {
  const body = JSON.stringify({ ...values, operation_id: crypto.randomUUID() });
  if (new TextEncoder().encode(body).length > 16 * 1024)
    throw new Error(
      "This form is too long. Shorten the description or acceptance criteria.",
    );
  return Object.freeze({ path, body, label });
}

export const stateLabel = (state: WorkState) => state.replaceAll("_", " ");
const transitions: Record<WorkState, WorkState[]> = {
  proposed: ["ready", "cancelled"],
  ready: ["in_progress", "blocked", "proposed", "cancelled"],
  in_progress: ["review", "blocked", "ready", "cancelled"],
  blocked: ["ready", "in_progress", "cancelled"],
  review: ["completed", "in_progress", "cancelled"],
  completed: [],
  cancelled: [],
};

/** Mirrors the closed domain transition graph; the server remains authoritative. */
export function availableTransitions(
  state: WorkState,
  project: WorkProject,
): WorkState[] {
  return transitions[state].filter((target) =>
    state === "review" && target !== "cancelled"
      ? project.can_review
      : project.can_contribute,
  );
}
