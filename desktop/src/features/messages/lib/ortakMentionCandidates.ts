import type { UserProfileLookup } from "@/features/profile/lib/identity";
import type { ChannelMember, ChannelType } from "@/shared/api/types";
import { normalizePubkey } from "@/shared/lib/pubkey";
import type { MentionCandidate } from "./mentionCandidates";

/** Office roster discovery; selection is revalidated at prepare and publish. */
export function ortakMentionCandidates({
  channelId,
  channelType,
  currentPubkey,
  members,
  profiles,
  isArchived,
}: {
  channelId: string | null;
  channelType?: ChannelType | null;
  currentPubkey: string | null;
  members: readonly ChannelMember[] | undefined;
  profiles: UserProfileLookup | undefined;
  isArchived: (pubkey: string) => boolean;
}): MentionCandidate[] {
  if (
    !channelId ||
    channelType !== "stream" ||
    !currentPubkey ||
    !members?.some((member) => normalizePubkey(member.pubkey) === currentPubkey)
  )
    return [];
  const candidates = new Map<string, MentionCandidate>();
  for (const member of members) {
    const pubkey = normalizePubkey(member.pubkey);
    if (!/^[0-9a-f]{64}$/.test(pubkey) || isArchived(pubkey)) continue;
    const profile = profiles?.[pubkey];
    candidates.set(pubkey, {
      kind: "identity",
      pubkey,
      displayName:
        member.displayName?.trim() ||
        profile?.displayName?.trim() ||
        profile?.nip05Handle?.trim() ||
        null,
      avatarUrl: profile?.avatarUrl ?? null,
      isMember: true,
      isAgent:
        member.isAgent === true ||
        member.role === "bot" ||
        profile?.isAgent === true,
      role: member.role,
    });
  }
  return [...candidates.values()];
}
