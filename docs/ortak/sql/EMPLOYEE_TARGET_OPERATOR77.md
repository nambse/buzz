# One-shot reviewed employee target registration

Source only. Root must first finish main77 and the matching Honcho77 extension
cutover and declare those selected services ready. This command does not migrate,
start services, adopt employees, create native namespaces, approve/publish facts,
enable runtime consumption or renew an existing target.

There was no existing registration CLI. The isolated auto-discovered Cargo
example uses the production
`inspect_reviewed_employee_namespace` → `validate_reviewed_employee_namespace`
→ `employee_memory_exports::register_target` chain. No manifest or shared
production file changes are required:

```text
cargo build --locked -p ortak-server --example register_employee_memory_targets
<root-staged-example> --config <absolute frozen0600 intent.json>
<root-staged-example> --config <same intent.json> --action register
<root-staged-example> --config <same intent.json> --action readback
<root-staged-example> --config <same intent.json> --action recover
```

The default is `plan`: it parses a bounded owner0600, regular, single-link,
no-follow file, checks the opened inode/generation and emits its SHA plus public
selection metadata. It reads no credential environment and performs no I/O to
either service. Root supplies the existing database/token environment only to
the actual process, using its owned private launcher. Neither value belongs in
argv, config, shell tracing or stdout. Root must capture stdout/stderr in a new
bounded private0600 receipt log; preserve every earlier attempt and this exact
intent file. Output is at most five JSON lines of <=4096 bytes plus a closed
failure line. The complete invocation has a300-second deadline and signal
cancellation, with existing bounded PG/HTTP operations beneath it.

The full input shape is:

```json
{
  "format": "ortak-employee-target-operator/1",
  "company_id": "<selected existing company UUID>",
  "community_id": "<selected existing community UUID>",
  "database_env": "ORTAK_DATABASE_URL",
  "database_port": 55433,
  "deployment": {
    "deployment_id": "<existing owned Honcho deployment UUID>",
    "endpoint_ref": "<existing opaque endpoint reference>",
    "origin": "http://127.0.0.1:8009",
    "token_ref": "<existing opaque Honcho token reference>",
    "token_env": "<existing selected token environment name>"
  },
  "targets": [{
    "original": "<replace with complete original HonchoCreatedResourcesReceipt OBJECT>",
    "destination_channel_id": "<explicit reviewed destination UUID>",
    "diagnostic": {
      "operation_id": "<fresh UUID frozen before execution>",
      "employee_revision_id": "<current existing revision UUID>",
      "employee_lifecycle_epoch": 1,
      "challenge": "<64 lowercase hexadecimal synthetic characters, frozen before execution>"
    },
    "valid_until": "<fixed operator-selected UTC timestamp with at most microsecond precision>"
  }]
}
```

This shape is illustrative, not executable JSON for a real target. Copy each
**public original creation receipt object**, never an adoption outcome or an
OAuth/signer file, from the already retained worker/prepared-memory selection.
Use its exact employee/binding/deployment/native ownership. Select1–3 distinct
employees (Ada, Bora and Deniz for the requested batch), with separate original
namespaces and diagnostic UUIDs. Freeze each current revision/lifecycle and one
fixed independent expiry, no more than90days away; replay never moves that
expiry. The database environment is restricted to explicit loopback55433
production or55432 disposable testing, with no URI query/fragment. The resolved
company/community must agree with the config before target work.

`register` first performs bounded **retained metadata readback**. An exact
committed target returns its ID, namespace/binding hashes, original registration
receipt hash, fixed expiry and actual enabled/runtime flags. It does not resolve
a token or make an HTTP call for that target, even if the target is now disabled
or expired. A conflicting target/diagnostic/expiry refuses; it is never updated.
Missing targets use the current selected employee/revision/memory check, then
the existing original-owned-namespace adapter inspection and finite synthetic
write/read/cleanup diagnostic. Registration is attempted at most twice using
the **same in-process witness** and expiry, with one-second separation. No
second diagnostic is run for a lost registration response. Final SQL authority,
destination membership and retained-scope checks remain the production API's.

The diagnostic UUID/challenge are durable intent in the input file before any
HTTP write. If validation times out or the process is interrupted, retain that
file. `readback` makes only bounded PG reads and reports `registered_retained`
or `not_registered`; it does not assert current runtime/publication authority.
If no target exists and diagnostic cleanup is uncertain, `recover` calls only
`recover_employee_namespace_diagnostic` on that same original owned namespace
and diagnostic. It can return a confirmed erased/tombstone receipt without
registering anything. It does not require the employee still be active.

A cleaned diagnostic cannot mint a replacement process-local I/O witness.
After confirmed cleanup with no target, a later fresh registration requires a
separately frozen new diagnostic intent; keep the old intent/cleanup receipt.
Do not change the original ownership/destination/expiry to turn a conflict into
success. No generic remote cleanup or renewal API is exposed here.

The root's actual successful batch supplies three `target_id` values. Add those
only through the separately selected worker
`reviewed_employee_destinations:[{target_id,destination_channel_id}]` configuration
when runtime use is intended. Registration itself leaves that opt-in separate;
explicit signed publication still follows the user-facing approval/export path.

Only the new example and its two subordinate files are build inputs. Source
gates already passed for the adapter and registration API remain reusable;
the integration gate is this example's focused compile followed by the one
authorized actual three-employee registration, not a new general test matrix.
