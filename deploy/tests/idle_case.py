"""E2E-IDLE-019: the sandbox lifecycle as designed (ARCHITECTURE §4.4, ADR-0016, GW-I-010 on a real backend).
With a short idle window, a finished task's sandbox is destroyed after the window and not before; its conversation and
workspace stay, and the next message starts a new sandbox on the same files, where the agent reads a file written
before. A stopped run's sandbox waits for the window too. Deleting a task removes its sandbox at once. This module holds
the scenario, the bounds and the checks; robustness.py runs them on the Docker stack.
"""
import time

# The idle window the stack runs with: long enough that "at once" (a delete) and "after the window" can't be confused.
IDLE_SECONDS = 45
SETTINGS = "idle_seconds={}\n".format(IDLE_SECONDS)
# The client sees a run end a little after the gateway stored it, so the window measured here can look slightly short.
EARLY_SECONDS = 3
# The worker's idle pass, the container's stop and its removal on a loaded engine.
LATE_SECONDS = 30
# A deleted task's sandbox is removed by the delete itself, well inside the idle window.
DELETE_SECONDS = 15
# The stopped run's call: still running when the stop arrives, and under the shell tool's 30 s timeout.
STOPPED_CALL_SECONDS = 25
MARKER = "written before the sandbox slept"


# Each task's first message; the fake model answers each task from its own script by turn ("select": "prompt"), so a
# task never gets another's reply whatever the arrival order (the default turn selection would give every new task the
# first reply).
FINISHED = "Write the marker"
STOPPED = "Start a long call"
DELETED = "Say done"


def shell(when, command):
    return {"when": when, "events": [{"type": "tool_call", "name": "shell", "split_into": 1,
                                      "arguments": {"command": command}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


def text(when, words):
    return {"when": when, "events": [{"type": "text", "text": words}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


SCENARIO = {"model": "fixture", "select": "prompt", "responses": [
    # The finished task's first run writes the marker; after its sandbox is destroyed, its second run reads it back.
    shell(FINISHED, "printf '{}' > marker".format(MARKER)), text(FINISHED, "Wrote the marker."),
    shell(FINISHED, "cat marker"), text(FINISHED, "Read the marker in a new sandbox."),
    # The stopped task's run is stopped during this call.
    shell(STOPPED, "echo started && sleep {}".format(STOPPED_CALL_SECONDS)),
    text(STOPPED, "This reply is never asked for: the run was stopped."),
    # The deleted task's run, deleted once it finished.
    text(DELETED, "Done."),
]}


def kinds(stored):
    return [frame.get("event", {}).get("type") for frame in stored]


def results(stored):
    return [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]


def until_gone(present, since, bound, clock=time.monotonic, sleep=time.sleep):
    """Polls `present()` until it is false; returns the seconds from `since` (a `clock()` reading). Fails past `bound`
    seconds from `since`."""
    while present():
        assert clock() - since < bound, "the sandbox was still there {} s later".format(bound)
        sleep(0.5)
    return clock() - since


def check_retired(seconds, what):
    """After the window, and not before it."""
    assert IDLE_SECONDS - EARLY_SECONDS <= seconds <= IDLE_SECONDS + LATE_SECONDS, \
        "{}: the sandbox went {:.0f} s after the run ended (window {} s)".format(what, seconds, IDLE_SECONDS)
    return "{}: destroyed {:.0f} s after the run ended (window {} s)".format(what, seconds, IDLE_SECONDS)


def check_deleted(seconds):
    assert seconds <= DELETE_SECONDS, "the deleted task's sandbox went {:.0f} s after the delete".format(seconds)
    return "deleting the task removed its sandbox {:.0f} s after the delete".format(seconds)


def check_woken(stored, first, second):
    """One conversation across both sandboxes: two completed runs, the marker read back in the second, contiguous."""
    order = kinds(stored)
    assert order.count("run.completed") == 2 and order[-1] == "run.completed", order
    assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1)), "one contiguous conversation"
    calls = results(stored)
    assert [call["status"] for call in calls] == ["ok", "ok"], calls
    assert calls[-1]["content"].strip() == MARKER, calls[-1]
    assert first != second, "the next message started a new sandbox"
    return "the next message started a new sandbox on the same workspace, where the agent read the file written before"
