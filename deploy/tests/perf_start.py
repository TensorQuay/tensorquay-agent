"""PERF-010 (PRD M3): Docker sandbox start on Linux, warm image: from the gateway accepting a message on a conversation
without a sandbox to that run's first `run.started` (the runtime is linked, configured and its tools registered).
Target p50 <= 3 s. Each sample uses a new conversation, so each creates a fresh sandbox; the backend split (gateway to
container created, Docker create to start, start to run.started) locates a regression. Then the returning task as a
person sees it on Docker (PERF-010-returning-task, PRD M3's 3 s): a task that already ran, its sandbox removed
after the idle window, so its next message starts a new container on the same workspace with the image warm; from the
message accepted to run.started, with the same split. Run with no other stack running on the host:
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_start.py

PERF-011 from the person's side, `--backend firecracker` (a prepared KVM host and the Firecracker stack's variables,
deploy/firecracker/README.md): the same span on the microVM backend for a new task's first message (the first boot,
its workspace and state images created) and for a returning task's next message (the VM retired at idle, the images
kept), split into accepted to VM host created, VM host create to start, start to the sandbox ready (guest boot and
the runtime linked and configured), and ready to run.started. Targets: p50 <= 2 s cold (PERF-011), and PRD M3's
3 s. Each new task is deleted after its two samples.
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_start.py --backend firecracker
"""
import argparse
import datetime
import http.client
import json
import os
import re
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import compose
import perf_common
from soak_audit import require
from soak_stack import Stack, cleanup_actions
from soak_stream import MAX_RECORD_BYTES, SseDecoder

WARMUPS, SAMPLES, TARGET_MS, STARTED_WITHIN = 5, 50, 3000, 60
# PERF-011's plan target for a cold start, and PRD M3's ceiling for what a person waits.
VM_TARGET_MS, OWNER_MS = 2000, 3000
# The microVM backend retires an idle VM after this many seconds, so a returning task boots again with its images;
# long enough that the VM host is still there to be read once run.started has arrived.
VM_IDLE_SECONDS = 10
# The Docker sandbox's idle window for a returning task, as the microVM's; how long its removal may take beyond it.
IDLE_SECONDS, RETIRE_SECONDS = VM_IDLE_SECONDS, 90


class StartStack(Stack):
    """The soak stack's installation; each new conversation's first turn gets one immediate reply."""
    def configure(self, rehearsal):
        reply = {"events": [{"type": "text", "text": "ready"}], "delay_ms": 0,
                 "usage": {"prompt_tokens": 1, "completion_tokens": 1}}
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": [reply]}))
        compose.write(self.directory.parent / "soak.json", json.dumps({"services": {"fake-model": {
            "image": self.images["fake-model"], "command": ["--scenario", "/scenarios/soak.json"],
            "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                         "target": "/scenarios/soak.json", "read_only": True}]}}}))
        config = self.directory / "gateway.toml"
        compose.write(config, idle_window(config.read_text()))

    def delete(self, conversation):
        request = urllib.request.Request(self.origin + "/api/v1/conversations/" + conversation, method="DELETE",
                                         headers={"Origin": self.origin, "Idempotency-Key": os.urandom(16).hex()})
        with self.client.open(request, timeout=15) as response:
            require(response.status in (200, 202, 204), "conversation delete failed")


def idle_window(text):
    """gateway.toml with the returning task's idle window; a fresh-sandbox sample deletes its task before it counts."""
    require("[sandbox]\n" in text, "gateway.toml has no [sandbox] table")
    return text.replace("[sandbox]\n", "[sandbox]\nidle_seconds={}\n".format(IDLE_SECONDS), 1)


def event_type(frame):
    event = frame.get("event")
    return event.get("type") if isinstance(event, dict) else frame.get("type")


def stream(stack, conversation, timeout):
    """Opens the conversation's live SSE stream from its first event; yields (SSE id, frame) records."""
    address = urllib.parse.urlsplit(stack.origin)
    connection = http.client.HTTPConnection(address.hostname, address.port, timeout=timeout)
    try:
        connection.request("GET", "/api/v1/conversations/{}/events?stream=1&protocol=1".format(conversation), headers={
            "Cookie": stack.cookie, "Origin": stack.origin, "Accept": "text/event-stream", "Last-Event-ID": "0"})
        response = connection.getresponse()
        require(response.status == 200, "the event stream was refused")
        require(response.getheader("Content-Type", "").startswith("text/event-stream"), "not an SSE response")
        decoder = SseDecoder()
        while True:
            line = response.readline(MAX_RECORD_BYTES + 1)
            require(bool(line), "unexpected SSE EOF")
            record = decoder.line(line)
            if record is not None:
                yield record
    finally:
        connection.close()


def watch(stack, conversation, connected, started):
    """Stamps the first run.started on both clocks; `connected` is set once the stream is subscribed."""
    for _identity, frame in stream(stack, conversation, STARTED_WITHIN):
        if frame.get("type") == "session.ready":
            connected.set()
        elif event_type(frame) == "run.started":
            started.update(monotonic=time.monotonic(), wall=time.time())
            return


def docker_time(text):
    """Docker's RFC 3339 instant with nanoseconds, as Unix seconds (microsecond precision)."""
    match = re.fullmatch(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d+))?Z", text)
    require(match, "unexpected Docker timestamp")
    fraction = (match.group(2) or "0")[:6].ljust(6, "0")
    moment = datetime.datetime.fromisoformat(match.group(1) + "." + fraction + "+00:00")
    return moment.timestamp()


def container_times(stack, conversation):
    """Docker's own create and start instants for the conversation's sandbox."""
    record = stack.sandbox(conversation)
    return docker_time(record["Created"]), docker_time(record["State"]["StartedAt"])


def sample(stack):
    """One fresh-sandbox start; returns the total and its backend split, in milliseconds."""
    conversation = stack.request("/api/v1/conversations", {"title": "perf-010", "model_route": "local"})["id"]
    connected, started = threading.Event(), {}
    reader = threading.Thread(target=watch, args=(stack, conversation, connected, started), daemon=True)
    reader.start()
    require(connected.wait(15), "the event stream did not open")
    sent_wall, sent = time.time(), time.monotonic()
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "start"})
    reader.join(STARTED_WITHIN)
    require("monotonic" in started, "no run.started within {} s".format(STARTED_WITHIN))
    created, running = container_times(stack, conversation)
    total = (started["monotonic"] - sent) * 1000
    split = docker_split(sent_wall, created, running, started["wall"])
    stack.delete(conversation)
    stack.runtimes.pop(conversation)  # The deleted sandbox has no logs left to save.
    deadline = time.monotonic() + 60
    while stack.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation]):
        require(time.monotonic() < deadline, "the sandbox was not removed after delete")
        time.sleep(0.2)
    return total, split


def arguments(argv):
    """The backend to measure (Docker unless firecracker is asked for) and the number of samples (§15.1: 50)."""
    def positive(text):
        value = int(text)
        if value < 1:
            raise argparse.ArgumentTypeError("at least one sample")
        return value
    parser = argparse.ArgumentParser(description="PERF-010 (Docker) and PERF-011 (microVM) sandbox start")
    parser.add_argument("--backend", choices=("docker", "firecracker"), default="docker")
    parser.add_argument("--samples", type=positive, default=SAMPLES)
    parser.add_argument("--busy-engine", action="store_true",
                        help="keep the container engine busy while sampling (microVM): two workers loop `docker run`")
    options = parser.parse_args(argv)
    if options.busy_engine and options.backend != "firecracker":
        parser.error("--busy-engine is for --backend firecracker (PERF-011) only; PERF-010 never keeps the engine busy")
    return options


def stage(frame):
    """The start stage a live frame marks: the sandbox creating or ready, or the run started; None otherwise."""
    if frame.get("type") == "session.ready":
        state = (frame.get("sandbox") or {}).get("state")
        return state if state in ("creating", "ready") else None
    return "run.started" if event_type(frame) == "run.started" else None


# The runtime's line once it is linked, configured and its environment loaded (tqa-runtime supervisor/actions.rs): the
# sandbox is ready for the run. The live stream has no frame for it (its next state is busy, at run.started).
REGISTERED = '"runtime session registered"'


def registered_at(log):
    """When the VM host's log (`docker logs --timestamps`, the host's clock) shows the runtime registering its
    session; the first such line counts."""
    for line in log.splitlines():
        stamp, _, text = line.partition(" ")
        if REGISTERED in text:
            return docker_time(stamp)
    require(False, "the VM host's log has no runtime session registered line")


def vm_split(sent, created, running, ready, started, end="run_started"):
    """The four stages, in milliseconds, from wall-clock instants in seconds; they add up to sent..started. `end`
    names what the last stage ends at: the run starting, or an upload answered."""
    instants = [sent, created, running, ready, started]
    require(instants == sorted(instants), "the start's stamps are out of order")
    names = ("accepted_to_vm_host_created_ms", "vm_host_create_to_start_ms", "vm_host_start_to_ready_ms",
             "ready_to_{}_ms".format(end))
    return {name: (later - earlier) * 1000 for name, earlier, later in zip(names, instants, instants[1:])}


def vm_summary(totals, splits):
    """p50 and p99 of the totals, each stage's p50, and both targets: PERF-011's verdict and PRD M3's 3 s."""
    summary = {"p50_ms": perf_common.percentile(totals, 50), "p99_ms": perf_common.percentile(totals, 99)}
    for name in splits[0]:
        summary[name.replace("_ms", "_p50_ms")] = perf_common.percentile([split[name] for split in splits], 50)
    summary["owner_3s_met"] = summary["p50_ms"] <= OWNER_MS
    return summary, "pass" if summary["p50_ms"] <= VM_TARGET_MS else "fail"


def run(stack, samples=SAMPLES, measure=sample):
    for _ in range(WARMUPS):
        measure(stack)
    totals, splits = [], []
    for _ in range(samples):
        total, split = measure(stack)
        totals.append(round(total, 1))
        splits.append({name: round(value, 1) for name, value in split.items()})
    summary = {"p50_ms": perf_common.percentile(totals, 50), "p99_ms": perf_common.percentile(totals, 99)}
    for name in splits[0]:
        summary[name.replace("_ms", "_p50_ms")] = perf_common.percentile([split[name] for split in splits], 50)
    return totals, splits, summary


def last_seq(items):
    """The highest sequence among a task's stored events; 0 for none."""
    return max((item["seq"] for item in items), default=0)


def vm_stream(stack, conversation, after=0):
    """The conversation's live SSE stream on the microVM stack, signed by its cookie jar, from after `after` (a
    returning task's history would otherwise replay its last run.started); yields (SSE id, frame)."""
    request = urllib.request.Request(
        stack.origin + "/api/v1/conversations/{}/events?stream=1&protocol=1".format(conversation),
        headers={"Origin": stack.origin, "Accept": "text/event-stream", "Last-Event-ID": str(after)})
    response = stack.client.open(request, timeout=STARTED_WITHIN)
    try:
        require(response.status == 200, "the event stream was refused")
        decoder = SseDecoder()
        while True:
            line = response.readline(MAX_RECORD_BYTES + 1)
            require(bool(line), "unexpected SSE EOF")
            record = decoder.line(line)
            if record is not None:
                yield record
    finally:
        response.close()


def vm_watch(stack, conversation, connected, armed, stamps, after):
    """Stamps each start stage's first frame once `armed` (the message is about to be sent), on the wall clock, and
    run.started on the monotonic one too; `connected` is set once the stream is subscribed. The sandbox's ready
    instant comes from the VM host's log (`registered_at`)."""
    for _identity, frame in vm_stream(stack, conversation, after):
        if frame.get("type") == "session.ready":
            connected.set()
        name = stage(frame) if armed.is_set() else None
        if name is not None and name not in stamps:
            stamps[name] = time.time()
            if name == "run.started":
                stamps["monotonic"] = time.monotonic()
                stamps["event_ts"] = frame.get("ts")
                return


def vm_hosts(stack, conversation):
    """The task's VM host containers (the backend labels each with its conversation)."""
    return compose.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation], stack.env,
                           capture=True).stdout.split()


def vm_host_times(stack, conversation):
    """Docker's own create and start instants for the task's one VM host, and when its log shows the runtime ready."""
    hosts = vm_hosts(stack, conversation)
    if len(hosts) != 1:
        listed = compose.command(["docker", "ps", "-a", "--filter", "label=tqa.backend=firecracker", "--format",
                                  '{{.ID}} {{.State}} {{.Label "tqa.conversation"}} {{.Label "tqa.role"}}'],
                                 stack.env, capture=True).stdout
        require(False, "expected one VM host for task {}, found {}; firecracker containers:\n{}".format(
            conversation, len(hosts), listed))
    record = json.loads(compose.command(["docker", "inspect", hosts[0]], stack.env, capture=True).stdout)[0]
    logs = compose.command(["docker", "logs", "--timestamps", hosts[0]], stack.env, capture=True)
    ready = registered_at(logs.stdout + "\n" + logs.stderr)
    return docker_time(record["Created"]), docker_time(record["State"]["StartedAt"]), ready


def vm_gone(stack, conversation, seconds=120):
    """Waits (bounded) until the task has no VM host: retired at idle, or removed with the task."""
    deadline = time.monotonic() + seconds
    while vm_hosts(stack, conversation):
        require(time.monotonic() < deadline, "the task's VM host was not removed within {} s".format(seconds))
        time.sleep(0.5)


def message_started(stack, conversation, text):
    """Sends a message on a task, reading its stream from after the task's stored events; returns the wall and
    monotonic instants it was sent and each start stage's stamps, run.started's on both clocks."""
    connected, armed, stamps = threading.Event(), threading.Event(), {}
    stored = stack.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]
    require(len(stored) < 1000, "a perf task's history outgrew one page")
    reader = threading.Thread(target=vm_watch, args=(stack, conversation, connected, armed, stamps, last_seq(stored)),
                              daemon=True)
    reader.start()
    require(connected.wait(15), "the event stream did not open")
    armed.set()
    sent_wall, sent = time.time(), time.monotonic()
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": text})
    reader.join(STARTED_WITHIN)
    require("monotonic" in stamps, "no run.started within {} s".format(STARTED_WITHIN))
    return sent_wall, sent, stamps


def vm_sample(stack, conversation, text):
    """One start of the task's VM for a message: the total and its four stages, in milliseconds."""
    sent_wall, sent, stamps = message_started(stack, conversation, text)
    created, running, ready = vm_host_times(stack, conversation)
    split = vm_split(sent_wall, created, running, ready, stamps["run.started"])
    # Not a stage: how long the stream took to deliver run.started after the event's own instant.
    split["stream_lag_ms"] = (stamps["run.started"] - docker_time(stamps["event_ts"])) * 1000
    return (stamps["monotonic"] - sent) * 1000, split


def vm_pair(stack):
    """A new task's first message (first boot, images created), then, once its VM retired at idle, its next message
    (a returning task); the task is deleted afterwards so its images don't accumulate."""
    conversation = stack.request("/api/v1/conversations", {"title": "perf-011", "model_route": "local"})["id"]
    cold = vm_sample(stack, conversation, "start")
    vm_gone(stack, conversation)
    returning = vm_sample(stack, conversation, "again")
    delete_task(stack, conversation)
    return cold, returning


def docker_split(sent_wall, created, running, started):
    """PERF-010's backend split: accepted to container created, Docker create to start, start to run.started."""
    return {"to_container_created_ms": (created - sent_wall) * 1000, "create_to_start_ms": (running - created) * 1000,
            "start_to_run_started_ms": (started - running) * 1000}


def returning_sample(stack):
    """A Docker task's first message, then, once its sandbox was removed at idle, its next message: the returning
    task's total and split, in milliseconds. The task is deleted afterwards."""
    conversation = stack.request("/api/v1/conversations", {"title": "perf-010-returning", "model_route": "local"})["id"]
    message_started(stack, conversation, "start")
    vm_gone(stack, conversation, IDLE_SECONDS + RETIRE_SECONDS)
    stack.runtimes.pop(conversation, None)  # Its next sandbox is a new container, not a replaced healthy one.
    sent_wall, sent, stamps = message_started(stack, conversation, "again")
    created, running = container_times(stack, conversation)
    total, split = (stamps["monotonic"] - sent) * 1000, docker_split(sent_wall, created, running,
                                                                      stamps["run.started"])
    stack.delete(conversation)
    stack.runtimes.pop(conversation, None)
    vm_gone(stack, conversation, 60)
    return total, split


def delete_task(stack, conversation):
    """Deletes the task and waits until its VM host is gone, so its images don't accumulate."""
    request = urllib.request.Request(stack.origin + "/api/v1/conversations/" + conversation, method="DELETE",
                                     headers={"Origin": stack.origin, "Idempotency-Key": os.urandom(16).hex()})
    with stack.client.open(request, timeout=30) as response:
        require(response.status in (200, 202, 204), "conversation delete failed")
    vm_gone(stack, conversation)


def vm_upload(stack):
    """A new task's first upload (8 bytes, as E2E-VM-013): no live sandbox, so it starts the VM, and the runtime in
    the guest stores the file. The total and its four stages, ending at the upload's answer, in milliseconds."""
    conversation = stack.request("/api/v1/conversations", {"title": "perf-011-upload", "model_route": "local"})["id"]
    request = urllib.request.Request(
        stack.origin + "/api/v1/conversations/{}/uploads?name=data.csv".format(conversation), data=b"a,b\n1,2\n",
        method="POST", headers={"Origin": stack.origin, "Content-Type": "application/octet-stream"})
    sent_wall, sent = time.time(), time.monotonic()
    try:
        with stack.client.open(request, timeout=STARTED_WITHIN) as response:
            response.read(65536)
            require(response.status == 201, "the upload was refused")
    except urllib.error.HTTPError as error:
        # The body says why (the c14b Gate 9 run, 5 Oct 2026: a 409 nobody could classify without it).
        body = error.read(4096).decode("utf-8", "replace")
        require(False, "the upload was refused: HTTP {} {}".format(error.code, body))
    answered_wall, answered = time.time(), time.monotonic()
    created, running, ready = vm_host_times(stack, conversation)
    split = vm_split(sent_wall, created, running, ready, answered_wall, end="upload_answered")
    delete_task(stack, conversation)
    return (answered - sent) * 1000, split


# How many workers keep the engine busy in a --busy-engine run, each looping a throwaway container.
BUSY_WORKERS = 2


def busy_engine(env, stop):
    """Starts workers that loop a throwaway `docker run` until `stop` is set: the Docker work a busy host does, which
    is what slows a new task's images (PERF-011's first stage). Returns the threads to join."""
    def work():
        while not stop.is_set():
            try:
                compose.command(["docker", "run", "--rm", "--network", "none", compose.TOYBOX, "true"], env,
                                timeout=120)
            except Exception:  # noqa: BLE001 (a failed throwaway run only means the engine is busier still)
                time.sleep(0.5)
    workers = [threading.Thread(target=work, daemon=True) for _ in range(BUSY_WORKERS)]
    for worker in workers:
        worker.start()
    return workers


def vm_images(env):
    """What the microVM start ran on (§15.1 item 1): every image ID and the guest kernel's manifest."""
    images = {role: env.get(variable, "") for role, variable in (
        ("gateway", "TQA_GATEWAY_IMAGE"), ("runtime", "TQA_RUNTIME_IMAGE"), ("vmm", "TQA_VMM_IMAGE"),
        ("fake-model", "TQA_FAKE_MODEL_IMAGE"))}
    kernel = Path(env["TQA_FC_KERNEL_DIR"]) / "manifest.json"
    return images, json.loads(kernel.read_text())


def microvm(samples, busy=False):
    """PERF-011 from the person's side on a disposable microVM stack; writes the new-task and returning-task results."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "firecracker"))
    import stack as vm_stack  # noqa: E402  (the Firecracker harness, beside this directory)
    reply = {"events": [{"type": "text", "text": "ready"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}
    written = []

    def check(stack):
        stop = threading.Event()
        workers = busy_engine(stack.env, stop) if busy else []
        try:
            measure(stack)
        finally:
            stop.set()
            for worker in workers:
                worker.join(150)

    def measure(stack):
        stack.sign_in()
        images, kernel = vm_images(stack.env)
        record = perf_common.environment(stack.env, images)
        # The installation's very first VM start, before any warm-up: a new person's first impression (E2E-VM-013's
        # upload took 4964 ms here). One sample per stack; its own result, never mixed into the warm ones.
        first_total, first_split = vm_upload(stack)
        path = perf_common.write("PERF-011-first-start", record, {
            "samples": 1, "definition": "the installation's first VM start: a new task's first upload, right after "
            "the stack is up (nothing started before it)", "backend": "firecracker", "guest_kernel": kernel,
            "busy_engine": busy},
            {"total_ms": [round(first_total, 1)], "split": [{k: round(v, 1) for k, v in first_split.items()}]},
            {"p50_ms": round(first_total, 1), "p99_ms": round(first_total, 1)},
            {"p50_ms_max": VM_TARGET_MS, "owner_ms": OWNER_MS},
            "pass" if first_total <= VM_TARGET_MS else "fail")
        print("PERF-011-first-start {:.0f} ms split={} -> {}".format(
            first_total, {k: round(v) for k, v in first_split.items()}, path), flush=True)
        for _ in range(WARMUPS):
            vm_pair(stack)
            vm_upload(stack)
        cold, returning, upload = ([], []), ([], []), ([], [])
        for _ in range(samples):
            measured = vm_pair(stack) + (vm_upload(stack),)
            for (totals, splits), (total, split) in zip((cold, returning, upload), measured):
                totals.append(round(total, 1))
                splits.append({name: round(value, 1) for name, value in split.items()})
        for identity, (totals, splits), definition in (
                ("PERF-011-new-task", cold, "a new task's first message accepted to its first run.started: first "
                 "boot, workspace and state images created"),
                ("PERF-011-returning-task", returning, "a returning task's next message accepted to run.started: "
                 "its VM retired at idle, its images kept"),
                ("PERF-011-upload-new-task", upload, "a new task's first upload (8 bytes) sent to its answer: it "
                 "starts the VM (first boot, images created) and the runtime stores the file")):
            summary, verdict = vm_summary(totals, splits)
            path = perf_common.write(identity, record, {
                "warmups": WARMUPS, "samples": samples, "definition": definition, "backend": "firecracker",
                "idle_seconds": VM_IDLE_SECONDS, "guest_kernel": kernel, "busy_engine": busy},
                {"total_ms": totals, "split": splits}, summary, {"p50_ms_max": VM_TARGET_MS, "owner_ms": OWNER_MS},
                verdict)
            written.append((identity, summary, verdict, path))
            print("{} {} p50={:.0f} ms p99={:.0f} ms owner 3 s {} split={} -> {}".format(
                identity, verdict, summary["p50_ms"], summary["p99_ms"], "met" if summary["owner_3s_met"] else "missed",
                {name: value for name, value in summary.items() if name.endswith("_p50_ms")}, path), flush=True)

    # vm_stack.run copies the process's environment: the readings' folder goes there, before the stack starts.
    perf_common.keep_readings(os.environ, "PERF-011")
    vm_stack.run("PERF-011", {"model": "fixture", "responses": [reply]}, check,
                 sandbox_settings="idle_seconds = {}\n".format(VM_IDLE_SECONDS))
    return written


def main(argv=None):
    compose.teardown_on_signals()  # a SIGTERM (gate9_run's time limit, a stop) tears the stack down like Ctrl-C
    options = arguments(sys.argv[1:] if argv is None else argv)
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("PERF-010 and PERF-011 need Docker: set TQA_DOCKER_TESTS=1 and run with no other stack "
                         "running")
    if options.backend == "firecracker":
        written = microvm(options.samples, options.busy_engine)
        if any(verdict != "pass" for _identity, _summary, verdict, _path in written):
            raise SystemExit("PERF-011 p50 exceeds {} ms: {}".format(VM_TARGET_MS, ", ".join(
                "{} {:.0f} ms".format(identity, summary["p50_ms"]) for identity, summary, _verdict, _path in written)))
        return
    env = os.environ.copy()
    perf_common.keep_readings(env, "PERF-010")
    temporary = tempfile.TemporaryDirectory(prefix="tqa-perf-010-")
    directory = Path(temporary.name) / "installation"
    directory.mkdir(mode=0o700)
    stack, failure = StartStack(directory, env, perf_common.log_directory("PERF-010")), None
    try:
        stack.prepare(rehearsal=True)
        record = perf_common.environment(env, stack.images)
        missed = []
        for identity, measure, definition in (
                ("PERF-010", sample, "message accepted to first run.started, fresh sandbox per sample"),
                ("PERF-010-returning-task", returning_sample,
                 "a returning task's next message accepted to run.started: the task already ran and its sandbox was "
                 "removed after the {} s idle window, so a new container starts on its workspace, the image warm"
                 .format(IDLE_SECONDS))):
            totals, splits, summary = run(stack, options.samples, measure)
            verdict = "pass" if summary["p50_ms"] <= TARGET_MS else "fail"
            path = perf_common.write(identity, record, {"warmups": WARMUPS, "samples": options.samples,
                                     "definition": definition, "idle_seconds": IDLE_SECONDS,
                                     "runtime_image": stack.images["runtime"]},
                                     {"total_ms": totals, "split": splits}, summary, {"p50_ms_max": TARGET_MS},
                                     verdict)
            print("{} {} p50={:.0f} ms p99={:.0f} ms split={} -> {}".format(
                identity, verdict, summary["p50_ms"], summary["p99_ms"],
                {name: value for name, value in summary.items() if name.endswith("_p50_ms")}, path), flush=True)
            if verdict != "pass":
                missed.append("{} p50 {:.0f} ms exceeds {} ms".format(identity, summary["p50_ms"], TARGET_MS))
        if missed:
            failure = AssertionError("; ".join(missed))
    except BaseException as error:
        failure = error
    finally:
        failure = cleanup_actions([stack.save_logs], failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
    if failure is not None:
        raise SystemExit("PERF-010 failed: {}".format(failure))


if __name__ == "__main__":
    main()
