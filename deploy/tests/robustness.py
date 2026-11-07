"""GW-I-018, GW-E-020, SBX-D-003, GW-I-036, GW-I-066, GW-I-314 and E2E-IDLE-019 against an owned real-runtime stack."""
import argparse
import http.cookiejar
import json
import os
import re
import secrets
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

import compose
import demo
import halfopen_case
import idle_case
import lifetime_case
from soak_chaos import DATABASE_QUERY, database_sample, observer_arguments


# The gateway's SSE keep-alive default (`stream_keepalive`); a quiet stream carries one record per interval.
DEFAULT_KEEPALIVE = 15
# The stream socket's read timeout must span several keep-alives, or a slightly late keep-alive during a quiet
# minute (a long tool call or a store outage) looks like a dead socket. `Stream.patience` bounds progress instead.
STREAM_READ_TIMEOUT = 45


def keepalive(config):
    """The `stream_keepalive` a gateway.toml sets at top level, else the default."""
    top = ("\n" + config).split("\n[", 1)[0]
    match = re.search(r"^stream_keepalive\s*=\s*(\d+)\s*$", top, re.MULTILINE)
    return int(match.group(1)) if match else DEFAULT_KEEPALIVE


def check_stream_timeout(config):
    interval = keepalive(config)
    assert STREAM_READ_TIMEOUT >= 3 * interval, (
        "The stream read timeout must exceed three keep-alive intervals: %s s vs %s s" % (STREAM_READ_TIMEOUT, interval))


def engine_snapshot(reason):
    """The engine's stacks and the host's pressure (engine_dump), never failing the caller: a diagnostic only."""
    try:
        import engine_dump
        return engine_dump.snapshot(reason)
    except Exception as error:
        return "Engine snapshot unavailable: {}".format(error)


# The stack runs the gateway's default io_timeout. A runtime frame the store can't take is retried for three of them
# before the link closes with store_unavailable (GW-U-075), so the outage's wait for that notice covers the retry
# budget plus Postgres's stop (up to 10 s) and the release of the shell command.
GATEWAY_IO_TIMEOUT = 10
STORE_OUTAGE_NOTICE = 3 * GATEWAY_IO_TIMEOUT + 30


class Stream:
    def __init__(self, response, after=0):
        self.response, self.last = response, after
        self.events = []
        # Every frame's kind in arrival order, control frames included (bounded; for failure diagnosis only).
        self.trail = []
        # Seconds without any protocol frame before the stream counts as stalled; parking waits longer.
        self.patience = 30

    def next(self):
        """Each socket read is bounded; keep-alives do not consume sequence numbers."""
        frame = None
        deadline = time.monotonic() + self.patience
        for _ in range(4096):
            if time.monotonic() >= deadline:
                # The trail and the engine's own view name where the stream stopped (the gate evaluations ask for it).
                print("Stream trail before the stall: " + json.dumps(self.trail), flush=True)
                print(engine_snapshot("no protocol frame for {} s".format(self.patience)), flush=True)
                raise AssertionError("No protocol frame arrived before the progress deadline")
            line = self.response.readline(262145)
            if not line:
                raise AssertionError("Stream ended before its expected frame")
            if len(line) > 262144:
                raise AssertionError("SSE record exceeded the protocol bound")
            if line.startswith(b"data:"):
                frame = json.loads(line[5:])
            if line in (b"\n", b"\r\n") and frame is not None:
                kind = frame.get("event", {}).get("type", frame.get("type"))
                state = (frame.get("sandbox") or {}).get("state")
                self.trail = (self.trail + [(frame.get("seq"), kind if state is None else kind + ":" + state)])[-24:]
                if "seq" in frame and frame["seq"] is not None:
                    assert frame["seq"] == self.last + 1, "Noncontiguous or duplicate sequence"
                    self.last = frame["seq"]
                    self.events.append(frame)
                return frame
        raise AssertionError("Too many stream records without a frame")

    def deadline(self, within):
        """The end of a wait for one frame: `within` seconds, or by default 90 s or the stream's patience if longer, so
        a run held on purpose (a held scripted reply) is not given up on at the first frame after 90 s."""
        return time.monotonic() + (max(90, self.patience) if within is None else within)

    def until(self, kind, within=None):
        deadline = self.deadline(within)
        while time.monotonic() < deadline:
            frame = self.next()
            event = frame.get("event", {})
            if event.get("type") == kind:
                return frame
            assert event.get("type") != "run.failed", "The real run failed: " + json.dumps(event)[:512]
        raise AssertionError("Expected lifecycle event did not arrive: " + kind)

    def ended(self, within=None):
        """The run's end, completed or failed: which one depends on timing the test does not control."""
        deadline = self.deadline(within)
        while time.monotonic() < deadline:
            frame = self.next()
            if frame.get("event", {}).get("type") in ("run.completed", "run.failed"):
                return frame
        raise AssertionError("The run did not end")

    def frame(self, kind, within=None):
        deadline = self.deadline(within)
        while time.monotonic() < deadline:
            frame = self.next()
            if frame["type"] == kind:
                return frame
            assert frame.get("event", {}).get("type") != "run.failed", "The real run failed: " + json.dumps(frame)[:512]
        raise AssertionError("Expected frame did not arrive: " + kind)

    def notice(self, code, within=30):
        """Waits up to `within` seconds for the control notice `code`; a quiet stream may stay quiet that long too."""
        deadline = time.monotonic() + within
        patience, self.patience = self.patience, max(self.patience, within)
        try:
            while time.monotonic() < deadline:
                frame = self.next()
                if frame["type"] == "error":
                    assert frame["code"] == code and frame.get("seq") is None
                    return
        finally:
            self.patience = patience
        raise AssertionError("Expected control notice did not arrive")


class Stack:
    def __init__(self, directory, env):
        self.directory, self.env = directory, env
        self.client = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.configuration = demo.COMPOSE + ["-f", str(directory.parent / "robustness.json")]
        self.streams = []

    def command(self, args):
        return compose.command(args, self.env, capture=True).stdout.strip()

    def prepare(self, case):
        # A tag can point at a stale build; the log records exactly which gateway image ran.
        image = compose.required_image(self.env, "TQA_GATEWAY_IMAGE")
        print("Gateway image: " + image + " " + self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image]),
              flush=True)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=demo.runtime_image(self.env), port=port)
        # prepare() gave the installation to the gateway user; take it back before writing the scenario.
        compose.ownership(self.directory, self.env)
        self.origin = "http://127.0.0.1:" + str(port)
        command = "printf 'kept' > marker; while [ ! -f release ]; do printf 'tick\\n'; sleep 1; done; cat marker"
        scenario = CASES[case][0] if case in CASES else {"model": "fixture", "responses": [
            {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
             "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
            {"events": [{"type": "text", "text": "The run continued."}],
             "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}
        compose.write(self.directory / "scenario.json", json.dumps(scenario))
        compose.write(self.directory.parent / "robustness.json", json.dumps({"services": {
            "fake-model": {"command": ["--scenario", "/scenarios/robustness.json"],
                           "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                                        "target": "/scenarios/robustness.json", "read_only": True}]}}}))
        check_stream_timeout((self.directory / "gateway.toml").read_text())
        if case in CASES:
            config = self.directory / "gateway.toml"
            compose.write(config, config.read_text().replace("[sandbox]\n", "[sandbox]\n" + CASES[case][1], 1))
        compose.ownership(self.directory, self.env, "1000:1000")
        self.up()

    def up(self):
        self.command(self.configuration + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "60"])

    def request(self, path, body=None, after=None, method=None):
        headers = {"Origin": self.origin, "Content-Type": "application/json"}
        if body is not None:
            headers["Idempotency-Key"] = secrets.token_hex(16)
        if after is not None:
            headers["Last-Event-ID"] = str(after)
        request = urllib.request.Request(self.origin + path, headers=headers, method=method,
                                         data=None if body is None else json.dumps(body).encode())
        response = self.client.open(request, timeout=15 if after is None else STREAM_READ_TIMEOUT)
        if after is not None:
            stream = Stream(response, after)
            self.streams.append(stream)
            return stream
        with response:
            data = response.read(1048577)
            assert len(data) <= 1048576, "Response exceeded fixture cap"
            return json.loads(data) if data else None

    def sign_in(self):
        password = "synthetic-robustness-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "robustness@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)

    def start_run(self, until="tool.start"):
        self.sign_in()
        conversation = self.request("/api/v1/conversations", {"title": "Recovery", "model_route": "local"})
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation
        stream = self.request(self.path + "/events?stream=1&protocol=1", after=0)
        self.request(self.path + "/messages", {"text": "Exercise recovery", "attachments": []})
        if until:
            stream.until(until)
        return stream

    def sandbox(self):
        containers = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"]]).split()
        assert len(containers) == 1, "Expected exactly one running sandbox"
        return containers[0]

    def reconnect(self, after):
        return self.request(self.path + "/events?stream=1&protocol=1", after=after)

    def release(self, sandbox):
        self.command(["docker", "exec", sandbox, "touch", "/workspace/release"])

    def verify(self, stream, last="run.completed"):
        stream.until(last)
        assert self.command(["docker", "exec", self.sandbox(), "cat", "/workspace/marker"]) == "kept"
        stored = self.request(self.path + "/events?after=0&limit=1000")["items"]
        assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1))
        assert stored[-1]["event"]["type"] == last
        return stored


def shutdown(stack):
    stream = stack.start_run()
    sandbox = stack.sandbox()
    stack.command(stack.configuration + ["stop", "--timeout", "10", "gateway"])
    stream.notice("restarting")
    assert stream.response.read(1) == b"", "Shutdown notice must terminate the stream"
    gateway = stack.command(stack.configuration + ["ps", "-aq", "gateway"])
    assert stack.command(["docker", "inspect", "--format", "{{.State.ExitCode}}", gateway]) == "0"
    assert stack.sandbox() == sandbox, "Gateway shutdown destroyed the runtime"
    stack.up()
    replay = stack.reconnect(stream.last)
    stack.release(sandbox)
    stack.verify(replay)
    assert stack.sandbox() == sandbox, "Gateway restart replaced a healthy runtime"
    print("GW-I-018: SIGTERM ended the stream with restarting; the same runtime redialled and completed", flush=True)


def kinds(frames):
    """Each frame's sequence and kind, never its content."""
    return [(frame.get("seq"), frame.get("event", {}).get("type", frame.get("type"))) for frame in frames]


def diagnose(stack, stream):
    """What the client stream received against what the gateway stored: frames stored past the stream's last one mean
    they were not delivered; none mean the runtime's replay never reached the store (GW-E-020 root cause)."""
    print("stream last seq {}; its last frames {}".format(stream.last, kinds(stream.events[-8:])), flush=True)
    print("stream trail, control frames included: {}".format(stream.trail), flush=True)
    try:
        stored = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
        print("stored {} frames; the last ones {}".format(len(stored), kinds(stored[-8:])), flush=True)
    except Exception as error:  # the diagnosis must never hide the failure
        print("stored frames could not be read: " + type(error).__name__, flush=True)


def outage(stack):
    stream = stack.start_run()
    sandbox = stack.sandbox()
    stack.command(stack.configuration + ["stop", "--timeout", "10", "postgres"])
    # Shell output is emitted when the command ends, so the run produces frames only after its release.
    stack.release(sandbox)
    stream.notice("store_unavailable", within=STORE_OUTAGE_NOTICE)
    before = stream.last
    stack.command(stack.configuration + ["up", "-d", "--no-build", "--wait", "postgres"])
    # The relay answers 503 with Retry-After while the store is down. Whether the run then completes (the store is back
    # within the runtime's model retry budget) or fails with a retryable provider error (it is not) depends on timing
    # the test does not control; either way the stream and the sandbox survive and the replay stays contiguous.
    try:
        end = stream.ended()
    except AssertionError:
        diagnose(stack, stream)
        raise
    if end["event"]["type"] == "run.failed":
        assert end["event"]["code"] == "provider_error", "An outage may fail the run only as a retryable provider error"
    assert stack.command(["docker", "exec", sandbox, "cat", "/workspace/marker"]) == "kept"
    stored = stack.request(stack.path + "/events?after=0&limit=1000")["items"]
    assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1)), "The replayed transcript has a gap"
    assert stored[-1]["seq"] == end["seq"], "The stream and the store disagree on the run's end"
    assert stream.last > before and stack.sandbox() == sandbox
    assert any(frame["event"]["type"] == "tool.result" for frame in stored)
    # After recovery the user can send again on the same sandbox.
    stack.request(stack.path + "/messages", {"text": "Continue after recovery", "attachments": []})
    stored = stack.verify(stream)
    assert stack.sandbox() == sandbox
    print("GW-E-020: real database outage kept the client stream and sandbox; WAL replay was contiguous; the run "
          "{} and the next message completed".format(
              "completed after its retries" if end["event"]["type"] == "run.completed" else "failed retryably"),
          flush=True)


def daemon(stack):
    stream = stack.start_run()
    original = stack.sandbox()
    # This mode is separately opted in only after coordinating every user of the daemon.
    stack.command(["docker", "desktop", "restart", "--timeout", "80"])
    stack.up()
    replay = stack.reconnect(stream.last)
    replay.until("run.resumed")
    assert stack.sandbox() != original, "A stopped sandbox was reused"
    stack.release(stack.sandbox())
    stored = stack.verify(replay)
    assert any(frame["event"]["type"] == "run.resumed" for frame in stored)
    print("SBX-D-003: daemon restart replaced the interrupted runtime and retained its workspace", flush=True)


def lifetime(stack):
    """GW-I-066: the gateway retires a busy container at its max lifetime plus the grace; the run resumes in a new one.
    Polls the stored events, not a stream: the run is quiet for 20 s at a time and lasts minutes."""
    stack.sign_in()
    stack.conversation = stack.request("/api/v1/conversations", {"title": "Lifetime", "model_route": "local"})["id"]
    stack.path = "/api/v1/conversations/" + stack.conversation
    stack.request(stack.path + "/messages", {"text": "Work", "attachments": []})
    stored, interrupted = lifetime_case.watch(lambda: stack.request(stack.path + "/events?after=0&limit=1000")["items"])
    log = stack.command(stack.configuration + ["logs", "--no-color", "gateway"])
    print(lifetime_case.check(stored, interrupted, log) + " (Docker)", flush=True)


def task(stack, title, words):
    """Creates a task, sends it a message and returns its path."""
    path = "/api/v1/conversations/" + stack.request("/api/v1/conversations", {"title": title, "model_route": "local"})["id"]
    stack.request(path + "/messages", {"text": words, "attachments": []})
    return path


def stored_until(stack, path, kind, count=1, seconds=120):
    """The task's stored events once `kind` is among them `count` times."""
    deadline = time.monotonic() + seconds
    while True:
        stored = stack.request(path + "/events?after=0&limit=1000")["items"]
        if idle_case.kinds(stored).count(kind) >= count:
            return stored
        assert time.monotonic() < deadline, "no {} x{} within {} s: {}".format(kind, count, seconds, idle_case.kinds(stored))
        time.sleep(0.5)


def container(stack, path):
    """The task's sandbox container, or None once it is gone (removed, not only stopped)."""
    found = stack.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + path.rsplit("/", 1)[1]]).split()
    assert len(found) <= 1, "one sandbox per task: {}".format(found)
    return found[0] if found else None


def retired(stack, path, what):
    """Waits from the run's end (seen now) until the task's sandbox is gone; checks it went after the window."""
    ended = time.monotonic()
    assert container(stack, path) is not None, "{}: the sandbox went with the run".format(what)
    seconds = idle_case.until_gone(lambda: container(stack, path) is not None, ended,
                                   idle_case.IDLE_SECONDS + idle_case.LATE_SECONDS)
    print("E2E-IDLE-019: " + idle_case.check_retired(seconds, what), flush=True)


def idle(stack):
    """E2E-IDLE-019: a finished or stopped run's sandbox goes after the idle window; the next message wakes the task on
    the same files; a deleted task's sandbox goes at once."""
    stack.sign_in()
    first_task = task(stack, "Idle", idle_case.FINISHED)
    stored_until(stack, first_task, "run.completed")
    first = container(stack, first_task)
    retired(stack, first_task, "a finished task")
    listed = stack.request(first_task + "/files?path=")["items"]
    assert "marker" in [item["name"] for item in listed], "the workspace stays while no sandbox runs: {}".format(listed)
    stack.request(first_task + "/messages", {"text": "Read the marker", "attachments": []})
    stored = stored_until(stack, first_task, "run.completed", count=2)
    print("E2E-IDLE-019: " + idle_case.check_woken(stored, first, container(stack, first_task)), flush=True)
    stopped_task = task(stack, "Stopped", idle_case.STOPPED)
    started = stored_until(stack, stopped_task, "tool.start")
    run = next(frame["event"]["run_id"] for frame in started if frame["event"]["type"] == "run.started")
    stack.request(stopped_task + "/runs/{}/cancel".format(run), {})
    stopped = idle_case.results(stored_until(stack, stopped_task, "run.cancelled"))
    assert [call["status"] for call in stopped] != ["ok"], "the stop came during the call: {}".format(stopped)
    retired(stack, stopped_task, "a stopped run")
    deleted_task = task(stack, "Deleted", idle_case.DELETED)
    stored_until(stack, deleted_task, "run.completed")
    assert container(stack, deleted_task) is not None, "the finished task's sandbox idles"
    since = time.monotonic()
    stack.request(deleted_task, method="DELETE")
    seconds = idle_case.until_gone(lambda: container(stack, deleted_task) is not None, since, idle_case.DELETE_SECONDS)
    print("E2E-IDLE-019: " + idle_case.check_deleted(seconds), flush=True)


def gateway_sample(stack):
    """The gateway's descriptors and TCP tables (the soak's observer, in its PID namespace) and its database
    connections, as one GW-I-314 sample."""
    gateway = stack.command(["docker", "inspect", "--format", "{{.Id}}",
                             stack.command(stack.configuration + ["ps", "-q", "gateway"])])
    observed = stack.command(observer_arguments(demo.runtime_image(stack.env), gateway)
                             + ["-c", halfopen_case.OBSERVER])
    database = database_sample(stack.command(stack.configuration + [
        "exec", "-T", "postgres", "psql", "-U", "tqa", "-d", "gateway", "-At", "-c", DATABASE_QUERY]))
    return halfopen_case.sample(observed, database["gateway_connections"])


class SandboxExits:
    """Read only: every start and die of the task's sandboxes from Docker's event stream (replayed from `since`), and
    each one's log followed from its start, so a sandbox the gateway removes at once (a "lost" one) still shows its exit
    code and last lines."""
    TAIL_LINES = 5

    def __init__(self, stack, path, since):
        self.env, self.lines, self.logs = stack.env, [], {}
        self.events = subprocess.Popen(
            ["docker", "events", "--since", "{:.3f}".format(since), "--format", "{{json .}}",
             "--filter", "type=container", "--filter", "event=start", "--filter", "event=die",
             "--filter", "label=tqa.conversation=" + path.rsplit("/", 1)[1]],
            env=self.env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        for line in self.events.stdout:
            self.lines.append(line)
            try:
                item = json.loads(line)
            except ValueError:
                continue
            ident = halfopen_case.container_id(item)
            if item.get("Action") == "start" and ident and ident not in self.logs:
                log = tempfile.TemporaryFile("w+")
                self.logs[ident] = (log, subprocess.Popen(
                    ["docker", "logs", "--follow", "--timestamps", ident], env=self.env, stdout=log,
                    stderr=subprocess.STDOUT, text=True))

    def close(self):
        """Stops following and returns one line per sandbox (halfopen_case.exits)."""
        time.sleep(2)  # the last die event's line
        self.events.terminate()
        self.reader.join(5)
        tails = {}
        for ident, (log, follower) in list(self.logs.items()):
            follower.terminate()
            follower.wait(5)
            log.seek(0)
            lines = log.read().splitlines()[-self.TAIL_LINES:]
            tails[ident] = [compose.redact(line.rstrip(), self.env)[:300] for line in lines]
            log.close()
        return halfopen_case.exits(self.lines, tails)


def halfopen(stack):
    """GW-I-314: an idle keep-alive connection from a sandbox to the gateway, then the soak's link_cut (the sandbox's
    networks disconnected) and a kill, so the gateway never hears its end; the gateway frees it within the bound. The
    task is deleted right after the kill: the gateway replaces a live task's lost sandbox, and a replacement's sockets
    couldn't be told apart from the held one's, while a deleted task's sandbox is retired, never replaced. The held
    connection is an HTTP keep-alive one, not the task's, so the delete leaves it as the cut left it."""
    stack.sign_in()
    since = time.time()
    path = task(stack, "Half-open", "Get ready")
    exits = SandboxExits(stack, path, since)
    try:
        halfopen_watch(stack, path)
    finally:
        try:
            lines = exits.close()
        except Exception as error:  # evidence only: never over the case's own outcome
            lines = ["could not be read: {!r}".format(error)]
        for line in lines:
            print("GW-I-314 sandbox exits: " + line, flush=True)


def halfopen_watch(stack, path):
    """The case itself, from the task's first run to the check."""
    stored_until(stack, path, "run.completed")
    sandbox = container(stack, path)
    record = json.loads(stack.command(["docker", "inspect", "--format", "{{json .NetworkSettings.Networks}}", sandbox]))
    addresses = {settings["IPAddress"] for settings in record.values() if settings.get("IPAddress")}
    stack.command(["docker", "exec", "-d", sandbox, "python3", "-c", halfopen_case.CLIENT])
    # Read quietly until the client has its answer and wrote its port (a failing `cat` would log each try).
    deadline, written = time.monotonic() + 30, ""
    while not written:
        assert time.monotonic() < deadline, "the sandbox's keep-alive client never got its answer"
        time.sleep(1)
        written = stack.command(["docker", "exec", sandbox, "sh", "-c",
                                 "cat {} 2>/dev/null || true".format(halfopen_case.PORT_FILE)])
    assert len(addresses) == 1, "one sandbox network: {}".format(len(addresses))
    peer = (next(iter(addresses)), int(written))
    baseline = halfopen_case.check_before(gateway_sample(stack), peer, addresses)
    # The soak's link_cut (soak_stack.cut_link), then its sandbox_kill: no FIN or RST can reach the gateway.
    for name in record:
        stack.command(["docker", "network", "disconnect", name, sandbox])
    stack.command(["docker", "kill", sandbox])
    stack.request(path, method="DELETE")
    connection, everything, snapshot = halfopen_case.watch(lambda: gateway_sample(stack), peer, addresses, baseline,
                                                           time.monotonic, time.sleep)
    # The figures first, so a run stopped by a later check still shows what the watch saw.
    print(halfopen_case.report(connection, everything, snapshot, peer, addresses, baseline), flush=True)
    assert container(stack, path) in (None, sandbox), "a replacement sandbox started; its sockets can't be told apart"
    print(halfopen_case.check(connection, everything, snapshot, peer, addresses, baseline) + " (Docker)", flush=True)


def halfopen_reconnect(stack):
    """GW-I-318: as GW-I-314, an idle keep-alive connection from a sandbox to the gateway and the soak's link_cut;
    but the link comes back after 30 s with the same address, and the sandbox lives on. The gateway keeps the
    connection: it is established to 150 s after the cut (past keepalive's probes, which come after the reconnect and
    are answered), and it answers a second request on the same socket."""
    stack.sign_in()
    since = time.time()
    path = task(stack, "Half-open reconnect", "Get ready")
    exits = SandboxExits(stack, path, since)
    try:
        halfopen_reconnect_watch(stack, path)
    finally:
        try:
            lines = exits.close()
        except Exception as error:  # evidence only: never over the case's own outcome
            lines = ["could not be read: {!r}".format(error)]
        for line in lines:
            print("GW-I-318 sandbox exits: " + line, flush=True)


def networks_of(stack, sandbox):
    """The sandbox's networks, name to address."""
    record = json.loads(stack.command(["docker", "inspect", "--format", "{{json .NetworkSettings.Networks}}", sandbox]))
    return {name: settings.get("IPAddress", "") for name, settings in record.items()}


def halfopen_reconnect_watch(stack, path):
    """The case itself, from the task's first run to the check."""
    stored_until(stack, path, "run.completed")
    sandbox = container(stack, path)
    before = networks_of(stack, sandbox)
    addresses = {address for address in before.values() if address}
    assert len(addresses) == 1, "one sandbox network: {}".format(len(addresses))
    stack.command(["docker", "exec", "-d", sandbox, "python3", "-c", halfopen_case.RECONNECT_CLIENT])
    deadline, written = time.monotonic() + 30, ""
    while not written:
        assert time.monotonic() < deadline, "the sandbox's keep-alive client never got its answer"
        time.sleep(1)
        written = stack.command(["docker", "exec", sandbox, "sh", "-c",
                                 "cat {} 2>/dev/null || true".format(halfopen_case.PORT_FILE)])
    peer = (next(iter(addresses)), int(written))
    halfopen_case.check_before(gateway_sample(stack), peer, addresses)
    # The soak's link_cut (soak_stack.cut_link); no kill, and the task stays.
    for name in before:
        stack.command(["docker", "network", "disconnect", name, sandbox])

    def reconnect():
        # As the soak's restore_link, but only with the same address: a changed one would test nothing.
        for name, address in before.items():
            try:
                stack.command(["docker", "network", "connect", "--ip", address, name, sandbox])
            except Exception as error:
                raise AssertionError("the reconnect tests nothing: Docker couldn't give the sandbox its address back "
                                     "on {}: {}".format(name, compose.redact(str(error), stack.env))) from None
        halfopen_case.check_restored(before, networks_of(stack, sandbox))

    seen = halfopen_case.watch_kept(lambda: gateway_sample(stack), peer, time.monotonic, time.sleep, reconnect)
    # The second request, on the same socket, whatever the watch saw: its answer is part of the report.
    stack.command(["docker", "exec", sandbox, "touch", halfopen_case.ASK_FILE])
    deadline, answer = time.monotonic() + 20, ""
    while not answer and time.monotonic() < deadline:
        time.sleep(1)
        answer = stack.command(["docker", "exec", sandbox, "sh", "-c",
                                "cat {} 2>/dev/null || true".format(halfopen_case.ANSWER_FILE)]).strip()
    print(halfopen_case.report_kept(seen, answer), flush=True)
    assert container(stack, path) == sandbox, "the task's sandbox was replaced: the connection's end can't be read"
    print(halfopen_case.check_kept(seen, answer) + " (Docker)", flush=True)


def keep_logs(env, failure, configuration):
    """After a failure (not Ctrl-C), before the stack is removed: the gateway's and every service's log and each
    sandbox's (`compose.save_logs`) in `TQA_ROBUSTNESS_EVIDENCE_DIR`/stack-logs, else a new folder that outlives this
    temporary installation, so a failure that does not repeat (TEST-PLAN §19a, GW-I-018) still shows where it stopped.
    The folder is named in the output."""
    if failure is None or isinstance(failure, KeyboardInterrupt):
        return []
    evidence = env.get("TQA_ROBUSTNESS_EVIDENCE_DIR")
    folder = Path(evidence) / "stack-logs" if evidence else Path(tempfile.mkdtemp(prefix="tqa-robustness-logs-"))
    written = compose.save_logs(env, folder, configuration=configuration)
    if written:
        print("Stack logs kept in {}".format(folder), flush=True)
    elif not evidence:
        folder.rmdir()
    return written


# The cases that run their own scenario and add their settings to [sandbox].
CASES = {"lifetime": (lifetime_case.SCENARIO, lifetime_case.SETTINGS), "idle": (idle_case.SCENARIO, idle_case.SETTINGS),
         "halfopen": (halfopen_case.SCENARIO, halfopen_case.SETTINGS),
         "halfopen-reconnect": (halfopen_case.SCENARIO, halfopen_case.RECONNECT_SETTINGS)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", choices=["shutdown", "outage", "daemon", "lifetime", "idle", "halfopen",
                                         "halfopen-reconnect"])
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    if args.case == "daemon" and os.environ.get("TQA_DAEMON_RESTART_TEST") != "1":
        raise SystemExit("Coordinate the daemon restart and set TQA_DAEMON_RESTART_TEST=1 explicitly")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.teardown_on_signals()
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-robustness-")
    directory = compose.installation(temporary)
    stack, failure = Stack(directory, env), None
    try:
        stack.prepare(args.case)
        {"shutdown": shutdown, "outage": outage, "daemon": daemon, "lifetime": lifetime, "idle": idle,
         "halfopen": halfopen, "halfopen-reconnect": halfopen_reconnect}[args.case](stack)
    except BaseException as error:
        failure = error
        print("Robustness contract failed: " + compose.redact(str(error), env), flush=True)
    finally:
        for stream in stack.streams:
            stream.response.close()
        keep_logs(env, failure, stack.configuration)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
