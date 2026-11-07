"""GW-I-066 on a real backend (ARCHITECTURE §4.4, reaper rule 1): a sandbox still busy at its max lifetime plus the
grace is retired while busy, and the run resumes from the journal in a new sandbox and completes. The rule lives in the
gateway's manager, not in a backend, so the Docker stack (robustness.py) and the Firecracker stack (contract.py) run
this same scenario and checks; they differ only in how they start the stack, read its events and read the gateway log.
"""
import time

# The shortest max lifetime the configuration accepts, and the manager's grace for a busy sandbox (LIFETIME_GRACE).
MAX_LIFETIME_SECONDS = 60
GRACE_SECONDS = 120
SETTINGS = "max_lifetime_seconds={}\n".format(MAX_LIFETIME_SECONDS)
# The run keeps its sandbox busy past the cap and the grace with calls that each stay under the shell tool's 30 s
# timeout: 11 x 20 s from the sandbox's start ends past 180 s, and what is left after the retirement (two calls or so)
# stays well inside the new sandbox's own cap and grace, so exactly one call is interrupted.
STEP_SECONDS = 20
STEPS = 11
# The retirement comes from the worker's 250 ms pass once the sandbox is 180 s old; the sandbox starts after the
# conversation exists, and a slow start (up to ~30 s on a saturated disk) or the stop itself can add to that.
LATE_SECONDS = 90
LOG_LINE = "sandbox reached its max lifetime"


def shell(command):
    return {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


SCENARIO = {"model": "fixture", "responses": (
    [shell("printf kept > marker")]
    + [shell("echo step {0} && sleep {1} && echo done {0}".format(step, STEP_SECONDS)) for step in range(1, STEPS + 1)]
    + [shell("cat marker"),
       {"events": [{"type": "text", "text": "Resumed after the lifetime cap."}],
        "usage": {"prompt_tokens": 20, "completion_tokens": 10}}])}


def kinds(stored):
    return [frame.get("event", {}).get("type") for frame in stored]


def results(stored):
    return [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]


def watch(fetch, seconds=STEPS * STEP_SECONDS + GRACE_SECONDS + LATE_SECONDS, clock=time.monotonic,
          sleep=time.sleep):
    """Polls `fetch()` (the stored events) until the run ends; returns them and the seconds from the call to the first
    interrupted tool result, or None when no call was interrupted. The caller starts it right after creating the
    conversation, so the time is an upper bound on the sandbox's age at the retirement."""
    started, interrupted = clock(), None
    while True:
        stored = fetch()
        if interrupted is None and any(result.get("status") == "interrupted" for result in results(stored)):
            interrupted = clock() - started
        if any(kind in ("run.completed", "run.failed") for kind in kinds(stored)):
            return stored, interrupted
        assert clock() - started < seconds, "the run did not end within {} s".format(seconds)
        sleep(0.5)


def check(stored, interrupted, log):
    """Rule 1's busy path: not before the cap plus the grace, one interrupted call, one resume in a new sandbox that
    kept the workspace, a completed run, and the gateway's own record of why it retired the sandbox."""
    order = kinds(stored)
    assert order and order[-1] == "run.completed", order
    assert order.count("run.resumed") == 1, order
    statuses = [result["status"] for result in results(stored)]
    assert len(statuses) == STEPS + 2 and statuses.count("interrupted") == 1, statuses
    assert set(statuses) == {"ok", "interrupted"}, statuses
    assert results(stored)[-1]["content"].strip() == "kept", results(stored)[-1]
    bound = MAX_LIFETIME_SECONDS + GRACE_SECONDS
    assert interrupted is not None and bound <= interrupted <= bound + LATE_SECONDS, \
        "the busy sandbox was retired {} s after the conversation began (expected {}..{} s)".format(
            interrupted, bound, bound + LATE_SECONDS)
    retired = [line for line in log.splitlines() if LOG_LINE in line]
    assert len(retired) >= 1 and "busy" in retired[0], retired or "no '{}' line in the gateway log".format(LOG_LINE)
    return "GW-I-066: the busy sandbox was retired {:.0f} s in (cap {} s + grace {} s); the run resumed in a new " \
           "sandbox on the same workspace and completed".format(interrupted, MAX_LIFETIME_SECONDS, GRACE_SECONDS)
