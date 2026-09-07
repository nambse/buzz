import type { TimelineMessage } from "@/features/messages/types";
import { useIdentityQuery } from "@/shared/api/hooks";
import { DropdownMenuGroup, DropdownMenuItem } from "@/shared/ui/dropdown-menu";
import { useOrtakOrigin } from "../useOrtakOrigin";
import { uuid } from "./validation";

function OwnMessageAction({
  message,
  origin,
  onSelect,
}: {
  message: TimelineMessage;
  origin: string;
  onSelect: (selection: { origin: string; actor: string }) => void;
}) {
  const identity = useIdentityQuery();
  const actor = identity.isError ? null : identity.data?.pubkey;
  return actor &&
    actor === message.pubkey &&
    (!message.signerPubkey || message.signerPubkey === actor) ? (
    <DropdownMenuGroup>
      <DropdownMenuItem onSelect={() => onSelect({ origin, actor })}>
        Review employee memory
      </DropdownMenuItem>
    </DropdownMenuGroup>
  ) : null;
}
function ConfiguredAction(props: {
  message: TimelineMessage;
  onSelect: (selection: { origin: string; actor: string }) => void;
}) {
  const origin = useOrtakOrigin();
  return origin ? <OwnMessageAction {...props} origin={origin} /> : null;
}
/** Own-authored plaintext only. No body, key material or client-supplied authority is sent. */
export function EmployeeMemoryAction({
  message,
  channel,
  onSelect,
}: {
  message: TimelineMessage;
  channel?: string | null;
  onSelect: (selection: { origin: string; actor: string }) => void;
}) {
  if (
    message.pending ||
    message.isAgent ||
    !uuid(channel) ||
    !/^[0-9a-f]{64}$/.test(message.id) ||
    !/^[0-9a-f]{64}$/.test(message.pubkey ?? "") ||
    !(message.kind === 9 || message.kind === 40002)
  )
    return null;
  return <ConfiguredAction message={message} onSelect={onSelect} />;
}
