"""WEB-E2E-021: uploads, workspace files and run changes on the real stack, checked on disk afterwards."""

import json
from pathlib import Path
import uuid

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

REPORT = "# Sales report\n\nRegions: north, south, east\nChecked twice.\n"


def verify_overlay(env, configuration):
    """The scripted model may read only the declared files scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/files.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "files.json").resolve()), "Unexpected scenario source"


def verify_result(env):
    """The upload and the agent's files are in the workspace; the snapshots live beside it, never inside it."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "Files result exceeded its bound"
    record = json.loads(result.read_text())
    workspace_id = str(uuid.UUID(record["workspaceId"]))
    root = Path(env["TQA_STORAGE_ROOT"]).resolve()
    workspace = root / "workspaces" / workspace_id
    assert workspace.is_dir() and not workspace.is_symlink(), "Missing workspace"
    upload = workspace / "uploads" / "sales.csv"
    assert upload.is_file() and not upload.is_symlink(), "The upload is not in the workspace"
    assert upload.read_text() == record["upload"], "The stored upload differs from what was sent"
    report = workspace / "report.md"
    assert report.is_file() and report.read_text() == REPORT, "The agent's report is not the second run's"
    snapshots = root / "changes" / workspace_id / "snapshots"
    manifests = sorted(snapshots.glob("*.json"))
    assert len(manifests) == 2, "Expected one snapshot per delivered message"
    for manifest in manifests:
        files = json.loads(manifest.read_text())["files"]
        # Uploads are part of the workspace and a run may change them (their edits are listed); private state never is.
        assert not any(path.startswith(".tqa/") for path in files), "Private state recorded"
    assert not (workspace / "changes").exists(), "Snapshot data must stay outside the workspace"
    assert record["refused"] == [413, "upload_too_large"], "An oversize upload was not refused"
    print("Files: upload stored, report written then revised, two snapshots outside the workspace", flush=True)
