import { useCallback, useRef, useState } from "react";
import { useConversationRead } from "../conversationMemory/useConversationRead";
import {
  useEmployeeMutation,
  type EmployeeMemoryClient,
} from "./useEmployeeMutation";
import { assertPage, assertPreview } from "./validation";
import { assertExport } from "./exportValidation";
import type {
  EmployeeDraft,
  EmployeePreview,
  EmployeeFact,
  EmployeeExportAction,
  MemoryKind,
} from "./types";

/** The UI may select hints; the signed preview alone determines the current audience. */
export function useEmployeeReview(
  client: EmployeeMemoryClient,
  actor: string,
  employee: string,
  source: { event: string; channel: string } | null,
  destination: string,
  kind: MemoryKind,
  open: boolean,
) {
  const [after, setAfter] = useState<string>();
  const invalidators = useRef<Array<() => void>>([]);
  const context = JSON.stringify([source?.event, source?.channel]);
  const mutation = useEmployeeMutation(
    client,
    employee,
    actor,
    context,
    open && Boolean(employee),
    () => {
      for (const invalidate of invalidators.current) invalidate();
    },
  );
  const loadFacts = useCallback(
    async (signal: AbortSignal) => {
      const page = await client.employeeMemoryFacts(employee, signal, after);
      assertPage(page, employee, actor, after);
      return page;
    },
    [client, employee, actor, after],
  );
  const facts = useConversationRead(
    loadFacts,
    open && Boolean(employee),
    mutation.revision,
    () => {
      for (const invalidate of invalidators.current) invalidate();
    },
  );
  const event = source?.event;
  const channel = source?.channel;
  const loadPreview = useCallback(
    async (signal: AbortSignal) => {
      if (!event || !channel) throw new Error("A current source is required.");
      const request = {
        source_event_id: event,
        destination_channel_id: destination,
        kind,
        human_public_key: kind === "relationship" ? actor : null,
      };
      const { preview } = await client.employeeMemoryPreview(
        employee,
        request,
        signal,
      );
      assertPreview(preview, employee, actor, channel, request);
      return preview;
    },
    [client, employee, actor, event, channel, destination, kind],
  );
  const preview = useConversationRead(
    loadPreview,
    open &&
      Boolean(event && channel && destination && employee) &&
      facts.ready &&
      facts.value?.can_approve === true,
    mutation.revision,
  );
  invalidators.current = [
    facts.invalidate,
    preview.invalidate,
    mutation.revoke,
  ];
  const current = useRef({
    open,
    employee,
    actor,
    destination,
    kind,
    event,
    preview,
    facts,
    mutation,
  });
  current.current = {
    open,
    employee,
    actor,
    destination,
    kind,
    event,
    preview,
    facts,
    mutation,
  };
  const blocked = mutation.busy || Boolean(mutation.pending);
  const readExport = useCallback(
    async (fact: string, signal: AbortSignal) => {
      const value = await client.employeeMemoryExport(employee, fact, signal);
      assertExport(value, fact);
      return value;
    },
    [client, employee],
  );
  return {
    facts,
    preview,
    mutation,
    after,
    blocked,
    readExport,
    publication: (
      fact: EmployeeFact,
      action: EmployeeExportAction,
      expectedVersion: number,
    ) => {
      const now = current.current;
      if (
        !now.open ||
        !now.facts.ready ||
        now.mutation.busy ||
        now.mutation.pending ||
        !now.facts.value?.facts.some((row) => row === fact) ||
        (action !== "retry_withdraw" && !now.facts.value.can_approve)
      )
        return;
      now.mutation.publication(fact, action, expectedVersion);
    },
    invalidate: () => {
      for (const invalidate of invalidators.current) invalidate();
    },
    setAfter: (value?: string) => {
      if (!blocked) setAfter(value);
    },
    approve: (observation: EmployeePreview, draft: EmployeeDraft) => {
      const now = current.current;
      if (
        !now.open ||
        now.mutation.busy ||
        now.mutation.pending ||
        !now.facts.ready ||
        !now.facts.value?.can_approve ||
        !now.preview.ready ||
        now.preview.value !== observation ||
        draft.source_event_id !== now.event ||
        draft.source_event_created_at !== observation.source.event_created_at ||
        draft.destination_channel_id !== now.destination ||
        draft.kind !== now.kind ||
        draft.human_public_key !==
          (now.kind === "relationship" ? now.actor : null) ||
        draft.expected_audience_hash !== observation.audience_hash ||
        !draft.reviewed
      )
        return;
      const until = Math.min(
        Date.parse(observation.max_expires_at),
        observation.valid_before
          ? Date.parse(observation.valid_before)
          : Infinity,
      );
      if (
        !(
          Date.now() < Date.parse(draft.expires_at) &&
          Date.parse(draft.expires_at) <= until
        )
      )
        return;
      now.mutation.approve(draft);
    },
    stop: (id: string) => {
      const now = current.current;
      const fact = now.facts.value?.facts.find((row) => row.id === id);
      if (
        now.open &&
        now.facts.ready &&
        !now.mutation.busy &&
        !now.mutation.pending &&
        fact
      )
        now.mutation.stop(fact);
    },
  };
}
