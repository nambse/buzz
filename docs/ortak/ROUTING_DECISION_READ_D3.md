# Message routing decisions

The message's **More actions → View routing decision** opens a read of its
persisted central routing outcome. This applies only to delivered text events
in an explicitly configured Ortak Office. It is useful even when no run exists.
Opening or refreshing the view cannot score a message, replay it, create a run,
refresh OAuth credentials, or wake an employee.

`GET /api/v1/channels/{channel_id}/messages/{message_id}/routing` uses the existing
signed NIP-98 product API and its current Office authority transaction. Before
reading a decision, the handler requires the canonical community/company
binding, exact source event/channel, supported text kind, nondeleted supported
channel, configured human channel grant and current private-channel audience. A
private DM must resolve to exactly one retained employee and one human using the
canonical participant fingerprint. Only that current human, with the counterpart
employee grant, can read it; candidate details are limited to that counterpart,
even when the reader has other employee grants. Encrypted gift wraps and group
DMs remain unsupported. Canonical
source deletion or binding purge makes retained decisions unavailable. Archived
channel history remains readable while its audience permits, following the
existing MVP read contract. A missing
binding returns 404; database failures remain 503. Denials use the existing
`access` audit action, without a new schema enum.

A currently accessible message without a persisted decision returns
`decision: null`. The UI says no outcome is recorded, rather than claiming
silence. An existing decision with inconsistent inbox/source pins returns 503
instead of disguising broken evidence as an absent record. A recorded `silent`
decision explicitly reports no employee dispatch.
A waking decision with no currently granted candidate details still remains a
waking decision; filtering must not turn it into a zero-recipient claim.

The projection includes mode, a typed stable reason, policy/scorer/prompt
versions, selected model/thinking, latency, bounded token counts and cache use.
It returns the first 32 recipients in saved order **after** filtering the current
configured employee grant, with an explicit truncation indicator. Each has a
finite score or no score and labels from the scorer's five-value vocabulary.
Unknown usage/failure/evidence fields and oversized scalar metadata are withheld.
Raw source content, manifests, provider bodies, input/binding hashes, excluded
targets and ungranted employee identifiers never enter the response. No global
candidate count is returned.

While the dialog is open, its read is refreshed every five seconds. These are
authorized snapshots, not a realtime decision stream. Failed checks clear the
displayed private result. Authorization denial stops retries; transient failures
stop after five attempts with bounded backoff and keep **Refresh routing**.
Closing, changing message or switching client aborts ownership; late responses
cannot populate the new selection. Keyboard/pointer menu selection uses the
existing Radix interaction, and closing restores the message action button.

The read uses the existing unique company/message decision key and bounded
recipient query, so no list/index migration is required. A durable decision
subscription and a real provider quality/dispatch cohort remain separate gates.

Validation seams:

- `postgres_authenticated_routes routing_read::`: six signed PostgreSQL cases
  for absent/zero/waking records, canaries/current scopes, and a held authority
  read racing membership revocation, inconsistent retained source pins, and
  private DM counterpart/recovery/current-participant fencing. These retained-row fixtures are explicitly
  inert and do not claim dispatch/provider acceptance.
- The existing E5 canonical-purge test additionally populates a routing decision,
  proves its row survives purge, and verifies this read becomes 404.
- `desktop/src/features/ortak/routing/routing.test.mjs`: seven actual client,
  menu-gate and panel/hook cases. Full Ortak JavaScript suite: 94 tests passed;
  TypeScript and scoped Biome checks passed. The build owner subsequently passed
  the complete 104-case signed PostgreSQL suite, including all six routing reads.
  Installed backend/native acceptance remains a separate deployment gate.
