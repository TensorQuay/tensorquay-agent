"""SEC-002 and TOOL-S-017 through real model, runtime and file-tool boundaries."""
import json
from pathlib import Path
import shlex
import time


def vectors():
    return [("../x", "path_outside_workspace"), ("/etc/passwd", "path_outside_workspace"),
            ("escape", "path_outside_workspace"), ("dir/../../x", "path_outside_workspace"),
            ("nul\0path", "path_invalid"), ("x" * 5000, "path_invalid")]


# ADR-0035 (8c7c2d8d): the file tools may write inside the workspace's .git, as TOOL-U-004 expects; hooks and the
# file-system monitor stay off for every git the tools start (SEC-045). So a .git/config write is a positive control
# here, not an escape: the path contract (nothing outside the workspace) is what these vectors test.
GIT_CONFIG = "git-config-by-file-tool"


def attempts():
    """The escapes: each is refused with its typed code."""
    calls = [("read_file", {"path": path}, code) for path, code in vectors()]
    for name in ["write_file", "edit_file"]:
        for path, code in vectors():
            args = {"path": path}
            if name == "write_file":
                args.update(content="escape-attempt", create_parents=False)
            else:
                args["edits"] = [{"old_text": "marker", "new_text": "escaped"}]
            calls.append((name, args, code))
    return calls


def allowed():
    """Writes inside the workspace that must succeed, .git included (ADR-0035)."""
    return [("write_file", {"path": ".git/config", "content": GIT_CONFIG, "create_parents": False})]


def shell(stage):
    source = Path(__file__).with_name("confinement_probe.py").read_text()
    return "python3 -c " + shlex.quote(source) + " " + stage


def scenario():
    def call(name, arguments):
        return {"type": "tool_call", "name": name, "arguments": arguments, "split_into": 1}

    steps = [[call("shell", {"command": shell("setup")})],
             [call(name, args) for name, args, _ in attempts()]
             + [call(name, args) for name, args in allowed()]
             + [call("read_file", {"path": "inside"})],
             [call("shell", {"command": shell("verify")})],
             [{"type": "text", "text": "Confinement checks completed."}]]
    return {"model": "fixture", "responses": [
        {"events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}}
        for events in steps]}


def check(stack, stream):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        frame = stream.next()
        assert frame["type"] != "error", "runtime notice: " + frame.get("code", "unknown")
        kind = frame.get("event", {}).get("type")
        assert kind != "run.failed", "the confinement run failed"
        if kind == "run.completed":
            break
    else:
        raise AssertionError("the confinement run did not complete")
    frames = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
    assert [frame["seq"] for frame in frames] == list(range(1, len(frames) + 1))
    events = [frame["event"] for frame in frames if frame.get("event")]
    calls = [event for event in events if event["type"] == "tool.call"]
    results = [event for event in events if event["type"] == "tool.result"]
    expected, permitted = attempts(), allowed()
    assert len(calls) == len(results) == len(expected) + len(permitted) + 3, "a scripted call or result is missing"
    by_id = {event["call_id"]: event for event in results}
    assert len(by_id) == len(results), "duplicate durable tool result"
    assert calls[0]["name"] == "shell" and calls[-1]["name"] == "shell"
    for call, (name, arguments, code) in zip(calls[1:1 + len(expected)], expected):
        assert (call["name"], call["arguments"]) == (name, arguments)
        result = by_id[call["call_id"]]
        assert result["status"] == "error", "file-tool escape succeeded: {} {}".format(name, arguments["path"][:40])
        assert result["content"].startswith(code + ":"), "wrong typed confinement refusal"
        assert "outside-read-marker" not in result["content"], "outside content was read"
    for call, (name, arguments) in zip(calls[1 + len(expected):-2], permitted):
        assert (call["name"], call["arguments"]) == (name, arguments)
        assert by_id[call["call_id"]]["status"] == "ok", "a write inside the workspace was refused: " + arguments["path"]
    for call, witness in [(calls[0], "absolute workspace, tmp and own-state writes succeeded"),
                          (calls[-2], "workspace-positive"),
                          (calls[-1], "outside canaries unchanged; root and token writes refused")]:
        result = by_id[call["call_id"]]
        assert result["status"] == "ok" and witness in result["content"], "positive control failed"
    workspace = Path(stack.env["TQA_STORAGE_ROOT"]) / "workspaces" / stack.conversation
    # The installation belongs to the sandbox's user while the stack runs (rootless Docker): hand it back to the host
    # user before reading the workspace from the host, as the push and clone checks do. Imported here, as a top-level
    # module: this file is also loaded on its own by the microVM contract (deploy/firecracker/contract.py), where a
    # package-relative import fails; both callers have deploy/tests on the path by now.
    import compose

    compose.ownership(stack.directory, stack.env)
    assert (workspace / "inside").read_text() == "workspace-positive"
    assert (workspace / ".git/config").read_text() == GIT_CONFIG, "the .git/config write didn't land in the workspace"
    assert not (workspace.parent / "x").exists(), "host workspace traversal wrote a file"
    assert not any(frame["type"] == "permission.request" for frame in frames)
    print("SEC-002/TOOL-S-017: {} file-tool escapes refused; a .git/config write inside the workspace allowed "
          "(ADR-0035); absolute workspace/tmp/state writes allowed; root/token writes refused; outside canaries "
          "unchanged".format(len(expected)), flush=True)


def diagnostics(stack):
    """Report sequence positions and error classes without logging frame or credential contents."""
    stored = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
    last = stored[-1]["seq"] if stored else 0
    inspect = json.loads(stack.command(["docker", "inspect", stack.sandbox()]))[0]
    state = next(item["Source"] for item in inspect["Mounts"] if item["Destination"] == "/run/tqa/state")
    records = [json.loads(line)["frame"] for line in (Path(state) / "events.jsonl").read_text().splitlines()]
    pending = [frame for frame in records if frame["seq"] > last]
    summary = [{"seq": frame["seq"], "kind": frame.get("event", {}).get("type", frame["type"]),
                "contains_nul": contains_nul(frame)}
               for frame in pending[:8]]
    print("Confinement durable cursor: {}; pending WAL summaries: {}".format(last, json.dumps(summary)), flush=True)
    logs = stack.command(stack.configuration + ["logs", "--no-color", "--tail", "80", "postgres"])
    errors = [line for line in logs.splitlines() if "ERROR:" in line]
    print("Database error classes: " + "\n".join(errors[-4:])[-2000:], flush=True)


def contains_nul(value):
    if isinstance(value, str):
        return "\0" in value
    if isinstance(value, list):
        return any(contains_nul(item) for item in value)
    if isinstance(value, dict):
        return any(contains_nul(key) or contains_nul(item) for key, item in value.items())
    return False
