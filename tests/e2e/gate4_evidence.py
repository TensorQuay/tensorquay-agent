"""E2E-TEAM-011: the pure parts of the Gate 4 demo: the scripted model replies, the checks over stored events and
the readable transcript. No I/O, so each part is unit-tested (test_gate4.py)."""

from datetime import datetime
import json

BRANCH_A, BRANCH_B = "demo/a", "demo/b"
# Bounds: the result stays small enough to publish beside the environment record, and a transcript line stays short.
RESULT_BYTES = 16384
DETAIL_CHARS = 160


# Each run's first message names its script (the scripted model's prompt selection, TK-U-036).
MESSAGE_A = "Push the shared work to demo/a."
MESSAGE_FORK = "Push the fork's work to demo/b."
MESSAGE_WAIT = "Summarise the shared workspace."
# Each person's own host key for the origin (ADR-0037): a task sees its name as a placeholder, which the egress proxy
# swaps for the value on the way to that host. The Git relay and its credentials are retired (ADR-0037 §7).
KEY = "GATE4_ORIGIN"
PUSH = 'git -c "http.extraHeader=Authorization: Bearer ${%s}" push -q origin HEAD:refs/heads/' % KEY
# A's push step is held this long (TK-U-035), keeping the workspace lease while B forks, runs and pushes; ADR-0035 left
# no approval to hold it at.
HOLD_MS = 240000


def is_push(arguments):
    """Whether a shell call is the demo's push: plain git with the person's key."""
    return str(arguments.get("command", "")).startswith(PUSH)


def replies():
    """Each run's own replies: A checks out its branch and its push is held; B's fork run checks out and pushes at
    once; B's waiting run answers once the lease is free. Each pushes with plain git and its person's key."""
    push = lambda branch: ("shell", {"command": PUSH + branch})
    scripts = [(MESSAGE_A, [("shell", {"command": "git checkout -b " + BRANCH_A}), push(BRANCH_A),
                            "Pushed {}.".format(BRANCH_A)]),
               (MESSAGE_FORK, [("shell", {"command": "git checkout -b " + BRANCH_B}), push(BRANCH_B),
                               "Pushed {} from the fork.".format(BRANCH_B)]),
               (MESSAGE_WAIT, ["The shared workspace was free again, so this task ran."])]
    responses = []
    for message, steps in scripts:
        for step in steps:
            events = ([{"type": "text", "text": step}] if isinstance(step, str) else
                      [{"type": "tool_call", "name": step[0], "arguments": step[1], "split_into": 1}])
            response = {"when": message, "events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}}
            if message == MESSAGE_A and isinstance(step, tuple) and is_push(step[1]):
                response["wait_ms"] = HOLD_MS
            responses.append(response)
    return {"model": "fixture", "select": "prompt", "responses": responses}


def kind(frame):
    return frame.get("event", {}).get("type", frame.get("type"))


def when(frame):
    return datetime.fromisoformat(frame["ts"].replace("Z", "+00:00"))


def run_interval(stored):
    """The first run's start and completion times; a run that never completed fails the demo."""
    started = next((when(frame) for frame in stored if kind(frame) == "run.started"), None)
    completed = next((when(frame) for frame in stored if kind(frame) == "run.completed"), None)
    if started is None or completed is None:
        raise AssertionError("A demo run did not start and complete: " + ", ".join(map(kind, stored))[-400:])
    return started, completed


def check_order(holder, fork, waiting):
    """The fork's run lies inside the lease holder's run; the waiting run starts only after the holder completed."""
    (a_start, a_end), (f_start, f_end), (w_start, _) = map(run_interval, (holder, fork, waiting))
    if not a_start < f_start <= f_end < a_end:
        raise AssertionError("The fork run did not run while the lease holder's run was active")
    if not a_end <= w_start:
        raise AssertionError("The waiting run started before the lease holder's run completed")
    return {"holder": [a_start.isoformat(), a_end.isoformat()], "fork": [f_start.isoformat(), f_end.isoformat()],
            "waiting_started": w_start.isoformat()}


def push_outcomes(stored):
    """Each push call's result status, in order: the shell calls that push with the person's key."""
    calls = {frame["event"]["call_id"]: frame for frame in stored
             if kind(frame) == "tool.call" and frame["event"]["name"] == "shell"
             and is_push(frame["event"].get("arguments", {}))}
    results = [frame["event"]["status"] for frame in stored
               if kind(frame) == "tool.result" and frame["event"]["call_id"] in calls]
    return {"calls": len(calls), "results": results}


def check_push(stored, branch):
    outcome = push_outcomes(stored)
    if outcome != {"calls": 1, "results": ["ok"]}:
        raise AssertionError("The {} push was not one call and one success: {}".format(branch, json.dumps(outcome)))
    return outcome


def check_refs(refs, commit):
    expected = {"refs/heads/main": commit, "refs/heads/" + BRANCH_A: commit, "refs/heads/" + BRANCH_B: commit}
    missing = {name: refs.get(name) for name, value in expected.items() if refs.get(name) != value}
    if missing:
        raise AssertionError("The origin does not hold both branches at the pushed commit: " + json.dumps(missing))
    return sorted(expected)


def check_clean(texts, secrets_):
    """No recorded text holds a credential, a token or a password."""
    for name, text in texts.items():
        for secret in secrets_:
            if secret and secret in text:
                raise AssertionError("A secret reached the recorded " + name)
        if "tqa_pat_" in text:
            raise AssertionError("An API token reached the recorded " + name)


def detail(frame):
    event = frame.get("event", {})
    kinds = {"message.user": lambda: event["text"],
             "tool.call": lambda: event["name"] + " " + json.dumps(event["arguments"], sort_keys=True),
             "tool.result": lambda: event["status"] + ": " + event["content"].splitlines()[0] if event["content"] else event["status"],
             "text.delta": lambda: event["text"]}
    text = kinds.get(kind(frame), lambda: "")()
    return text if len(text) <= DETAIL_CHARS else text[:DETAIL_CHARS - 1] + "…"


SHOWN = {"message.user", "run.started", "tool.call", "tool.result", "text.delta", "run.completed", "run.failed"}


def transcript(conversations, notes):
    """A readable timeline per conversation, then the orchestrator's own observations (queue state, refusals)."""
    lines = ["# Gate 4 demo transcript (E2E-TEAM-011)", ""]
    for title, stored in conversations:
        lines += ["## " + title, ""]
        for frame in stored:
            if kind(frame) in SHOWN:
                text = detail(frame)
                stamp = "`{}` ".format(frame["ts"]) if frame.get("ts") else ""
                lines.append("- {}{}{}".format(stamp, kind(frame), " — " + text if text else ""))
        lines.append("")
    lines += ["## Observations", ""] + ["- " + note for note in notes] + [""]
    return "\n".join(lines)


def result(record):
    text = json.dumps(record, indent=2, sort_keys=True) + "\n"
    if len(text) > RESULT_BYTES:
        raise AssertionError("The demo result exceeded its bound")
    return text
