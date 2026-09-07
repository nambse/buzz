# G74 journal volume recovery — actual capture and offline restore

The real Files run `fdd36ad5-0923-421b-93a8-d8179e513b8a` completed on the
Docker local journal volume. Root opened the resulting artifact in the native
client and completed Work review. Root subsequently captured the actual schema74
stack and completed its offline foundation restore on 2026-09-06. Bundle
`214fd4f027a34604aeb7469d9dfb9a60` was sealed at 04:37:36 UTC; restore
`cea594c6416d42f7a3403aa7509d2c70` passed at 04:39:45 UTC. The exact paths,
manifest hashes, owner registry and source-resume boundary are recorded in
[Current private G74 recovery selection](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).

The current volume's raw physical restoration and coherent logical comparison
passed, including 25 terminal journal runs, valid cursors, two workspace runs
and two terminal tool calls. All pending/invalid counters were zero. The actual
filesystem component restored 16 workspace entries; database restoration
verified 135 main and 19 Honcho tables. The same original sources were restarted;
the restored copies remain inactive.

## Exact selection

The selected controller is
`2ec604cb372a9ca42708d6ca8962b3d66195929f608278cb70b19ba5c339b630`, image
`sha256:679b6f47b6ec04fa7fd8601b19f605efe0736aa840ccc63ec73cabeb643cbfbd`.
The worker image remains `aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a`.
The exact local volume is
`ortak-journal-74-7d40b392f693427caa2e5e2be61d84d9`, created
`2026-09-06T03:49:25Z`, owner ID `7d40b392-f693-427c-aa2e-5e2be61d84d9`.
Name, creation time, local driver/scope/no driver options, both company/owner
labels and the actual controller mount source must all match. Unknown mounts
remain refused. The original host `state` directory is retained crash history
and is never a fallback for this selection.

The first new preparation `7a0bd4ea8936435ab8fee93e2a59bb27` refused before
pause because this controller's daemon reports four bind sources below
`/host_mnt/private/tmp` and its socket source as
`/run/host-services/docker.proxy.sock`. The subsequent read-only mount witness
`g74-daemon-bind-selection-d10b6943549b4dd1b96133409fa3ec63/receipt.json`
(SHA `9a4ed2164b548382eeabc8f2c898c3844d079eff5e5d912514c813adfda2be09`)
records those exact sources. Only controller `2ec604…` accepts these pinned
values. Other service source paths and the failed preparation remain unchanged;
there is no global path normalization or alias fallback.

The completed operation's pre-pause public selection records are under
`/private/tmp/ortak-private-20260905/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6`:

- `current-owners74-volume.json` binds all five native owners. Worker uses
  `launch-ortak-worker-volume74.py`; management uses
  `launch-ortak-management-volume74.py`. Their original receipts are retained.
- `journal-volume-7d40b392f693427caa2e5e2be61d84d9/{receipt.json,controller-active.json,controller/config.json}`
  binds the prepared volume and the corrected controller.
- `files-volume-final-49ee3bf8cdd84b608138c808fbfd6935/receipt.json` preserves the
  successful Files run and its drained observation.
- `office-restore-volume74-6c1eb5e85d274bf9ade2260a493c2ac1/receipt.json` and
  `manual-work-cohort/154db79a3fbc412d96caadcfccb070bf/receipt.json` preserve the
  subsequent ordinary Office baseline: revision
  `61430887-dcc6-4def-8435-cfd723077f69`, routing enabled, empty tools/workspaces
  and a fresh enabled cohort. These are historical observations; current drain
  and process authority must still be observed before any new capture.

## Storage adapter

`private_recovery_journal.py` supplies the explicit storage selection and
read-only volume mount. `None` retains the legacy binding contract for older
frozen operators; the current selection is explicitly the named volume. The
inventory never discovers additional storage or grants authority from its name.

The existing Linux lease holder mounts the selected volume read-only at the
same logical state path and holds the real executor and OAuth locks. It has
no Docker socket, network or application entrypoint. `--init` places Python
below namespace PID1, so its 900-second SIGALRM watchdog also bounds a blocked
pipe after client loss. The bounded RPC accepts only journal status, raw
archive transfer and release, with at most 64 requests. Transfer frames contain
at most 3 KiB of archive bytes; total bytes and the final SHA are verified by
the caller. It creates no extra Docker reader or executor.

Workspace callbacks now read journal status from that same live lock owner.
Capture receives the raw archive from it, with no read of the old host journal.
The transfer admits only the root directory, `journal.sqlite`, zero-byte
`executor.lock`, and optional WAL/SHM files. Missing companions are explicit
metadata. Unknown sidecars, links, extra paths, unexpected ownership/modes or
xattrs refuse. Each file is bounded to 64 MiB, total content to 192 MiB; source
descriptor identities and file generations are checked before and after.

Capture retains `journal-raw.tar` and the exact transferable metadata. The
existing bounded file child physically extracts it to a fresh private host
directory and verifies every byte/mode/timestamp. The parent records and
reaps that child; an unconfirmed stop remains a containment error. Source
UID/GID remain recorded as provenance; inert host files belong to the current
UID. SQLite operates on another working copy, preserving raw WAL/SHM evidence.
The normal coherent `journal.sqlite` backup remains the main journal component.

## Offline result and limits

Offline preflight binds the raw archive to the preparation's exact controller
and volume generation. Restoration physically recreates the raw tree in a fresh
inert directory, makes a coherent working backup from those raw bytes, and
compares all journal tables, rows, cursors and diagnostics against the captured
coherent SQLite backup. Equal counts with different row bytes fail. A tar check
alone cannot return a restored result.

The offline result does not recreate a Docker volume, apply Linux ownership to
the host or start a service. Activation remains closed and explicitly requires
a fresh owned local journal volume and reviewed generation rebinding, in
addition to the existing original-writer containment and same-key recovery
gates. The source volume is never overwritten or recreated by these helpers.

The preceding focused verification covered actual local flock ownership, framed transfer,
physical extraction and release, production capture/restore callers, malformed
storage selection, raw/coherent disagreement and existing barrier/manifest
guards. Host fixtures simulate only the unavailable Linux xattr probe; one
native Linux xattr check was explicitly skipped. Those local fixtures did not
claim installed Docker mount/UID behavior; the later root-executed operation
above supplies actual selected-volume capture and physical restoration evidence.
Its frozen 28-file closure `2d38e3faa196e68063341355ee3fa330e34d2ed64b3af778d541ffccb9878c67`
is historical snapshot provenance, not reusable authority after source resume.
Any later live operation requires new current owners and reviewed preparation.
All previously frozen G69/G73 operations and their successful archives remain
unchanged historical evidence.
