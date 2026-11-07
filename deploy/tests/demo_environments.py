"""E2E-ENV-010: two immutable templates and evidence from the retained workspace."""

import json
from pathlib import Path
import re
import uuid

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

TEST_FILE = "def test_workspace_survives():\n    assert 2 + 2 == 4\n"


def verify_overlay(env, configuration):
    """The scenario may expose only its declared fixture, read-only, to the scripted model."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/environment.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "environment.json").resolve()), "Unexpected scenario source"


def configure(directory, env, images):
    """Write the approved template catalogue only after both local image IDs are validated."""
    if set(images) != {"minimal", "workstation"} or not all(
            re.fullmatch(r"sha256:[0-9a-f]{64}", value) for value in images.values()):
        raise ValueError("Both environment images must resolve to immutable local IDs")
    config = directory / "gateway.toml"
    compose.ownership(directory, env)
    content = config.read_text()
    for name, label, capabilities in (
            ("workstation", "Workstation", ["shell", "python", "node", "build", "browser", "data"]),
            ("minimal", "Minimal", ["shell"])):
        content += "\n[[templates]]\nname={}\ndisplay_name={}\nimage={}\ncapabilities={}\ndefault={}\n".format(
            json.dumps(name), json.dumps(label), json.dumps(images[name]),
            json.dumps(capabilities), "true" if name == "workstation" else "false")
    compose.write(config, content)
    compose.ownership(directory, env, "1000:1000")


def verify_workspace(env):
    """Require the original test and the browser output inside the reported conversation workspace."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 4096, "Environment result exceeded its bound"
    identity = str(uuid.UUID(json.loads(result.read_text())["conversationId"]))
    root = (Path(env["TQA_STORAGE_ROOT"]) / "workspaces").resolve()
    workspace = root / identity
    assert root in workspace.resolve().parents, "The workspace escaped its storage root"
    folder = workspace / "environment"
    for name in ("test_persisted.py", "page.png"):
        target = folder / name
        assert target.is_file() and workspace.resolve() in target.resolve().parents, "Missing or escaped " + name
        assert 0 < target.stat().st_size <= 2 * 1024 * 1024, "Unexpected file size"
    assert (folder / "test_persisted.py").read_text() == TEST_FILE, "The original test was changed"
    with (folder / "page.png").open("rb") as screenshot:
        assert screenshot.read(8) == b"\x89PNG\r\n\x1a\n", "The browser output is not PNG"
    print("Host workspace: the original test survived and the browser wrote environment/page.png", flush=True)
