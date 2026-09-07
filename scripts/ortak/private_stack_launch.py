"""Selected native services with bounded rotating logs and process-tree shutdown."""

import json
import logging
from logging.handlers import RotatingFileHandler
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import time

from private_native_services import environment, private_file, selected_root
from private_stack_state import STATE_DIRECTORY, fingerprint, require


def service_environment(root, action):
    """Resolve only this installation's explicit credential references into child env."""
    env = environment(root)
    if action == "relay":
        store = json.loads(private_file(root / "object-store/credentials.json"))
        env.update(ORTAK_CENTRAL_ROUTING_ENABLED="true", BUZZ_S3_ENDPOINT="http://127.0.0.1:9008",
                   BUZZ_S3_ACCESS_KEY=store["access_key"], BUZZ_S3_SECRET_KEY=store["secret_key"],
                   BUZZ_S3_BUCKET="ortak-private-media", BUZZ_S3_REGION="us-east-1")
    if action == "api":
        env["ORTAK_API_CONFIG_JSON"] = private_file(root / "api-config.json")
    if action in ("worker", "management"):
        token = private_file(root / "hermes/controller/service-token").strip()
        env.update(ORTAK_HERMES_BRIDGE_TOKEN=token, ORTAK_HERMES_SERVICE_TOKEN=token,
                   ORTAK_HONCHO_PRIVATE_TOKEN=private_file(root / "honcho/admin-token").strip())
        api = json.loads(private_file(root / "api-config.json"))
        employees = api["humans"][0]["employee_ids"]
        require(0 < len(employees) <= 32)
        for employee in employees:
            require(isinstance(employee, str) and employee.replace("-", "").isalnum())
            config = json.loads(private_file(root / "employees" / employee / "provisioning.json"))
            signer = json.loads(private_file(root / "employees" / employee / "signer.json"))
            key = config["office_signer"]["secret_env"]
            require(key.startswith("ORTAK_") and key.replace("_", "").isalnum())
            env[key] = signer["secret_key"]
    if action == "worker":
        config = private_file(root / "worker-config.json")
        selected = json.loads(config).get("semantic")
        if selected is not None:
            require(selected.get("adapter") == "hermes-codex"
                    and selected.get("bridge_token_env") == "ORTAK_SEMANTIC_SERVICE_TOKEN"
                    and selected.get("deployment", {}).get("bridge_token_ref") == "secret://ortak-private-v0/semantic-service")
            env["ORTAK_SEMANTIC_SERVICE_TOKEN"] = private_file(root / "hermes/semantic/service-token").strip()
        env.update(ORTAK_WORKER_ENABLED="true", ORTAK_WORKER_CONFIG_JSON=config)
    if action == "management":
        env.update(ORTAK_MANAGEMENT_ENABLED="true", ORTAK_MANAGEMENT_ACTION="work",
                   ORTAK_MANAGEMENT_COMMUNITY_ID=api["community_id"])
    return env


def supervise(argv, env, cwd, log_path):
    """Forward termination, bound every log chunk/file, and reap the complete child group."""
    require(not log_path.is_symlink())
    logger = logging.getLogger("ortak-private-service")
    logger.setLevel(logging.WARNING)
    handler = RotatingFileHandler(log_path, maxBytes=4 * 1024**2, backupCount=3, encoding="utf-8")
    logger.addHandler(handler)
    process = subprocess.Popen(argv, env=env, cwd=cwd, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    stopped_at = None

    def terminate(_number, _frame):
        nonlocal stopped_at
        if stopped_at is None:
            stopped_at = time.monotonic()
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass

    previous = {s: signal.signal(s, terminate) for s in (signal.SIGTERM, signal.SIGINT)}
    try:
        with selectors.DefaultSelector() as ready:
            ready.register(process.stdout, selectors.EVENT_READ)
            while True:
                if process.poll() is not None and stopped_at is None:
                    terminate(signal.SIGTERM, None)
                if stopped_at is not None and time.monotonic() - stopped_at >= 40:
                    raise TimeoutError("service_shutdown_timeout")
                if not ready.select(0.5):
                    continue
                chunk = os.read(process.stdout.fileno(), 32768)
                if not chunk:
                    break
                logger.warning("%s", chunk.decode("utf-8", errors="replace").rstrip())
        return process.wait(timeout=3)
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=3)
        process.stdout.close()
        for number, original in previous.items():
            signal.signal(number, original)
        handler.close()
        logger.removeHandler(handler)


def main():
    """Launch only the installed immutable service selection; never search PATH for it."""
    os.umask(0o077)
    root = selected_root(STATE_DIRECTORY)
    manifest = json.loads(private_file(root / "lifecycle/installation.json", 131072))
    action = sys.argv[1]
    selected = manifest["services"][action]
    binary = Path(selected["path"])
    require(fingerprint(binary) == selected["sha256"])
    raise SystemExit(supervise([str(binary)], service_environment(root, action), root,
                               root / "logs" / f"{action}.log"))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("Selected Ortak service refused; private state retained.") from None
