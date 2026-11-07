"""WEB-E2E-011: two tasks on one workspace take turns; a fork runs at once in its own copy. Checked on disk."""

import json
from pathlib import Path
import uuid

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

MARK = "run,"
# Each task's first message names its script (the scripted model's prompt selection, TK-U-036).
TASKS = {"first": "Record the first run", "fork": "Record a run in the copy", "second": "Record the second run"}


def configure(directory, env):
    """Nothing beyond the base: the first task holds the lease in a held model call until Stop (ADR-0035)."""


def verify_overlay(env, configuration):
    """The scripted model may read only the declared workspaces scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/workspaces.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "workspaces.json").resolve()), "Unexpected scenario source"


def _marks(root, identity):
    workspace = root / "workspaces" / str(uuid.UUID(identity))
    assert workspace.is_dir() and not workspace.is_symlink(), "Missing workspace"
    assert root in workspace.resolve().parents, "The workspace escaped the storage root"
    runs = workspace / "runs.txt"
    assert runs.is_file() and not runs.is_symlink(), "No run wrote its mark"
    assert runs.stat().st_size <= 256, "Unexpected marker size"
    return runs.read_text()


def verify_result(env):
    """The first task held the shared workspace and was stopped before its mark; the second then ran there; the fork's
    copy holds only its own run."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "Workspaces result exceeded its bound"
    record = json.loads(result.read_text())
    root = Path(env["TQA_STORAGE_ROOT"]).resolve()
    assert record["workspace"] != record["fork"], "The fork must be a different workspace"
    assert _marks(root, record["workspace"]) == MARK, "The shared workspace must hold the second task's run only"
    assert _marks(root, record["fork"]) == MARK, "The fork must hold only its own run"
    assert record["order"] == ["first", "fork", "second"], "Unexpected run order: {}".format(record["order"])
    print("Workspaces: the second task waited for the first to stop, then ran on the workspace; the fork ran at once in "
          "its own copy", flush=True)
