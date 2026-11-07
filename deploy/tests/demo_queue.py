"""WEB-E2E-018: one real admission slot, held by a scripted model call until the holder presses Stop."""

import json
from pathlib import Path

import compose

# This file's fixtures, found from its own location so the suite passes from any working directory.
FIXTURES = Path(__file__).resolve().parent / "fixtures"

# The first message of each task names its script (the scripted model's prompt selection, TK-U-036).
HOLD = "Hold the slot"
QUEUED = "Wait for a free slot"


def configure(directory, env):
    """One admission slot for everyone; every call runs (ADR-0035), so nothing else is configured."""
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    compose.write(config, config.read_text() + "\n[admission]\nslots=1\nper_user=1\n")
    compose.ownership(directory, env, "1000:1000")


def verify_overlay(env, configuration):
    """The scripted model may read only the declared two-turn admission scenario."""
    config = json.loads(compose.command(configuration + ["config", "--format", "json"], env, capture=True).stdout)
    mounts = config["services"]["fake-model"].get("volumes", [])
    assert len(mounts) == 1, "Unexpected model fixture mounts"
    mount = mounts[0]
    assert mount.get("type") == "bind" and mount.get("read_only") is True, "The scenario mount must be read-only"
    assert mount.get("target") == "/scenarios/queue.json", "Unexpected scenario target"
    assert mount.get("source") == str((FIXTURES / "queue.json").resolve()), "Unexpected scenario source"


# The browser runs WEB-E2E-018 at two widths in two colour schemes.
WIDTHS = 4


def verify_result(env):
    """Per width, the browser's pair: the holder cancelled (stopped), then the queued task completed, as two different
    tasks. The outcomes are each task's run_status from the gateway's task list, which the browser read; the frames it
    keeps are the evidence of the queue itself."""
    result = Path(env["TQA_STACK_RESULT"])
    assert result.stat().st_size <= 16384, "Queue result exceeded its bound"
    runs = json.loads(result.read_text())["runs"]
    assert len(runs) == 2 * WIDTHS, "Expected a holder and a queued task per width: {}".format(len(runs))
    for holder, queued in zip(runs[::2], runs[1::2]):
        assert (holder["outcome"], queued["outcome"]) == ("cancelled", "completed"), "Unexpected outcomes: {}".format(
            (holder["outcome"], queued["outcome"]))
        assert holder["conversationId"] and queued["conversationId"], "A task without its id"
        assert holder["conversationId"] != queued["conversationId"], "The holder and the queued task are one task"
    print("Queue: at each width the held slot was freed by Stop and the queued task then ran", flush=True)
