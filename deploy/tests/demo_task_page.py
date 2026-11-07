"""The feature matrix (G6): Stop, a held message and a long answer on the real stack, with a scripted model that holds
one reply until Stop and streams another slowly (fixtures/task-page.json)."""

import json
from pathlib import Path
import uuid

import compose
import demo_task_page_recovery

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"


def verify_overlay(env, configuration):
    """The scripted model may read only the declared task-page scenario, read-only."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/task-page.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "task-page.json").resolve()), "Unexpected scenario source"
    demo_task_page_recovery.start(env)  # WEB-E2E-042's sandbox killer, on the spec's markers


def verify_result(env):
    """The page stopped the held run (the gateway says cancelled) and followed the long answer to its end."""
    demo_task_page_recovery.verify_result()  # WEB-E2E-042: two kills, each replaced
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 4096, "Task-page result exceeded its bound"
    record = json.loads(result.read_text())
    uuid.UUID(record["held"]["id"])
    uuid.UUID(record["long"]["id"])
    assert record["held"]["run_status"] == "cancelled", "The held run was not stopped"
    print("Task page: Stop ended the held run, a waiting message was cancelled and one dropped; the long answer "
          "copied and followed", flush=True)
