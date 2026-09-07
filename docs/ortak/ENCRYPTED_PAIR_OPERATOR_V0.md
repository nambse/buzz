# Explicit private encrypted-pair registration

`ortak-encrypted-pair` is an operator composition of the existing
`PgDecryptJobs::register_pair` transaction. It requires the `encrypted-dm` binary
feature, `ORTAK_ENCRYPTED_PAIR_ENABLED=true`, an explicit private database and a
bounded `ORTAK_ENCRYPTED_PAIR_CONFIG_JSON`. It reads no signing key or OAuth store,
sends no message and does not change worker configuration. The company is resolved
from the selected community; a supplied company or secret field is refused.

The configuration contains only `community_id`, `selection_id`, `channel_id`,
`employee_id`, `human_public_key`, `employee_public_key`, `office_binding_id`,
`key_version` and `decrypt_ref`. Use a fresh stable selection UUID, the actual
canonical private two-member DM, current employee Office binding and its opaque
reference. Retain the exact private request before invoking the command. A lost
acknowledgement is retried with that same request; a changed tuple or disabled
selection is refused instead of silently re-enabling it.

The production transaction rechecks the canonical pair and current authority,
serializes registration and preserves the existing SQL commit guards. The
command's static parse test covers opt-in, payload bounds, nil identities,
negative key version, identical participants and unknown/secret fields. Existing
signed API and PostgreSQL confidential tests exercise the registration port and
authority changes; an operator invocation is not runtime health evidence.

For the reboot-created private installation, the new Deniz DM is
`0728ab67-44d9-417f-83cb-2be7da369d75`. Its empty signed creation receipt is
`evidence/fresh-deniz-dm80.json`; no plaintext acceptance message was sent.
The native recipe selects this channel instead of the pre-reboot test channel.
Registration, cohort capture/reconciliation, worker purpose-specific key selection
and controller capability activation are separate explicit steps. The native
bundle must be rebuilt before testing its protected composer. None of these steps
adopts or replaces the old Cem/Zeynep resources.
