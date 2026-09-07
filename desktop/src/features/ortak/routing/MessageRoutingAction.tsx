import type { TimelineMessage } from "@/features/messages/types";
import { DropdownMenuGroup, DropdownMenuItem } from "@/shared/ui/dropdown-menu";
import { useOrtakOrigin } from "../useOrtakOrigin";

export function canReadMessageRouting(
  message: TimelineMessage,
  channel?: string | null,
) {
  return (
    Boolean(channel) &&
    !message.pending &&
    [9, 40002].includes(message.kind ?? 0) &&
    /^[0-9a-f]{64}$/.test(message.id)
  );
}

/** Mounted only while the menu is open; no per-row background request or wake. */
export function MessageRoutingAction({
  message,
  channel,
  onSelect,
}: {
  message: TimelineMessage;
  channel?: string | null;
  onSelect: (origin: string) => void;
}) {
  const origin = useOrtakOrigin();
  return origin && canReadMessageRouting(message, channel) ? (
    <DropdownMenuGroup>
      <DropdownMenuItem onSelect={() => onSelect(origin)}>
        View routing decision
      </DropdownMenuItem>
    </DropdownMenuGroup>
  ) : null;
}
