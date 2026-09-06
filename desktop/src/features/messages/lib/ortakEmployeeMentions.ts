import { resolveOrtakOrigin } from "@/features/ortak/config";
import { getRelayHttpUrl } from "@/shared/api/tauri";
import {
  getChannelDetails,
  getChannelMembers,
} from "@/shared/api/tauriChannels";
import { normalizePubkey } from "@/shared/lib/pubkey";

/** Re-read Office membership; the control plane alone decides whether to wake. */
export async function validateOrtakEmployeeMentions(
  pubkeys: readonly string[],
  currentPubkey: string | null,
  channelId: string | null | undefined,
  isChannelScope: boolean,
): Promise<void> {
  if (pubkeys.length === 0) return;
  if (!channelId || !isChannelScope || !currentPubkey || pubkeys.length > 64)
    throw new Error("Ortak mentions require an existing Office channel.");
  const relay = await getRelayHttpUrl();
  if (!resolveOrtakOrigin(import.meta.env?.VITE_ORTAK_API_BINDINGS_JSON, relay))
    throw new Error("The Office relay has no configured Ortak binding.");
  // These native calls query current kind 39000/39002 events, bypassing the
  // autocomplete and React Query caches. Repeat at prepare and publish.
  const [channel, members] = await Promise.all([
    getChannelDetails(channelId),
    getChannelMembers(channelId),
  ]);
  const keys = new Set(members.map((member) => normalizePubkey(member.pubkey)));
  if (
    channel.id !== channelId ||
    channel.channelType !== "stream" ||
    channel.archivedAt !== null ||
    !keys.has(normalizePubkey(currentPubkey)) ||
    pubkeys.some((key) => !keys.has(normalizePubkey(key))) ||
    (await getRelayHttpUrl()) !== relay
  )
    throw new Error("Office channel membership changed.");
}
