/** Metadata from the native command's fresh signed central read; never a grant. */
export interface Pair {
  format: "ortak-native-encrypted-dm-authority/1";
  company_id: string;
  community_id: string;
  channel_id: string;
  employee_id: string;
  human_public_key: string;
  employee_public_key: string;
  pair_hash: string;
  selection_id: string;
  selection_generation: string;
  office_binding_id: string;
  key_version: string;
  office_generation: string;
  authority_epoch: string;
  observed_at: string;
  valid_before: string;
}
export interface Context {
  view_id: string;
  channel_id: string;
  expected_human: string;
  expected_relay: string;
}
export interface Authority {
  pair: Pair;
  scope: string;
}
export interface Pending {
  operation_id: string;
  scope: string;
  rumor_id: string;
  outer_ids: [string, string];
  acknowledged: [boolean, boolean];
  retired_at: number | null;
}
/** Deliberately volatile: never pass this object to ordinary event/cache APIs. */
export interface MessageView {
  rumor_id: string;
  sender: string;
  created_at: number;
  reply_to: string | null;
  text: string;
}
export interface OpenView extends Authority {
  draft: { version: number; text: string };
  pending: Pending | null;
  retired: Pending[];
  messages: MessageView[];
  limited: boolean;
  withheld_count: number;
}
export type NativeDm = <T>(
  command: string,
  args: Record<string, unknown>,
) => Promise<T>;
