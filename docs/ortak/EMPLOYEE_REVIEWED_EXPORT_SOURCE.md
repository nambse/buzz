# Employee-owned reviewed exports: source integration

This is the held source for a genuine company/employee Honcho namespace. Root's
server library check passed before the final test-only additions and two public
API documentation comments. The two signed PostgreSQL and three adapter HTTP
regressions below are prepared, not executed by this agent. No numbered
migration, live grant, deployment, periodic diagnostic or run recall is added.

The signed source routes are mounted by root for its integration gate, behind
the existing host-derived company and fresh NIP-98 middleware. The original
employee review facade and project/conversation semantics remain unchanged.

## Source and wire

`crates/ortak-memory/src/employee_reviewed/` implements explicit namespace
inspection, finite diagnostic write/read/cleanup, strict publication and
withdrawal ACKs, and selected-ID remote recall. `http.rs` adds lower per-request
limits while preserving old callers' limits. Only a private process-local value
can represent successful initial I/O; JSON deserialization cannot create it.

`crates/ortak-server/src/employee_memory_exports/` owns retained registration,
explicit current target refresh, leased prepare/ACK/failure, a finite worker
step and the private Principal-only export commands. Network I/O occurs outside
the short database transactions. The worker is not selected by the live worker
composition. `schedule_one` does not loop or renew ownership evidence.

The new signed routes are:

- GET/POST `/api/v1/employees/{employee}/reviewed-memory/{fact}/export`.
- POST `.../export/retry/{publish|withdraw}`.

Both POSTs accept only `{operation_id,expected_version}`. Publication requires
version 1. Retry uses the observed job retry version, from 0 through 7. The
server chooses the one unambiguous current target for the fact's exact employee
and destination; a request cannot nominate a foreign target. Metadata is bounded
to 16 KiB and contains no edited text, provenance or source body. The original
human and remaining employee ceiling retain metadata, identical command replay
and withdrawal-retry recovery after source/capability loss. New publication and
publication retry require the explicit review capability and current configured
source and destination channel ceilings. Operator role is insufficient.

The exact remote wire and hash recipes are in
[EMPLOYEE_REVIEWED_MEMORY_PROTOCOL.md](EMPLOYEE_REVIEWED_MEMORY_PROTOCOL.md).
An immutable target `registration_receipt` retains the completed one-time
diagnostic. Its 55-second freshness admits initial registration only. The
operator's separate expiry is fixed and at most 90 days; read-only health or a
model revision refresh cannot extend it. Exact committed registration replay
returns its historical target without enabling or renewing it. Ordinary remote
operations inspect the original owned namespace without synthetic writes.

## SQL assembly and inventory

Root applies these unnumbered fragments in order to an explicitly disposable
database containing immutable 1–76:

1. `sql/employee_reviewed_memory_candidate.sql`.
2. `sql/employee_reviewed_memory_authority_candidate.sql`.
3. `sql/employee_reviewed_memory_protocol_candidate.sql`.

The final fragment replaces the refusing target current-data port and its guard,
adds the immutable registration receipt, and corrects receipt expiry semantics:
expired text is unavailable for recall but is not proof of physical deletion.
Only a confirmed irreversible tombstone proves erasure in the remote extension.
SQL authenticates no SQL-credential holder or caller-set actor/GUC. Signed
authorization belongs to the private Principal facade; actual ownership/I/O
belongs to the explicitly selected adapter and sealed witness.

The central inventory remains eight retained tables:
`employee_memory_channel_authorities`, `employee_reviewed_memory_facts`,
`employee_reviewed_memory_operations`, `employee_reviewed_memory_targets`,
`employee_reviewed_memory_exports`, `employee_reviewed_memory_export_jobs`,
`employee_reviewed_memory_export_commands`,
`employee_reviewed_memory_export_receipts`.

The isolated Honcho family adds seven tables:
`ortak_employee_reviewed_records`, `ortak_employee_reviewed_content`,
`ortak_employee_reviewed_tombstones`, `ortak_employee_reviewed_operations`,
`ortak_employee_diagnostics`, `ortak_employee_diagnostic_content`,
`ortak_employee_diagnostic_tombstones`. Only the two content tables erase bytes;
all operation, ownership and tombstone evidence is retained. Existing native
Honcho messages and prior project-family tables are unaffected.

## Prepared focused gates

```sh
cargo test --locked -p ortak-memory --lib tests::http_contract::employee -- --ignored --test-threads=1
cargo test --locked -p ortak-server --test postgres_authenticated_routes employee_memory::exports:: -- --ignored --test-threads=1
```

The server fixture requires `ORTAK_TEST_DATABASE_URL` on disposable port 55432
and the assembled SQL above. It creates fresh public fixture identities and uses
a local controlled Honcho transport. The real adapter obtains the namespace
witness; the real signed facade writes facts/export commands; the production
worker records exact live-lease ACKs. It does not insert synthetic target or ACK
rows. Existing facade tests keep their old fixture behavior.

The five cases cover exact readback and confirmed cleanup, cleanup-only recovery,
forged identity/ACK/body bounds, explicit relationship-human matching, no recurring
diagnostics, signed registration/publication/Stop after source loss, immutable
registration expiry, same-key retry and current lease ownership. Installed Honcho
schema/guard/protocol tests remain an independent root gate.

The next runtime slice must resolve an actual human requester and canonical
Office source/destination, join current exact employee target selection, freeze
source/destination and target consumption epochs, and recheck them at every
admission/use/delivery boundary. The selected-recall adapter primitive alone
authorizes none of that. Manual Work, ambient employee-global recall and local
approved-text fallback remain closed.
