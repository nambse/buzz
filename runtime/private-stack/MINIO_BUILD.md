# Private MinIO source build

This recipe builds the requested official source release
[RELEASE.2025-10-15T17-29-55Z](https://github.com/minio/minio/releases/tag/RELEASE.2025-10-15T17-29-55Z)
from commit
[9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a](https://github.com/minio/minio/commit/9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a).
The release page directs container users to build source. The upstream
[Dockerfile](https://github.com/minio/minio/blob/9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a/Dockerfile)
starts from a floating MinIO image; this private build instead compiles the
verified archive and uses a scratch runtime.

`minio-source-lock.json` records the downloaded archive, exact Go image,
expanded size and nine reviewed source hashes. The archive receipt is:

- File: `/private/tmp/ortak-minio-9e49d5e.tar.gz`
- Bytes: 24,232,282; expanded source: 37,957,841 bytes across 1566 members, no links.
- SHA256: `45521908307306e925c98d629e1c17d78c8b72b6ee242b1bfb1409f7d8ee5841`
- Official builder:
  `golang:1.24.8-alpine@sha256:3d78beb141d98f42337f1252ecf2a5f20374109929a4c3f6817f9e4179cc0ae5`

The inspected [go.mod](https://github.com/minio/minio/blob/9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a/go.mod)
requires Go 1.24.0 and selects toolchain 1.24.8. The builder checks its real Go
version and sets `GOTOOLCHAIN=local`, so it cannot silently download another
compiler. `go build -mod=readonly -tags=kqueue` follows the production build
flags in the pinned Makefile while avoiding its development/debug targets.
Archive and go.mod/go.sum checks run before compilation; module hashes are
checked again afterward. No `go generate`, frontend build, linter, package
manager, installer script, or development tool is invoked.

## Root integration build

The root task owns image building. Prepare a **new dedicated context** containing
only the received archive, named `source.tar.gz`. Do not point Docker at all of
`/private/tmp` or at any private credential/state directory:

```sh
mkdir /private/tmp/ortak-minio-build-input-20260905
cp /private/tmp/ortak-minio-9e49d5e.tar.gz /private/tmp/ortak-minio-build-input-20260905/source.tar.gz
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  buildx build --builder default --provenance=false --load \
  -f /Users/nambse/.codex/worktrees/a5ed/ortak.dev/runtime/private-stack/Dockerfile.minio \
  -t ortak-minio-private:2025-10-15 /private/tmp/ortak-minio-build-input-20260905
```

Only copy the authored Dockerfile/receipt/docs into the repository; the archive
stays outside Git. The build tag is a temporary handle. After success, inspect
the actual image ID and source label, run `--version`, and pin the observed
immutable image in the fresh stack. Source review alone is not a successful
image build or S3 conformance receipt.

The reconstructed command environment and explicit Docker Desktop socket select
only this local daemon. The explicit default builder prevents a previously
selected Buildx instance from redirecting the build elsewhere. Another host
needs an explicitly reviewed local daemon/builder selection; do not inherit
ambient Docker context or endpoint settings.

Parallel compilation is capped at 2 and Go's runtime memory target at 1536 MiB.
Module and compiler caches live only during one build step and are removed
before its layer completes. This avoids retaining duplicate dependency layers
or a global BuildKit cache mount; temporary compilation still needs free disk.
The supplied source expands to about 38 MB. The scratch runtime carries only the
static server binary, CA certificates and upstream license, without Go, a
shell, an SDK client, the source tree or development dependencies. Do not prune
unrelated images/caches to make room; the root task tracks its disk budget.

## Fresh runtime composition

Before the object-store Compose overlay, create its explicit `image.env`
selection. `prepare_private_object_store.py` creates credentials but does not
publish an image selection. For the recorded local artifact, verify the image
identity and source revision:

```sh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  image inspect --format '{{.Id}} {{index .Config.Labels "org.opencontainers.image.revision"}}' \
  sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41
```

The ID must be the requested immutable ID and the revision must be
`9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a`. A future clean build may have a different
image ID. Inspect that completed build's temporary tag, verify its revision and
`--version` output against this source receipt, and substitute the newly
observed `sha256:<64 lowercase hex>` ID below. Never write the floating tag into
the selection file. Run this from the repository root, after preparing the
private object-store directory:

```sh
python3 - sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41 <<'PY'
import re
import sys
from pathlib import Path
sys.path.insert(0, str(Path("scripts/ortak").resolve()))
from init_private_stack import create_file
from private_native_services import private_file, selected_root

image_id = sys.argv[1]
if not re.fullmatch(r"sha256:[0-9a-f]{64}", image_id):
    raise SystemExit("An inspected immutable image ID is required")
root = selected_root(Path("/private/tmp/ortak-private-20260905"))
directory = root / "object-store"
if directory.is_symlink() or not directory.is_dir() or directory.stat().st_mode & 0o077:
    raise SystemExit("Prepare the private object-store directory first")
private_file(directory / "credentials.json", 4096)
destination = directory / "image.env"
expected = "ORTAK_MINIO_IMAGE=" + image_id + "\n"
if destination.exists() or destination.is_symlink():
    if private_file(destination, 256) != expected:
        raise SystemExit("Existing image selection differs; review it explicitly")
else:
    create_file(destination, expected, 0o600)
print("Verified private image selection preserved or published")
PY
```

This step creates only the protected image selection file and refuses to
overwrite a different selection. It does not start a service or inspect any
old bucket. The subsequent Compose command supplies this file with
`--env-file /private/tmp/ortak-private-20260905/object-store/image.env`.

The default image command prints the version and creates no store. The root
stack supplies an explicit server command:

```yaml
command: [server, --address, ':9000', /data]
ports: ['127.0.0.1:9008:9000']
environment:
  MINIO_ROOT_USER_FILE: /run/secrets/minio_root_user
  MINIO_ROOT_PASSWORD_FILE: /run/secrets/minio_root_password
  MINIO_BROWSER: 'off'
  MINIO_UPDATE: 'off'
```

The image's numeric user is 10001:10001. Supply a new writable `/data` volume
owned by that UID, and a small writable `/tmp` tmpfs for HOME/config/cert paths
when using a readonly root filesystem. Publish no console port. Docker socket,
old buckets/volumes, provider profiles and owner credentials are not needed.
The root task creates fresh selected S3 credentials and mounts only those files.

The **native binary** reads both `MINIO_ROOT_USER_FILE` and
`MINIO_ROOT_PASSWORD_FILE`; a shell entrypoint or credential-valued environment
file is unnecessary. This was checked in the actual downloaded
`cmd/common-main.go` and `internal/config/constants.go`. Missing/empty files are
ignored by that upstream loader, so the initializer/composition must require
both fresh nonempty files; do not accept an unauthenticated health response as
proof that the intended credentials loaded.

The actual pinned `cmd/healthcheck-router.go` registers GET and HEAD:

```sh
curl --fail --max-time 3 http://127.0.0.1:9008/minio/health/live
curl --fail --max-time 3 http://127.0.0.1:9008/minio/health/ready
```

These probes need no credentials and can run from the host; the scratch image
has no curl or shell. The fresh relay's **authenticated Git/S3 conformance** is
the required next gate after readiness, using only the new selected bucket.
No service was launched and no bucket or credential was accessed by this source
build lane. The root task records actual image/conformance receipts separately.


## Actual private-stack receipt — 2026-09-05

The integration owner built this recipe successfully. The real binary reports
the selected release and exact commit; go.mod and go.sum remained unchanged.
The Docker-inspected immutable image is:

`sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41`

The owner started only the new private MinIO service at 127.0.0.1:9008 with
native file-backed credentials. Both live and ready probes returned HTTP 200.
Authenticated SigV4 bucket creation and idempotent HEAD passed, followed by the
fresh relay's A3 Git/S3 conformance gate. The relay subsequently opened its
intended loopback listeners at ports 3038, 8089 and 9198. These are actual
integration receipts supplied by the root task; this lane did not access
credential values or old buckets. The temporary build tag is not the pin.
