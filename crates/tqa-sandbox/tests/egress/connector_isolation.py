"""TOOL-I-018 (gateway-proxied MCP) and SEC-013 (connector credential isolation) on the real stack.

Built on WEB-E2E-010's connector fixture (ConnectorStack and mcp_fixture.py, unchanged): the remote MCP server over HTTPS,
the seeded `tracker` connector and a credential generated per run. A second connector,
`elsewhere`, is seeded but not enabled by default, so it is outside the conversation's set. Everything is driven over the
API; there is no browser.

TOOL-I-018:
- the runtime discovers the tool through the proxy (`mcp.server` ready with one tool);
- the call is journalled as `mcp.tracker.create_issue` and answered by the remote, with nothing asked first (ADR-0035);
- the remote receives the credential (injected by the gateway, from the gateway's address) and no Tqa-* header;
- a request from inside the sandbox for `elsewhere`, with the runtime token, gets 403.

SEC-013:
- while the run's first model reply is held (the scripted model's `wait_ms`, TK-U-035), a capture helper joins the
  sandbox's network namespace (the workstation image's own python3 with NET_RAW; no new image) and records every frame
  leaving or entering the sandbox, through the call.
  It cannot start earlier: the sandbox exists only once the message starts the run, and the runtime's initialize and
  tools/list follow within milliseconds. Those carry no credential either way, since the gateway adds it past the sandbox;
- each TCP flow's payload is reassembled in sequence order before the search, so a value split across segments cannot
  hide; a hole in any flow fails the check, and a synthetic split value is a positive control;
- the credential, in any of its encoded forms, is absent from the raw frames and every reassembled stream, from every
  sandbox process's environment, command line and memory-map listing, and from the sandbox's writable files (process
  memory itself is not read: that needs ptrace, which the sandbox refuses);
- positive controls by content: one reassembled stream carries the runtime token and the call to /mcp/tracker, the file
  search finds the runtime token in /run/tqa/token, and the process search finds TQA_GATEWAY_URL in a /proc environ.

With `--profile`, the task is made from a personal profile (the rows' "connector-enabled profile" and "a connector
outside the profile's allow-list"): `tracker` is off by default, so a task without the profile has no connector; the
person's profile allows `tracker` only, the task made from it carries exactly that set, and `elsewhere` is refused as
outside it. Everything else is checked as above.

Run from the repository root: PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 python3 -m egress.connector_isolation
[--profile]
"""
import json
import os
import signal
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.request
from pathlib import Path

from .stack import ROOT, compose
from .web_connector import ARGUMENTS, CONNECTOR, EMAIL, MODEL_TOOL, ConnectorStack, forms, found, settled

# The microVM half's own pieces (--backend firecracker): the hashed in-guest search and the scripted turns.
sys.path.insert(0, str(ROOT / "deploy/firecracker"))
import guest_search  # noqa: E402
import isolation_checks  # noqa: E402

OTHER = "elsewhere"
DEADLINE_SECONDS = 600
CAPTURE_SECONDS = 300
CAPTURE_READY_SECONDS = 30
# The first model reply is held this long (TK-U-035), so the capture joins the sandbox's network before the call. The
# capture is a `docker run`, and engine stalls of 30 s and more happen under load, so the hold is generous; a late capture
# still fails (the positive control), it never passes.
CAPTURE_HOLD_MS = 120000
# Records every frame on the sandbox's interfaces, each after its 4-byte length, until /capture/stop appears or the
# time runs out.
CAPTURE = (
    "import os,socket,struct,time\n"
    "s=socket.socket(socket.AF_PACKET,socket.SOCK_RAW,socket.ntohs(3))\n"
    "s.settimeout(0.5)\n"
    "out=open('/capture/packets.bin','wb')\n"
    "open('/capture/ready','w').close()\n"
    "end=time.time()+float(os.environ.get('CAPTURE_SECONDS','180'))\n"
    "while time.time()<end and not os.path.exists('/capture/stop'):\n"
    "    try:\n"
    "        f=s.recv(65535);out.write(struct.pack('>I',len(f))+f);out.flush()\n"
    "    except socket.timeout:\n"
    "        pass\n"
)
# Run inside the sandbox as the sandbox user: every process's environment, command line and memory-map listing, and every
# readable file under the writable roots, searched for labelled needles read from standard input (credential forms and
# positive controls). Prints only (place, label) hits, never a needle.
SANDBOX_SEARCH = (
    "import json,os,sys\n"
    "needles=[(label,value.encode()) for label,value in json.load(sys.stdin)]\n"
    "hits=[]\n"
    "def scan(name,data):\n"
    "    hits.extend([name,label] for label,value in needles if value in data)\n"
    "for pid in [p for p in os.listdir('/proc') if p.isdigit()]:\n"
    "    for part in ('environ','cmdline','maps'):\n"
    "        try:\n"
    "            scan('/proc/'+pid+'/'+part,open('/proc/'+pid+'/'+part,'rb').read())\n"
    "        except OSError:\n"
    "            pass\n"
    "for root in ('/workspace','/tmp','/run','/home','/var/tmp'):\n"
    "    for base,_,files in os.walk(root):\n"
    "        for name in files:\n"
    "            path=os.path.join(base,name)\n"
    "            try:\n"
    "                if os.path.isfile(path) and os.path.getsize(path)<=16*1024*1024:\n"
    "                    scan(path,open(path,'rb').read())\n"
    "            except OSError:\n"
    "                pass\n"
    "print(json.dumps(sorted(set(map(tuple,hits)))))\n"
)


def frames(capture):
    """The capture's frames: each is stored after its 4-byte big-endian length."""
    offset = 0
    while offset + 4 <= len(capture):
        (size,) = struct.unpack_from(">I", capture, offset)
        yield capture[offset + 4:offset + 4 + size]
        offset += 4 + size


def segment(frame):
    """(flow, sequence number, payload) for an Ethernet frame holding a TCP segment over IPv4 or IPv6, else None."""
    if len(frame) < 14:
        return None
    kind, ip = struct.unpack_from(">H", frame, 12)[0], frame[14:]
    if kind == 0x0800 and len(ip) >= 20 and ip[9] == 6:
        source, target, tcp = ip[12:16], ip[16:20], ip[(ip[0] & 0x0F) * 4:struct.unpack_from(">H", ip, 2)[0]]
    elif kind == 0x86DD and len(ip) >= 40 and ip[6] == 6:
        source, target, tcp = ip[8:24], ip[24:40], ip[40:40 + struct.unpack_from(">H", ip, 4)[0]]
    else:
        return None
    if len(tcp) < 20:
        return None
    source_port, target_port, sequence = struct.unpack_from(">HHI", tcp)
    return (source, source_port, target, target_port), sequence, tcp[(tcp[12] >> 4) * 4:]


def streams(capture):
    """Each TCP flow's payload reassembled in sequence order (retransmissions and loopback copies collapse), and the
    number of holes: gaps the capture missed, where a split value could hide."""
    flows = {}
    for frame in frames(capture):
        parsed = segment(frame)
        if parsed and parsed[2]:
            segments = flows.setdefault(parsed[0], {})
            if len(parsed[2]) > len(segments.get(parsed[1], b"")):
                segments[parsed[1]] = parsed[2]
    result, holes = {}, 0
    for flow, segments in flows.items():
        first = next(iter(segments))
        # Offsets from the first segment seen, signed, so a wrapped or earlier sequence number still orders correctly.
        ordered = sorted(((sequence - first + 2 ** 31) % 2 ** 32 - 2 ** 31, payload)
                         for sequence, payload in segments.items())
        data, end = bytearray(), ordered[0][0]
        for offset, payload in ordered:
            holes += offset > end
            if offset + len(payload) > end:
                data += payload[max(0, end - offset):]
                end = offset + len(payload)
        result[flow] = bytes(data)
    return result, holes


def synthetic(parts):
    """A capture of one IPv4 TCP flow whose segments are (sequence number, payload): the reassembly's positive control."""
    capture = b""
    for sequence, payload in parts:
        tcp = struct.pack(">HHIIBBHHH", 40000, 8080, sequence, 0, 5 << 4, 0x18, 65535, 0, 0) + payload
        ip = struct.pack(">BBHHHBBH4s4s", 0x45, 0, 20 + len(tcp), 0, 0, 64, 6, 0, bytes(4), bytes(4)) + tcp
        frame = bytes(12) + b"\x08\x00" + ip
        capture += struct.pack(">I", len(frame)) + frame
    return capture


class IsolationStack(ConnectorStack):
    def scenario(self):
        super().scenario()
        storage = Path(self.env["TQA_STORAGE_ROOT"])
        seed = ("\n[[connectors]]\nname = {name}\nurl = {url}\n"
                "enabled_by_default = false\nca_file = {ca}\n[connectors.auth]\nscheme = \"header\"\n"
                "header = \"X-Api-Key\"\nrequired = true\n").format(
            name=json.dumps(OTHER), url=json.dumps("https://{}:8443/mcp".format(self.upstream)),
            ca=json.dumps(str(storage / "fixture-ca.pem")))
        config = self.directory / "gateway.toml"
        compose.write(config, config.read_text() + seed)
        settled(config.read_text())
        # The call waits until the capture has joined the sandbox's network: the first reply is held.
        scenario = json.loads((self.directory / "scenario.json").read_text())
        scenario["responses"][0]["wait_ms"] = CAPTURE_HOLD_MS
        compose.write(self.directory / "scenario.json", json.dumps(scenario))

    def send(self, method, path, body):
        """A JSON request with the session cookie and the Origin header, for methods robustness.Stack lacks."""
        request = urllib.request.Request(self.origin + path, method=method, data=json.dumps(body).encode(),
                                         headers={"Origin": self.origin, "Content-Type": "application/json",
                                                  "Idempotency-Key": os.urandom(16).hex()})
        with self.client.open(request, timeout=15) as response:
            return response.status

    def start(self):
        """The administrator's task with the connector (enabled by default) and the person's credential, by API."""
        account = {"email": EMAIL, "password": self.env["TQA_STACK_PASSWORD"]}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        conversation = self.request("/api/v1/conversations", {"title": "Connector isolation", "model_route": "local"})
        assert conversation.get("connectors") == [CONNECTOR], "Only tracker is in the task's set: " + json.dumps(conversation)
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation
        status = self.send("PUT", "/api/v1/connectors/{}/credential".format(CONNECTOR), {"value": self.credential})
        assert status in (200, 201, 204), "The credential was not stored: {}".format(status)

    def capture(self, sandbox):
        """Starts the capture helper in the sandbox's network namespace; returns its directory and container."""
        directory = self.directory.parent / "capture"
        directory.mkdir(mode=0o700)
        container = self.command(["docker", "run", "-d", "--rm", "--network", "container:" + sandbox, "--user", "0",
                                  "--cap-drop", "ALL", "--cap-add", "NET_RAW", "--read-only",
                                  "-e", "CAPTURE_SECONDS={}".format(CAPTURE_SECONDS),
                                  "-v", "{}:/capture".format(directory), "--entrypoint", "python3", self.runtime,
                                  "-c", CAPTURE])
        deadline = time.monotonic() + CAPTURE_READY_SECONDS
        while not (directory / "ready").exists():
            assert time.monotonic() < deadline, "The capture helper did not start"
            time.sleep(0.2)
        return directory, container

    def run(self):
        """Starts the task, captures while its first reply is held, and returns the stored events and the captured
        bytes."""
        stream = self.request(self.path + "/events?stream=1&protocol=1", after=0)
        self.request(self.path + "/messages", {"text": "File the crash as an issue.", "attachments": []})
        stream.until("step.started")
        # No frame comes while the reply is held.
        stream.patience = CAPTURE_HOLD_MS // 1000 + 60
        started = time.monotonic()
        sandbox = self.sandbox()
        directory, container = self.capture(sandbox)
        assert time.monotonic() - started < CAPTURE_HOLD_MS / 1000 - 5, "The capture joined too late to see the call"
        try:
            stream.until("run.completed", within=CAPTURE_HOLD_MS // 1000 + 60)
            time.sleep(1)
        finally:
            (directory / "stop").touch()
            self.command(["docker", "stop", "--time", "5", container])
        return self.stored(), (directory / "packets.bin").read_bytes(), sandbox

    def outside_the_set(self, sandbox):
        """From inside the sandbox, with the runtime token: the connector outside the task's set gets 403."""
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                           "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                                      "clientInfo": {"name": "probe", "version": "1"}}})
        script = ('curl -s -o /tmp/answer -w "%{http_code}" -X POST -H "Authorization: Bearer $(cat /run/tqa/token)" '
                  '-H "Content-Type: application/json" -H "Accept: application/json, text/event-stream" '
                  "--data '" + body + "' \"${TQA_GATEWAY_URL%/}/mcp/" + OTHER + "\"; printf ' '; cat /tmp/answer")
        return self.command(["docker", "exec", sandbox, "sh", "-c", script])

    def search_sandbox(self, sandbox, needles):
        """The (label, value) needles go through standard input, so they never appear on a command line or in a log."""
        done = subprocess.run(["docker", "exec", "-i", sandbox, "python3", "-c", SANDBOX_SEARCH], env=self.env,
                              input=json.dumps(needles), capture_output=True, text=True, timeout=120, check=True)
        return [tuple(hit) for hit in json.loads(done.stdout)]

    def runtime_token(self, sandbox):
        return self.command(["docker", "exec", sandbox, "cat", "/run/tqa/token"])


# The person's profile in the --profile case: one built-in tool and the tracker connector, nothing else.
PROFILE = {"name": "tracker-only", "display_name": "Tracker only", "model_route": "local",
           "instructions": "File issues in the tracker.", "tools": ["read_file"], "skills": "none",
           "mcp_servers": [], "connectors": [CONNECTOR]}


class ProfileIsolationStack(IsolationStack):
    """The same fixture, with the task made from a personal profile that alone brings the connector."""

    def scenario(self):
        super().scenario()
        config = self.directory / "gateway.toml"
        text = config.read_text()
        assert text.count("enabled_by_default = true") == 1, "The seed must hold one connector enabled by default"
        compose.ownership(self.directory, self.env)
        compose.write(config, text.replace("enabled_by_default = true", "enabled_by_default = false"))
        settled(config.read_text())

    def start(self):
        account = {"email": EMAIL, "password": self.env["TQA_STACK_PASSWORD"]}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        control = self.request("/api/v1/conversations", {"title": "Without a profile", "model_route": "local"})
        assert control.get("connectors") == [] and not control.get("profile"), \
            "A task without the profile must have no connector: " + json.dumps(control)
        profile = self.request("/api/v1/profiles", PROFILE)
        assert profile.get("usable") is True, "The profile must be usable: " + json.dumps(profile)
        conversation = self.request("/api/v1/conversations", {"title": "Connector isolation", "profile": profile["id"]})
        copy = conversation.get("profile") or {}
        assert copy.get("name") == PROFILE["name"] and "connectors" in copy.get("sets", []), json.dumps(copy)
        assert conversation.get("connectors") == [CONNECTOR], "Only the profile brings tracker: " + json.dumps(conversation)
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation
        status = self.send("PUT", "/api/v1/connectors/{}/credential".format(CONNECTOR), {"value": self.credential})
        assert status in (200, 201, 204), "The credential was not stored: {}".format(status)


class MicrovmIsolationStack(IsolationStack):
    """SEC-013's microVM half: the same fixture and task, the sandbox a microVM. The guest has no `docker exec`, so a
    scripted shell turn after the call searches it (given hashes only) and asks for `elsewhere` from inside;
    the capture joins the VM host's network namespace, where the guest's tap is."""
    backend = "firecracker"

    def scenario(self):
        super().scenario()
        labelled = [("form:{}".format(index), value) for index, value in enumerate(forms(self.credential))]
        gateway_url = tomllib.loads((self.directory / "gateway.toml").read_text())["sandbox"]["gateway_url"]
        responses = isolation_checks.connector_scenario(MODEL_TOOL, ARGUMENTS, labelled, OTHER, gateway_url)
        # As on Docker, the call waits for the capture: the first reply is held.
        responses[0]["wait_ms"] = CAPTURE_HOLD_MS
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": responses}))


def check_microvm(stack):
    stored, packets, host = stack.run()
    events = [frame["event"] for frame in stored if frame.get("type") == "event"]
    servers = [event for event in events if event["type"] == "mcp.server"]
    assert servers == [{"type": "mcp.server", "name": CONNECTOR, "state": "ready", "tools": 1}], servers
    calls = [event for event in events if event["type"] == "tool.call"]
    assert [call["name"] for call in calls] == ["mcp.tracker.create_issue", "shell"], [call["name"] for call in calls]
    results = [event for event in events if event["type"] == "tool.result"]
    assert len(results) == 2 and results[0]["status"] == "ok" and "Created issue #7" in results[0]["content"], results
    record = stack.upstream_get("/record")
    remote_calls = record["calls"]
    assert len(remote_calls) == 1 and remote_calls[0]["arguments"] == ARGUMENTS, remote_calls
    assert remote_calls[0]["auth_valid"], "The gateway injected the person's credential"
    assert remote_calls[0]["peer"] == stack.gateway_address(), "Only the gateway reaches the remote"
    assert not any(item["tqa_headers"] for item in record["requests"]), "No Tqa-* header reaches the remote"
    status, answer = isolation_checks.outside_answer(results[1]["content"])
    assert status == "403", "A connector outside the task's set is refused: " + status + " " + answer[:300]
    assert "connector_not_allowed" in answer, "The refusal names its code: " + answer[:300]
    values = forms(stack.credential)
    flows, holes = streams(packets)
    assert holes == 0, "The capture missed {} stretches of TCP payload, so a split value could hide".format(holes)
    assert isolation_checks.saw_the_call(flows, CONNECTOR), \
        "One reassembled stream carries the call with its authorization, so the capture saw it"
    for data in [packets, *flows.values()]:
        assert found(values, data.decode("latin-1")) == [], "The credential left the guest"
    report = guest_search.report(results[1]["content"])
    problems = guest_search.problems(report)
    assert not problems, problems
    for place, text in (("the events", json.dumps(stored)), ("the logs", stack.logs())):
        assert found(values, text) == [], "The credential reached " + place
    print("SEC-013 on the microVM passed: the tool was called through the proxy with the credential injected by the "
          "gateway; {} was refused with 403 from inside the guest; the credential is absent from the guest's processes "
          "and {} files ({} bytes; the search found the runtime token and environment), from {} reassembled TCP "
          "streams captured at the VM host (one carrying the call), the events and the logs; VM host {}".format(
              OTHER, report["files"], report["bytes"], len(flows), host[:12]), flush=True)

def check(stack):
    stored, packets, sandbox = stack.run()
    events = [frame["event"] for frame in stored if frame.get("type") == "event"]
    servers = [event for event in events if event["type"] == "mcp.server"]
    assert servers == [{"type": "mcp.server", "name": CONNECTOR, "state": "ready", "tools": 1}], servers
    calls = [event for event in events if event["type"] == "tool.call"]
    assert [call["name"] for call in calls] == ["mcp.tracker.create_issue"], calls
    results = [event for event in events if event["type"] == "tool.result"]
    assert len(results) == 1 and results[0]["status"] == "ok" and "Created issue #7" in results[0]["content"], results
    record = stack.upstream_get("/record")
    remote_calls = record["calls"]
    assert len(remote_calls) == 1 and remote_calls[0]["arguments"] == ARGUMENTS, remote_calls
    assert remote_calls[0]["auth_valid"], "The gateway injected the person's credential"
    assert remote_calls[0]["peer"] == stack.gateway_address(), "Only the gateway reaches the remote"
    assert not any(item["tqa_headers"] for item in record["requests"]), "No Tqa-* header reaches the remote"
    status, _, answer = stack.outside_the_set(sandbox).partition(" ")
    assert status == "403", "A connector outside the task's set is refused: " + status + " " + answer[:300]
    assert "connector_not_allowed" in answer, "The refusal names its code: " + answer[:300]

    values = forms(stack.credential)
    # Positive controls: each search finds a planted copy, so an empty result means something.
    assert found(values, "x" + values[0] + "x") == [values[0]], "The form search misses a planted copy"
    marker = "split-marker-" + os.urandom(6).hex()
    split, _ = streams(synthetic([(1000 + 7, marker[7:].encode()), (1000, marker[:7].encode()),
                                  (1000, marker[:7].encode())]))
    assert found([marker], split[next(iter(split))].decode("latin-1")) == [marker], "Reassembly misses a split value"
    assert marker.encode() not in synthetic([(0, marker[:7].encode()), (7, marker[7:].encode())]), "The control splits it"
    token = stack.runtime_token(sandbox)
    flows, holes = streams(packets)
    assert holes == 0, "The capture missed {} stretches of TCP payload, so a split value could hide".format(holes)
    carried = [data for data in flows.values()
                if token.encode() in data and b"/mcp/" + CONNECTOR.encode() in data and b"tools/call" in data]
    assert carried, "One reassembled stream carries the runtime token and the call, so the capture saw it"
    for data in [packets, *flows.values()]:
        assert found(values, data.decode("latin-1")) == [], "The credential left the sandbox"
    labelled = [("form:{}".format(index), value) for index, value in enumerate(values)]
    hits = stack.search_sandbox(sandbox, labelled + [("control:token", token), ("control:gateway", "TQA_GATEWAY_URL=")])
    assert ("/run/tqa/token", "control:token") in hits, "The search did not read /run/tqa/token"
    assert any(place.startswith("/proc/") and place.endswith("/environ") and label == "control:gateway"
               for place, label in hits), "The search could not read the runtime's environment"
    leaks = [hit for hit in hits if hit[1].startswith("form:")]
    assert leaks == [], "The credential is inside the sandbox: " + json.dumps(leaks)
    for place, text in (("the events", json.dumps(stored)), ("the logs", stack.logs())):
        assert found(values, text) == [], "The credential reached " + place
    print("TOOL-I-018 and SEC-013 passed: the tool was discovered and called through the proxy with the credential "
          "injected by the gateway; {} was refused with 403; the credential is absent from the sandbox's processes "
          "and files (whose search found the runtime token and environment), from {} reassembled TCP streams of its "
          "captured traffic (one carrying the runtime token and the call), the events and the logs".format(
              OTHER, len(flows)), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("The connector isolation fixture was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-connector-isolation-")
    directory = compose.installation(temporary)
    env["TQA_STACK_PASSWORD"] = os.urandom(16).hex()
    profiled = "--profile" in sys.argv
    microvm = sys.argv[-2:] == ["--backend", "firecracker"]
    kind = MicrovmIsolationStack if microvm else ProfileIsolationStack if profiled else IsolationStack
    assert not (microvm and profiled), "--profile runs on Docker only"
    stack, failure = kind(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    # A microVM's first run may build the runtime's root filesystem before the stack starts.
    signal.alarm(DEADLINE_SECONDS * 2 if microvm else DEADLINE_SECONDS)
    try:
        stack.prepare("push")
        stack.start()
        (check_microvm if microvm else check)(stack)
        if profiled:
            print("(--profile) The task was made from the person's profile, which alone brought the connector; "
                  "elsewhere was refused as outside the profile's allow-list.", flush=True)
    except BaseException as error:
        failure = error
        print("Connector isolation contract failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in getattr(stack, "streams", []):
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
