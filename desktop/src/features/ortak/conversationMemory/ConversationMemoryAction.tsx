import type { TimelineMessage } from "@/features/messages/types";
import { DropdownMenuGroup, DropdownMenuItem } from "@/shared/ui/dropdown-menu";
import { useOrtakOrigin } from "../useOrtakOrigin";

/** Plaintext stream-like messages only; the signed resolver enforces actual channel kind. */
export function ConversationMemoryAction({
  message,
  channel,
  onSelect,
}: {
  message: TimelineMessage;
  channel?: string | null;
  onSelect: (origin: string) => void;
}) {
  const origin = useOrtakOrigin();
  const allowed =
    !message.pending &&
    Boolean(channel) &&
    (message.kind === 9 || message.kind === 40002) &&
    /^[0-9a-f]{64}$/.test(message.id);
  return origin && allowed ? (
    <DropdownMenuGroup>
      <DropdownMenuItem onSelect={() => onSelect(origin)}>
        Review conversation memory
      </DropdownMenuItem>
    </DropdownMenuGroup>
  ) : null;
}
