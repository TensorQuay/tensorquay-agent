"""E2E-VM-012 and SBX-F-006: the Gate 8 demo as an automated scenario over the public API (DEV-PLAN Phase 8).
A task runs in a microVM: one tool call writes a marker and completes, then the VM is killed during the next call (the
Firecracker process gets SIGKILL). The run is interrupted, resumes in a new VM on the same workspace image, and
completes: the model reads back the marker, because a journaled tool result never runs ahead of its file changes
(ARCHITECTURE §4.5, `sync_before_result`), and the transcript is one contiguous event stream, as the UI shows it.
Run from the repository root on a prepared KVM host (deploy/firecracker/README.md), one stack at a time:
    TQA_DOCKER_TESTS=1 TQA_VMM_IMAGE=<tqa-vmm image> TQA_FC_KERNEL_DIR=<kernel dir> python3 tests/e2e/gate8.py
"""
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "firecracker"))
import stack  # noqa: E402

# How long the second tool call runs: long enough to kill its VM in the middle of it.
TOOL_SECONDS = 60
# Detection (the link drops and the dead-agent rule), the new VM's boot and the resumed steps.
RESUME_SECONDS = 300

SCENARIO = {"model": "fixture", "responses": [
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": "printf kept > marker"}}],
     "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {
        "command": "echo started && sleep {} && echo finished".format(TOOL_SECONDS)}}],
     "usage": {"prompt_tokens": 20, "completion_tokens": 10}},
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": "cat marker"}}],
     "usage": {"prompt_tokens": 30, "completion_tokens": 10}},
    {"events": [{"type": "text", "text": "Recovered in a new microVM."}],
     "usage": {"prompt_tokens": 40, "completion_tokens": 10}}]}


def second_call_running(demo, conversation, seconds=180):
    """Waits until the first call has its result and the second has started."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        order = kinds(stack.events(demo, conversation))
        if order.count("tool.start") >= 2 and order.count("tool.result") >= 1:
            return
        time.sleep(0.5)
    raise AssertionError("the second tool call never started")


def kinds(stored):
    return [frame["event"]["type"] for frame in stored]


def one_vm_host(demo):
    hosts = demo.vm_hosts()
    assert len(hosts) == 1, "exactly one VM host: {}".format(hosts)
    return hosts[0]


def crash(demo):
    """E2E-VM-012: kill the VM mid-tool; the run resumes in a new VM on the same image and completes."""
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Gate 8", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Work, then survive", "attachments": []})
    second_call_running(demo, conversation)
    first = one_vm_host(demo)
    subprocess.run(["docker", "kill", "--signal", "KILL", first], check=True, capture_output=True, timeout=30)
    killed_at = time.monotonic()
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), RESUME_SECONDS)
    recovered = time.monotonic() - killed_at
    order = kinds(stored)
    assert order[-1] == "run.completed", order
    assert "run.resumed" in order, order
    results = stack.tool_results(stored)
    assert [result["status"] for result in results] == ["ok", "interrupted", "ok"], results
    assert results[2]["content"].strip() == "kept", \
        "a completed call's file survived the crash in the same workspace image: {}".format(results)
    assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1)), "one contiguous stream"
    second = one_vm_host(demo)
    assert second != first, "a new VM host replaced the killed one"
    print("E2E-VM-012 / SBX-F-006: the killed VM's run was interrupted, resumed in a new VM on the same image and "
          "completed; the completed call's file survived; recovery took {:.0f} s".format(recovered), flush=True)
    print("Events: " + ", ".join(order), flush=True)


if __name__ == "__main__":
    stack.run("E2E-VM-012", SCENARIO, crash)
