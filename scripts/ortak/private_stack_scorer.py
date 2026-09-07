"""Explicitly register one stopped, prepared scoring service in the persistent lifecycle."""

import json

from private_native_services import private_file
from private_stack_state import PROJECT, container_contract, fingerprint, inspect_container, require, save_json


def select_scorer(installation):
    """Verify prepared resources; registration starts no listener or provider request."""
    installation.verify()
    root = installation.root
    selected = json.loads(private_file(root / "hermes/semantic/selection.json"))
    require(selected["format"] == "ortak-private-scorer/1"
            and selected["company_id"] == installation.manifest["company_id"])
    row = inspect_container(selected["container_id"])
    actual = container_contract(row)
    if "semantic" in installation.manifest["containers"]:
        require(actual == installation.manifest["containers"]["semantic"] == selected["contract"])
        return {"scorer": "already_registered", "provider_health": "not_probed"}
    require(not row["State"]["Running"] and row["State"]["Status"] == "created"
            and row["Name"] == f"/{PROJECT}-semantic"
            and row["Image"] == selected["image"] and actual == selected["contract"])
    labels = row["Config"].get("Labels", {})
    require(labels.get("org.ortak.role") == "semantic-scorer"
            and labels.get("org.ortak.company") == selected["company_id"]
            and labels.get("org.ortak.deployment") == selected["deployment_id"])
    security = actual["scorer_contract"]
    require(security["user"] == "10001:10001" and security["ReadonlyRootfs"] is True
            and security["Init"] is True and security["stop_timeout"] == 45
            and security["CapDrop"] == ["ALL"] and "no-new-privileges" in security["SecurityOpt"]
            and 0 < security["PidsLimit"] <= 128 and 0 < security["Memory"] <= 512 * 1024**2
            and 0 < security["NanoCpus"] <= 2_000_000_000
            and security["NetworkMode"] == "ortak-private-v0-runtime"
            and actual["restart"] == {"Name": "unless-stopped", "MaximumRetryCount": 0}
            and actual["ports"] == {"8651/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8651"}]})
    config_path = root / "hermes/semantic/config.json"
    require(fingerprint(config_path) == selected["config_sha256"])
    config = json.loads(private_file(config_path))
    require(config["company_id"] == selected["company_id"]
            and config["semantic"]["deployment_id"] == selected["deployment_id"]
            and config["semantic"]["binding_sha256"] == selected["binding_sha256"])
    # The exact shared OAuth parent is the only writable host mount. No Docker
    # socket, employee workspace or journal is accessible to this listener.
    expected = {str(root / "hermes/oauth"): (str(root / "hermes/oauth"), True),
                str(config_path): ("/private/semantic-config.json", False),
                str(root / "hermes/semantic/service-token"): ("/private/semantic-service-token", False)}
    require(len(actual["mounts"]) == 3)
    for mount in actual["mounts"]:
        source = mount["Source"].removeprefix("/host_mnt")
        require(mount["Type"] == "bind" and source in expected
                and (mount["Destination"], mount["RW"]) == expected[source])
    argv = security["entrypoint"] + security["cmd"]
    require(argv == ["python", "-m", "ortak_hermes_bridge.semantic", "--config", "/private/semantic-config.json",
                     "--token-file", "/private/semantic-service-token", "--port", "8651",
                     "--listen-address", "0.0.0.0", "--enable-selected-semantic-oauth"])
    installation.record("select-scorer", "registering_stopped_scorer")
    save_json(installation.directory / "installation.before-scorer.json", installation.manifest)
    installation.manifest["containers"]["semantic"] = actual
    save_json(installation.directory / "installation.json", installation.manifest)
    installation.verify()
    installation.record("select-scorer", "registered_not_started")
    return {"scorer": "registered_not_started", "provider_health": "not_probed"}
