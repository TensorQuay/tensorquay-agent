"""WEB-E2E-018: real LAN origin and filesystem evidence for the long scenario."""

import ipaddress
import json
from pathlib import Path
import uuid

import compose


def lan_host(env):
    value = env.get("TQA_UX_HOST")
    if not value:
        script = ("const entries=Object.values(require('node:os').networkInterfaces()).flat();"
                  "console.log(JSON.stringify(entries.find(e=>e.family==='IPv4'&&!e.internal)?.address??null))")
        value = json.loads(compose.command(["node", "-e", script], env, capture=True).stdout)
    if not isinstance(value, str):
        raise ValueError("The UX suite needs a non-loopback IPv4 interface or TQA_UX_HOST")
    address = ipaddress.IPv4Address(value)
    if address.is_loopback or address.is_unspecified or address.is_multicast:
        raise ValueError("TQA_UX_HOST must be a non-loopback unicast IPv4 address")
    return str(address)


def verify_workspaces(env):
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 65536, "UX result exceeded its bound"
    identities = json.loads(result.read_text())["conversations"]
    assert isinstance(identities, list) and 1 <= len(identities) <= 24, "Expected a bounded conversation list"
    identities = [str(uuid.UUID(value)) for value in identities]
    assert len(set(identities)) == len(identities), "Expected unique conversation IDs"
    root = (Path(env["TQA_STORAGE_ROOT"]) / "workspaces").resolve()
    for identity in identities:
        workspace = root / identity
        assert root in workspace.resolve().parents, "The workspace escaped its storage root"
        for relative, expected in (
            ("demo/arithmetic.py", "def add(left, right):\n    return left + right\n"),
            ("demo/README.md", "# Arithmetic demo\nVerified: two arithmetic tests pass.\n"),
        ):
            target = workspace / relative
            assert target.is_file(), "The runtime did not create " + relative
            assert workspace.resolve() in target.resolve().parents, "The created file escaped its workspace"
            assert target.stat().st_size <= 4096 and target.read_text() == expected, "Unexpected " + relative
    print("Host workspaces: {} separate conversations contain the corrected code and verified README".format(len(identities)), flush=True)
