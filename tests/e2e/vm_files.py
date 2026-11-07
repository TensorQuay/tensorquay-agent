"""E2E-VM-013 (ADR-0030): workspace files on the microVM backend, end to end over the public API. The gateway never
reads a microVM workspace on the host: every files operation below is answered by the runtime inside the guest, over
the link. An upload with no live sandbox starts the owner's VM; a message attaches it; a run writes files that are
listed, previewed and downloaded; a second run's changes are compared live and after the next snapshot; the workspace
is forked and read back in a new task. Counts and timings are printed per operation.
Run from the repository root on a prepared KVM host (deploy/firecracker/README.md), one stack at a time:
    TQA_DOCKER_TESTS=1 TQA_VMM_IMAGE=<tqa-vmm image> TQA_FC_KERNEL_DIR=<kernel dir> \\
        TQA_RUNTIME_IMAGE=<runtime> TQA_GATEWAY_IMAGE=<gateway> TQA_FAKE_MODEL_IMAGE=<fake model> python3 tests/e2e/vm_files.py
"""
from pathlib import Path
import json
import sys
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "firecracker"))
import stack  # noqa: E402

USAGE = {"prompt_tokens": 10, "completion_tokens": 10}


def shell(command):
    return {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
            "usage": USAGE}


def text(words):
    return {"events": [{"type": "text", "text": words}], "usage": USAGE}


SCENARIO = {"model": "fixture", "responses": [
    shell("printf 'one\\n' > notes.txt && mkdir -p src && printf 'fn main() {}\\n' > src/main.rs && wc -c uploads/data.csv"),
    text("Wrote the notes."),
    shell("printf 'two\\n' >> notes.txt && printf '# Report\\n' > report.md"),
    text("Added the report."),
]}

TIMINGS = []


def timed(operation, action):
    started = time.monotonic()
    result = action()
    TIMINGS.append((operation, time.monotonic() - started))
    return result


def raw(demo, method, path, data=None, content_type=None):
    """A request whose body or answer is not JSON: the status and the bytes."""
    headers = {"Origin": demo.origin}
    if content_type:
        headers["Content-Type"] = content_type
    request = urllib.request.Request(demo.origin + path, headers=headers, data=data, method=method)
    with demo.client.open(request, timeout=300) as response:
        return response.status, response.read(16 << 20)


def runs(demo, conversation):
    """Run ids in the order their runs started."""
    return [frame["event"]["run_id"] for frame in stack.events(demo, conversation)
            if frame.get("event", {}).get("type") == "run.started"]


def message(demo, conversation, words, attachments=()):
    before = len(runs(demo, conversation))
    demo.request("/api/v1/conversations/{}/messages".format(conversation),
                 {"text": words, "attachments": list(attachments)})
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        stored = stack.events(demo, conversation)
        ended = [frame for frame in stored if frame.get("event", {}).get("type") in ("run.completed", "run.failed")]
        if len(ended) > before:
            assert ended[-1]["event"]["type"] == "run.completed", ended[-1]
            return runs(demo, conversation)[-1], stored
        time.sleep(0.5)
    raise AssertionError("the run did not end")


def files(demo, conversation, path=""):
    query = "?" + urllib.parse.urlencode({"path": path}) if path else ""
    return demo.request("/api/v1/conversations/{}/files{}".format(conversation, query))


def check(demo):
    demo.sign_in()
    ready = demo.request("/readyz?detail=1")
    assert ready["sandbox"]["backend"] == "firecracker", ready
    conversation = demo.request("/api/v1/conversations", {"title": "Files", "model_route": "local"})["id"]
    base = "/api/v1/conversations/{}".format(conversation)
    # An upload with no live sandbox: the owner's VM starts, and the runtime stores it under a free name.
    status, body = timed("upload (VM start)", lambda: raw(
        demo, "POST", base + "/uploads?name=data.csv", b"a,b\n1,2\n", "application/octet-stream"))
    uploaded = json.loads(body)
    assert status == 201 and uploaded == {"path": "uploads/data.csv", "name": "data.csv", "size": 8}, uploaded
    assert len(demo.vm_hosts()) == 1, "the upload started the owner's VM"
    # A message attaching it; the run writes files and reads the upload in the guest.
    first, stored = timed("message + run 1", lambda: message(demo, conversation, "Write the notes", ["uploads/data.csv"]))
    results = stack.tool_results(stored)
    assert results[-1]["status"] == "ok" and "8 uploads/data.csv" in results[-1]["content"], results[-1]
    listed = timed("list", lambda: files(demo, conversation))
    names = [(item["name"], item["kind"]) for item in listed["items"]]
    assert names == [("src", "dir"), ("uploads", "dir"), ("notes.txt", "file")], names
    nested = timed("list src", lambda: files(demo, conversation, "src"))
    assert [item["name"] for item in nested["items"]] == ["main.rs"], nested
    preview = timed("preview", lambda: demo.request(base + "/files/content?path=notes.txt"))
    assert (preview["text"], preview["size"], preview["binary"]) == ("one\n", 4, False), preview
    status, downloaded = timed("download", lambda: raw(demo, "GET", base + "/files/download?path=src/main.rs"))
    assert (status, downloaded) == (200, b"fn main() {}\n"), downloaded
    # A second run: its changes are compared with the live workspace, then with the next snapshot.
    second, _ = timed("message + run 2", lambda: message(demo, conversation, "Add the report"))
    live = timed("changes (live)", lambda: demo.request(base + "/runs/{}/changes".format(second)))
    expected = [
        {"path": "notes.txt", "status": "modified", "added": 1, "removed": 0, "binary": False, "too_large": False},
        {"path": "report.md", "status": "added", "added": 1, "removed": 0, "binary": False, "too_large": False},
    ]
    assert (live["files"], live["live"]) == (expected, True), live
    diff = timed("diff", lambda: demo.request(
        base + "/runs/{}/changes/diff?path=notes.txt".format(second)))
    assert diff["unified"] == "--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1,2 @@\n one\n+two\n", diff
    first_changes = timed("changes (closed)", lambda: demo.request(base + "/runs/{}/changes".format(first)))
    assert [change["path"] for change in first_changes["files"]] == ["notes.txt", "src/main.rs"], first_changes
    # The workspace forked while its VM runs, then read back in a new task on the copy.
    workspace = demo.request(base)["workspace_id"]
    fork = timed("fork", lambda: demo.request("/api/v1/workspaces/{}/fork".format(workspace), {"name": "Copy"}))
    deadline = time.monotonic() + 300
    while demo.request("/api/v1/workspaces/{}".format(fork["id"]))["state"] == "forking":
        assert time.monotonic() < deadline, "the fork did not settle"
        time.sleep(0.5)
    copy = demo.request("/api/v1/conversations",
                        {"title": "Copy", "model_route": "local", "workspace_id": fork["id"]})["id"]
    copied = timed("list (fork, VM start)", lambda: files(demo, copy))
    assert [item["name"] for item in copied["items"]] == ["src", "uploads", "notes.txt", "report.md"], copied
    print("E2E-VM-013: files, uploads, attachments, run changes and fork answered from the runtime in the guest",
          flush=True)
    for operation, seconds in TIMINGS:
        print("  {:<24} {:7.0f} ms".format(operation, seconds * 1000), flush=True)


if __name__ == "__main__":
    stack.run("E2E-VM-013", SCENARIO, check)
