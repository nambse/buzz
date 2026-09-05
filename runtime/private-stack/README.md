# Private development infrastructure

This Compose project creates separate PostgreSQL and Redis stores for the
private Ortak loop. It does not reuse the existing schema-test database,
Mezaton containers, Coolify volumes or employee profiles. Both host ports bind
only to loopback; Redis and PostgreSQL require freshly generated credentials.
Image digests record the locally available artifacts used for this integration.

From the repository root:

```sh
python3 scripts/ortak/init_private_stack.py --state-dir /private/tmp/ortak-private-20260905
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  compose --project-name ortak-private-20260905 \
  --env-file /private/tmp/ortak-private-20260905/compose.env \
  -f runtime/private-stack/compose.yaml up -d --wait --wait-timeout 90
```

Every Docker command in this local recipe reconstructs its environment and pins
the exact Docker Desktop socket. Compose also pins the project name explicitly,
so ambient `DOCKER_HOST`, `DOCKER_CONTEXT` or `COMPOSE_PROJECT_NAME` cannot select
another daemon or project. These commands target this specific macOS host;
another installation needs its own explicitly reviewed daemon selection.

Initialization refuses an existing unmarked directory. Re-running against the
completed marked directory preserves all credentials. Generated state lives
outside the repository beneath a mode0700 directory. `runtime.env` contains
credentials for the native Ortak services and is mode0600; do not copy it into
Git, reports or logs. The container-mounted secret files are readable by their
service UIDs, while their parent directory remains private on the host.

This dated Compose project accepts only the documented canonical state directory.
It refuses another state path, so a second initialization cannot silently reuse
the same named volumes with newly generated credentials.

`scripts/ortak/private_native_services.py` reconstructs each native process's
environment from exactly the two private store URLs and a fresh identity bundle.
It does not inherit provider, proxy, profile or database overrides. Run its
`prepare` action once, then `migrate`; these preserve completed identities and
apply the normal relay migrations to localhost55433. The `relay` action keeps
central routing disabled and binds application/health/metrics to127.0.0.1 on
3038/8089/9198. The `api` action additionally requires an explicit private
`api-config.json` containing public audience grants and listens on8787.

```sh
python3 scripts/ortak/private_native_services.py --state-dir /private/tmp/ortak-private-20260905 --binary-dir /private/tmp/ortak-root-build-target/debug prepare
python3 scripts/ortak/private_native_services.py --state-dir /private/tmp/ortak-private-20260905 --binary-dir /private/tmp/ortak-root-build-target/debug migrate
```

The retained relay requires the isolated object store for its startup Git
conformance check. Keep that check enabled. Follow `MINIO_BUILD.md` for the
reviewed source build; the actual selected local image is
`sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41`.
Prepare its fresh credentials, publish the verified immutable image selection
using the `MINIO_BUILD.md` image-pin step, then use the additional Compose file.
The credential helper does not create `object-store/image.env` itself:

```sh
python3 scripts/ortak/prepare_private_object_store.py --state-dir /private/tmp/ortak-private-20260905
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  compose --project-name ortak-private-20260905 \
  --env-file /private/tmp/ortak-private-20260905/compose.env \
  --env-file /private/tmp/ortak-private-20260905/object-store/image.env \
  -f runtime/private-stack/compose.yaml \
  -f runtime/private-stack/object-store.compose.yaml up -d minio
python3 scripts/ortak/initialize_private_bucket.py --state-dir /private/tmp/ortak-private-20260905
python3 scripts/ortak/private_native_services.py --state-dir /private/tmp/ortak-private-20260905 --binary-dir /private/tmp/ortak-root-build-target/debug relay
```

Provision the fresh Ada memory bundle separately using
[`MEMORY_BOOTSTRAP.md`](MEMORY_BOOTSTRAP.md). Its helper freezes the server
receipt and native resource IDs, exercises a diagnostic remember/recall, and
writes a protected worker configuration fragment without activating routing.

MinIO uses loopback9008 and its own volume. Its signed bucket creation and the
relay's actual conformance check passed. Relay health endpoints are
`http://localhost:8089/_liveness` and `http://localhost:8089/_readiness`.

After creating a private channel through `buzz channels create`, use
`bootstrap_private_control.py --state-dir ... --community <UUID> --channel <UUID>`.
It checks the selected fresh owner's live channel membership and commits the
company, Office binding and draft employee in one transaction. It then publishes
the private API audience file; interrupted publication can be retried without
recreating identities. Start `api` with the same native service launcher.
`node desktop/scripts/ortak-private-api-check.mjs` checks actual signed API
requests against the newly bootstrapped draft state.

The private desktop package is built with
`node desktop/scripts/ortak-private-native.mjs build`. Launch it with the explicit
`desktop` action and `--binary-dir` set to its `Ortak Private.app/Contents/MacOS`
directory. That action verifies the bundle identifier and selects the new test
owner from private state in the process environment. It does not overwrite any
desktop/keyring identity. Directly opening the bundle uses its separately saved
private-app identity, which has not been granted the API audience. The package
automatically connects only to the compiled localhost3038 Office.

PostgreSQL uses localhost55433 and Redis localhost56382. Existing disposable
Rust test databases continue using localhost55432. The infrastructure does not
run migrations, create employees, enable central routing or launch Hermes by
itself. Those remain explicit composition and verified activation steps.

Inspect or stop only this project, retaining its volumes:

```sh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  compose --project-name ortak-private-20260905 \
  --env-file /private/tmp/ortak-private-20260905/compose.env \
  -f runtime/private-stack/compose.yaml ps
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  compose --project-name ortak-private-20260905 \
  --env-file /private/tmp/ortak-private-20260905/compose.env \
  -f runtime/private-stack/compose.yaml stop
```

The base stop command covers PostgreSQL and Redis. Use the full overlay stop
command in [`OPERATIONS.md`](OPERATIONS.md) to include the selected MinIO store,
after quiescing its writers.

Do not use volume-removal commands as routine cleanup. This local development
recipe is not evidence of a deployed or complete product workflow.
