# Encrypted worker composition — source handoff

Status: implemented source, not activated or executed by this lane. Migration77
and the selected installed bridge capability are prerequisites. This adds no SQL
definitions to the reviewed jobs/admission/execution fragments.

`ortak-worker` accepts optional `encrypted_dm` inside its existing bounded
`ORTAK_WORKER_CONFIG_JSON`. Absence is default-off. A binary built without
`encrypted-dm` rejects an explicit selection. With the feature, absent/invalid
configuration leaves retained keyless recovery available; it never registers,
enables, changes or adopts a pair. The initial database contract remains one
enabled pair per employee, with immutable retained selection IDs.

The object has exactly these public fields:

```json
{
  "format": "ortak-encrypted-worker/1",
  "pair_ids": ["<existing selection UUID>"],
  "relay_origin": "wss://<selected Office host>/",
  "key_bindings": [{
    "signer": {
      "company_id": "<this worker company UUID>",
      "employee_id": "<durable employee ID>",
      "signer_ref": "<exact existing opaque Office reference>",
      "public_key": "<exact employee Office public key, lower hex>",
      "secret_env": "<explicit existing purpose-authorized environment name>"
    },
    "office_binding_id": "<existing verified Office binding UUID>",
    "key_version": 0,
    "purposes": ["dm_decrypt", "confidential_wrap", "confidential_unwrap", "dm_seal"]
  }]
}
```

This is a shape illustration, not runnable configuration. There are 1–16 unique
non-nil pair IDs and 1–16 exact owned key bindings. Every selected binding must
permit all four closed purposes; no raw key is in the JSON. The existing provider
rejects duplicate owner/version, key or environment aliases. Relay construction
accepts a bare WSS origin or explicit loopback WS, with no auth/query/fragment.
The outer worker JSON remains capped at16KiB, so it can reject a large selection
before the per-field ceilings are reached. Missing secret material is discovered
only by the particular authorized purpose operation.

Fresh work additionally requires the installed `ConfidentialDmV1` capability,
the current explicit pair, company/community binding, enabled Office cohort and
selected channel/employee, exact Office key/ref/version and a validated active
Hermes revision with genuinely empty permissions. No Files policy is silently
downgraded. A stale policy/identity is checked again by the job, protected commit
and every execution/effect repository. The routing scorer, ordinary memory/Work
composition and ordinary Office signer availability do not gate this lane.

Each tick keysets at most32 untouched1059 metadata rows against at most16 explicit
pairs, queues/claims at most one decrypt job, and processes at most one dispatch,
observation, reply seal and publication. The existing job120s source deadline,
three attempts with1s/5s backoff, five-second crypto budget, protected ten-minute
execution deadline, finite leases and matching two-copy ACK rules remain intact.
No employee subscribes independently. The ordinary inbox consumer excludes exact
retained job sources and current untouched selected wrappers; old unsupported
decisions are never reset. A never-enqueued wrapper beyond the120s window can
still receive the existing unsupported outcome, with no encrypted execution.

After verification the same protected object is retained for at most three commit
attempts, with1s/5s backoff. Receipt lookup precedes current admission checks. An
uncertain commit never marks its possibly consumed job failed or recomputes
encryption. Process loss retains either the committed receipt/run or the original
finite unconsumed job. Operational errors propagate with closed messages.

Recovery scopes come from an already resolved company and exact retained
selection/job/run provenance, capped at128 communities. They are not current
read/use grants. Office unbinding, disabled/replaced config, loss of capability,
or removed keys never trigger enrollment or key discovery. One unselected run is
queued for cancellation per tick; while these remain, execution uses only the
new stop-only observation claim. Its adapter calls cannot read events or send
keys. One unleased/expired frozen copy with an existing cancellation is retired
per tick without inventing an ACK. Unconfirmed containment stays unresolved.

An unconsumed terminal failed/cancelled decrypt job can finalize only its exact
still-untouched inbox row as failed. The original job error, claim history and
outer tuple remain unchanged. Existing universal community write fences apply;
the recovery lane does not bypass canonical quiescence.

Root setup must select the existing pair and current matching empty-policy
revision, install77 before this worker source, choose the tested confidential
bridge artifact/profile and exact Office relay, and explicitly supply only the
selected Office environment references. This worker performs none of those
operator mutations.

Two focused new gate selectors (not executed by this lane):

- Server worker binary with `encrypted-dm`:
  `encrypted_worker_config_binds_explicit_company_and_all_purposes_without_key_io`.
- Runtime `postgres_run_supervision` with `encrypted-dm`, disposable77 and the
  existing explicit synthetic key environment:
  `encrypted_suspended_recovery_uses_retained_scope_and_denies_all_keys`.

The second uses real protected admission and the selected HTTP adapter, verifies
the stop-only port cannot claim a normal observing run, suspends the fixture
company through the production mutation path, resolves its retained community,
then requires keyless cancellation ACK and no remaining recoverable scope.
The retained cohort foreign key prevents removing its Office binding; the test
does not remove that protection or claim a reachable unbinding workflow.
Its synthetic server exposes lookup/start/cancel only; any
event/content request fails the fixture. Earlier passed execution gates remain
separate evidence.
