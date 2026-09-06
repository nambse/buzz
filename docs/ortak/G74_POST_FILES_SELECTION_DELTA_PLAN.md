# G74 selection after the Files acceptance run

## Completed selection and recovery

The planned post-Files baseline and populated G74 recovery have now completed.
Root returned Ada to ordinary Office with empty tool/workspace permissions,
routing enabled and revision `61430887-dcc6-4def-8435-cfd723077f69`; a fresh
cohort was captured, reconciled and enabled. Historical Files run evidence,
input files and sealed run copies remained in scope after that policy change.

The final selection uses controller `2ec604…`, image `679b6f47…`, unchanged
worker image `aebff…`, and the exact owned local journal volume `7d40b392…`.
The original host journal and controller `6783…` are historical crash evidence,
not current storage. `current-owners74-volume.json` supplied the pre-pause five
owners; their restart requires a new owner receipt.

Root used fresh preparation `0157e313a698413096492514afa3d1e5` and registry
`c8b8d10995044751be326de19917cef1`. Actual capture
`214fd4f027a34604aeb7469d9dfb9a60` and offline restore
`cea594c6416d42f7a3403aa7509d2c70` passed with the frozen 28-file operator closure.
The source services resumed, while restored execution stayed disabled. Exact
hashes, counts and limits are in
[Current private G74 recovery selection](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).

## Historical plan before the crash and completed recovery

The following records the earlier 26-file planning checkpoint. Its future-tense
steps and owner pins are history and must not be replayed as a current recipe.

The selected rollout directory is
`/private/tmp/ortak-private-20260905/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6`.
All paths below are relative to that directory. Existing controller 6783/image 032e,
worker image aebff, Honcho 33d4/image 9358 and the five host owners remain selected.
Their identities must be observed again during the eventual preparation.

The completed Manual Work cutover is historical evidence:

- `config/catalog74-files-manual.json` and `files-manual-preparation.json` bind
  the reviewed routing=false catalog. The preparation receipt keeps its original
  `prepared_not_imported` status.
- `files-manual-active-selection.json` and its exact
  `files-manual-activation-8e7b1e0f993a492f97f2635c50e54b3c/receipt.json`
  bind active Files revision `bb0ae186-2a3a-4bac-841f-0f0b89976bb8`.
- `manual-work-cohort/139cd705cd354effb92e61a34ca94dd8/` supplies
  `intent.json`, `initial-witness.json`, `capture-result.json`,
  `page-0-result.json`, `enable-result.json`, `after-enable-witness.json`
  and `receipt.json`. The exact enabled capture is
  `47b7152e-8f4e-4b5b-be20-1692f92d5a79`; reconciliation scanned 15 and inserted 0.
  Its zero-obligation witness predates the new Work run and is not a later drain.

Root's intended final baseline is ordinary Office: empty tool/workspace
permissions, routing=true, and an enabled fresh cohort. The existing
`config/catalog74-empty.json` expresses that policy; its retained runtime
workspace_ref remains unchanged and does not itself grant Files access. Root
will first verify a RoutingDisabled Office decision while Manual Work Files is
active, finish the actual Work acceptance, settle all actions/readers, disable
the cohort, apply the existing empty catalog, then capture/reconcile/enable a new
cohort. None of those future outcomes is asserted here.

After root supplies the actual final evidence, add the exact public JSON paths
to `private_recovery_inventory.py::PUBLIC_FILES`: the files above, the empty
catalog, the Files run acceptance receipt, the RoutingDisabled decision receipt,
and the final empty activation/current-selection/cohort receipts. Resolve those
last paths from root's completed operations rather than inventing names. Their
hashes and pointer targets become part of the new preparation. Preserve all
existing registration, migration and prior activation receipts unchanged.

The retained workspace component stays selected after permissions become empty.
Preparation and the held capture barrier must observe the current six-table
rows, exact input files, sealed run copies, stopped reader proofs and terminal
controller journal. Do not revert to the registration receipt's historical zero
use/action/reader counts, renew an expired grant, or discard stopped failed
preparation history. Reject pending/result_ready actions, unresolved readers
even after lease expiry, nonterminal runs, unsettled cancellation/output/probe
work and any other selected capture obligations.

Before freezing the new operation, run focused public-evidence binding tests:
changed/missing receipt or pointer target refuses; final empty permissions and
historical Files uses coexist; a routing=false or stale capture receipt is not
misrepresented as the final ordinary Office baseline. The existing containment,
same-snapshot filesystem and physical offline extraction gates remain required.
Then create a fresh read-only preparation and owner registry. Only root executes
the reviewed pause/capture/resume sequence. Resume preserves the captured final
policy and cohort; it does not replay either historical cohort helper or an
activation operation.
