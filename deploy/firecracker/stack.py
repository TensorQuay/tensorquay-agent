"""The Compose stack on the microVM backend (SBX-F-020, Gate 8): deploy/tests/compose.py's installation plus
deploy/compose/firecracker.yml, the guest kernel and the runtime template's rootfs in the storage root, and the
gateway configured as deploy/firecracker/README.md describes. Rootless; nothing here needs root. The host must pass
`host-setup.sh --check` (and `kvm-no-uaccess.sh --check`) first.

Run from the repository root, one stack at a time on the host (a rootless Docker daemon stalls under concurrent stacks):
    TQA_DOCKER_TESTS=1 TQA_VMM_IMAGE=<tqa-vmm image> TQA_FC_KERNEL_DIR=<kernel dir> python3 deploy/firecracker/stack.py smoke
Optional: TQA_FC_ROOTFS_DIR (a rootfs cache to copy from), TQA_RUNTIME_IMAGE, TQA_GATEWAY_IMAGE, TQA_FAKE_MODEL_IMAGE.
"""
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
import compose  # noqa: E402
import demo  # noqa: E402

OVERLAY = ["-f", "deploy/compose/firecracker.yml"]
# Each stack's sandbox network is a 10.215.X.0/24 that no network on the engine overlaps, drawn at random among the
# free ones, with the gateway pinned at .10: apart from the Docker stacks' pools (10.213.X), the pool deploy/README.md
# suggests for a second installation (10.214.0.0/16) and the KVM test network (198.51.100.0/24). A fixed subnet made a second microVM stack on the engine fail to create its network, so every
# G-13 stack failed while another ran (13h-pre).
VM_NETWORKS = "10.215.{}.0/24"
GATEWAY_HOST = 10


def vm_network(used, randbelow=secrets.randbelow):
    """A free sandbox network for one stack and the gateway's pinned address in it, given the engine's subnets."""
    taken = [ipaddress.ip_network(subnet, strict=False) for subnet in used]
    free = [candidate for candidate in (ipaddress.ip_network(VM_NETWORKS.format(third)) for third in range(256))
            if not any(candidate.version == network.version and candidate.overlaps(network) for network in taken)]
    if not free:
        raise RuntimeError("no free 10.215.X.0/24 sandbox network on this engine")
    subnet = free[randbelow(len(free))]
    return str(subnet), str(subnet.network_address + GATEWAY_HOST)


# Values that only let a torn-down stack interpolate the overlay before it drew its network; never used to create one.
UNDRAWN = {"TQA_FC_SUBNET": VM_NETWORKS.format(0),
           "TQA_FC_GATEWAY_ADDRESS": str(ipaddress.ip_network(VM_NETWORKS.format(0)).network_address + GATEWAY_HOST)}


def claim_network(env):
    """Draws this stack's sandbox network from the engine's free ones and sets it for the overlay; returns the
    gateway's pinned address, which goes into gateway.toml."""
    subnet, gateway = vm_network(compose.engine_subnets(env))
    env.update(TQA_FC_SUBNET=subnet, TQA_FC_GATEWAY_ADDRESS=gateway)
    return gateway


def scenario_for(scenario, gateway):
    """A case's scenario: as given, or made from the gateway's address once the stack has drawn its network."""
    return scenario(gateway) if callable(scenario) else scenario
# The workspace quota a stack's tasks get; a new workspace image is this plus ext4 metadata, sparse. A test workspace
# needs little of it; without it, images follow the 10 GiB default quota.
WORKSPACE_QUOTA = 1 << 30
IMAGE_ID = re.compile(r"sha256:[0-9a-f]{64}")


def image_id(reference, env):
    identity = compose.command(["docker", "image", "inspect", "--format", "{{.Id}}", reference], env,
                               capture=True).stdout.strip()
    if not IMAGE_ID.fullmatch(identity):
        raise ValueError("{} must resolve to an immutable local image ID".format(reference))
    return identity


def vm_host_under_test(env, vmm_image):
    """A microVM's sandbox container runs the VM host image, so the sandbox image guard (`compose.retire`) counts it
    under test; the microVMs' root filesystems are made only from the runtime under test (`artefacts`)."""
    pinned = [image for image in env.get("TQA_PINNED_IMAGES", "").split(",") if image]
    env["TQA_PINNED_IMAGES"] = ",".join(sorted(set(pinned) | {vmm_image}))


def firecracker_config(text, vmm_image, gateway_address, workspace_quota=WORKSPACE_QUOTA):
    """gateway.toml for the microVM backend: the backend switched, its section with the pinned address, and the
    workspace quota that sizes a new workspace image (a case that fills the workspace asks for a small one)."""
    if "backend='docker'" not in text or "[firecracker]" in text:
        raise ValueError("expected the Docker installation's gateway.toml")
    return (text.replace("backend='docker'", "backend='firecracker'", 1)
            + "[firecracker]\nvmm_image=" + json.dumps(vmm_image) + "\ngateway_address=" + json.dumps(gateway_address)
            + "\n[workspaces]\nquota_bytes=" + str(workspace_quota) + "\n")


def cached_rootfs(cache, runtime, vmm_image):
    """The cached rootfs directory for `runtime` when it was built by this VM host image, else None."""
    if cache is None:
        return None
    directory = Path(cache) / runtime.removeprefix("sha256:")
    try:
        manifest = json.loads((directory / "manifest.json").read_text())
    except (OSError, ValueError):
        return None
    current = manifest.get("builder", {}).get("image_id") == vmm_image and manifest.get("schema") == 2
    return directory if current else None


def artefacts(storage, env, runtime, vmm_image):
    """The kernel and the runtime's rootfs where the gateway's defaults look: storage.root/kernel and /images."""
    kernel = Path(env["TQA_FC_KERNEL_DIR"])
    (storage / "kernel").mkdir()
    for name in ("vmlinux", "manifest.json"):
        shutil.copy2(kernel / name, storage / "kernel" / name)
    images = storage / "images"
    images.mkdir()
    cached = cached_rootfs(env.get("TQA_FC_ROOTFS_DIR"), runtime, vmm_image)
    if cached is not None:
        compose.command(["cp", "-a", "--sparse=always", str(cached), str(images / cached.name)], env, timeout=600)
    else:
        compose.command(["python3", str(ROOT / "images" / "rootfs" / "build_rootfs.py"), "--image", runtime,
                         "--vmm-image", vmm_image, "--out", str(images)], env, timeout=1800)


class Stack:
    """One disposable installation on the microVM backend, driven through the public API."""

    def __init__(self, directory, env):
        self.directory, self.env = directory, env
        self.configuration = demo.COMPOSE + OVERLAY

    def prepare(self, scenario, sandbox_settings="", appended="", workspace_quota=WORKSPACE_QUOTA):
        """Writes the installation and the fake model's scenario, then starts the stack. `sandbox_settings` are extra
        `[sandbox]` lines (for example a short `max_lifetime_seconds`); `appended` adds whole sections."""
        # The images under test, named before anything is claimed or written (the engine is shared).
        for name in ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_FAKE_MODEL_IMAGE"):
            compose.required_image(self.env, name)
        # First, so that tearing down after any failure can still interpolate the overlay.
        self.gateway_address = claim_network(self.env)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        runtime = image_id(compose.required_image(self.env, "TQA_RUNTIME_IMAGE"), self.env)
        vmm_image = image_id(self.env["TQA_VMM_IMAGE"], self.env)
        compose.prepare(self.directory, self.env, runtime=runtime, port=port)
        vm_host_under_test(self.env, vmm_image)
        compose.ownership(self.directory, self.env)
        config = self.directory / "gateway.toml"
        text = firecracker_config(config.read_text(), vmm_image, self.gateway_address, workspace_quota)
        compose.write(config, text.replace("[sandbox]\n", "[sandbox]\n" + sandbox_settings, 1) + appended)
        artefacts(self.directory / "storage", self.env, runtime, vmm_image)
        compose.write(self.directory / "scenario.json", json.dumps(scenario_for(scenario, self.gateway_address)))
        compose.write(self.directory.parent / "scenario.yml", json.dumps({"services": {"fake-model": {
            "command": ["--scenario", "/scenarios/firecracker.json"],
            "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                         "target": "/scenarios/firecracker.json", "read_only": True}]}}}))
        self.configuration = demo.COMPOSE + OVERLAY + ["-f", str(self.directory.parent / "scenario.yml")]
        compose.ownership(self.directory, self.env, "1000:1000")
        self.origin = "http://127.0.0.1:" + str(port)
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())
        compose.command(self.configuration + ["up", "-d", "--no-build", "--wait", "--wait-timeout", "120"],
                        self.env, timeout=180)

    def request(self, path, body=None, *, stream=False, method=None):
        headers = {"Origin": self.origin, "Content-Type": "application/json"}
        if body is not None:
            headers["Idempotency-Key"] = secrets.token_hex(16)
        data = None if body is None else json.dumps(body).encode()
        response = self.client.open(urllib.request.Request(self.origin + path, headers=headers, data=data, method=method),
                                    timeout=300 if stream else 30)
        if stream:
            return response
        with response:
            payload = response.read(1048577)
        if len(payload) > 1048576:
            raise AssertionError("response exceeded the fixture cap")
        return json.loads(payload) if payload else None

    def sign_in(self):
        password = "synthetic-firecracker-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "firecracker@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)

    def vm_hosts(self):
        """The running VM host containers of this installation, by the backend's instance label."""
        output = compose.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"]],
                                 self.env, capture=True).stdout
        return output.split()


# The engine's own words for a container that is already gone or being removed; nothing else counts as removed.
ALREADY_GONE = ("No such container", "is already in progress")


def forced_removal(returncode, stderr):
    """A `docker rm --force` after a stop: removed, or already gone because the gateway removed it first; any other
    failure is refused with the engine's own words."""
    if returncode == 0:
        return "removed"
    if any(marker in stderr for marker in ALREADY_GONE):
        return "already gone"
    raise AssertionError("docker rm --force failed (exit {}): {}".format(returncode, stderr.strip()[:500]))


def events(stack, conversation, after=0):
    """Stored events after `after`, in order."""
    return stack.request("/api/v1/conversations/{}/events?after={}&limit=1000".format(conversation, after))["items"]



# Log lines printed per exited service before teardown removes it.
LOG_TAIL_LINES = 200
# A failed case's gateway log, every level: enough to reach back past a VM's loss to the case's start.
CASE_LOG_LINES = 1000


def exited_services(ps_output, *, failed):
    """Services that stopped: those that failed always, and after a failed case every stopped one. `docker compose ps
    --format json` prints one object per line (newer Compose) or one array (older)."""
    text = ps_output.strip()
    rows = json.loads(text) if text.startswith("[") else [json.loads(line) for line in text.splitlines() if line.strip()]
    stopped = [row for row in rows if row.get("State") not in ("running", "restarting", "created")]
    return sorted(row["Service"] for row in stopped if failed or row.get("ExitCode", 0) != 0)


def dump_exited(configuration, env, *, failed, run=compose.command, out=print):
    """Prints the redacted log tail of every exited service before teardown removes it; never raises, so a diagnostic
    can't hide the case's own outcome."""
    try:
        listed = run(configuration + ["ps", "--all", "--format", "json"], env, capture=True, timeout=60).stdout
        services = exited_services(listed, failed=failed)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        out("could not list the stack's services: " + compose.redact(str(error), env))
        return
    for service in services:
        try:
            logs = run(configuration + ["logs", "--no-color", "--tail", str(LOG_TAIL_LINES), service], env, capture=True,
                       timeout=60).stdout
        except (OSError, subprocess.SubprocessError) as error:
            logs = "(logs unavailable: {})".format(error)
        out("--- {} exited; last {} log lines ---\n{}".format(service, LOG_TAIL_LINES, compose.redact(logs, env)))


def run(case, scenario, check, sandbox_settings="", appended="", workspace_quota=WORKSPACE_QUOTA):
    """Starts a disposable stack for `scenario`, runs `check(stack)`, and always tears the stack down."""
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Firecracker stack tests require TQA_DOCKER_TESTS=1 (and a prepared KVM host)")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-firecracker-")
    directory = compose.installation(temporary)
    stack, failure, prepared = Stack(directory, env), None, False
    recording = directory.parent / "vm-hosts"
    recording.mkdir()
    recorder = VmHostRecorder(env, recording)
    try:
        stack.prepare(scenario, sandbox_settings, appended, workspace_quota)
        prepared = True
        dump_startup_retries(stack.configuration, env, out=lambda text: print(text, flush=True))
        recorder.start()
        check(stack)
    except BaseException as error:
        failure = error
        print(case + " failed: " + compose.redact(str(error), env), flush=True)
    finally:
        recorder.stop()
        # Only once the installation exists (as retire() checks), so a failure before it has nothing to list.
        if "COMPOSE_PROJECT_NAME" in env:
            printer = lambda text: print(text, flush=True)  # noqa: E731
            dump_exited(stack.configuration, env, failed=failure is not None, out=printer)
            if failure is not None:
                # A case can fail with every service still running: then the VM hosts and the gateway's own
                # warnings are what say why.
                dump_vm_hosts(env, out=printer)
                # Every VM host that existed during the case, a lost one included, and the gateway's whole recent log
                # (the INFO lines around a loss say what the gateway saw).
                recorder.dump(out=printer)
                dump_gateway_warnings(stack.configuration, env, out=printer)
                if prepared:
                    dump_gateway_tail(stack.configuration, env, lines=CASE_LOG_LINES, out=printer)
            if came_up_failed(failure, prepared=prepared):
                dump_service_states(stack.configuration, env, out=printer)
                dump_gateway_tail(stack.configuration, env, out=printer)
                dump_gateway_threads(stack.configuration, env, out=printer)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


def timeline(stored, last=15):
    """The conversation's last stored events, for a timeout's message."""
    if not stored:
        return "no stored events"
    return ", ".join("{} {}".format(frame.get("seq"), frame.get("event", {}).get("type", frame.get("type")))
                     for frame in stored[-last:])


class VmHostRecorder:
    """Follows every VM host of the installation from the moment it appears: its log (`docker logs -f`, the console to
    its last line) and its last inspected state. The gateway removes a lost VM's host as soon as it notices, before any
    failure dump could list it; this keeps what each one showed, gone ones included."""

    def __init__(self, env, directory, *, hosts=None, state=None, follow=None, events=None):
        self.env, self.directory = env, directory
        self.hosts, self.state, self.follow = hosts or self.listed, state or self.inspected, follow or self.followed
        # The engine's own record of each container's end (die with its exit code, oom, kill), even one removed
        # between two polls.
        self.events, self.events_path, self.events_process = events or self.followed_events, directory / "events", None
        self.seen, self.current = {}, set()
        self.stopping, self.thread = threading.Event(), None

    def followed_events(self, path):
        with path.open("wb") as output:
            return subprocess.Popen(["docker", "events", "--filter",
                                     "label=tqa.instance=" + self.env.get("TQA_SANDBOX_NETWORK", ""),
                                     "--format", "{{json .}}"], stdout=output, stderr=subprocess.STDOUT, env=self.env)

    def start_events(self):
        self.events_process = self.events(self.events_path)

    def listed(self):
        return compose.command(["docker", "ps", "-aq", "--no-trunc", "--filter",
                                "label=tqa.instance=" + self.env.get("TQA_SANDBOX_NETWORK", "")],
                               self.env, capture=True, timeout=30).stdout.split()

    def inspected(self, host):
        try:
            return json.loads(compose.command(["docker", "inspect", "--format", "{{json .State}}", host], self.env,
                                              capture=True, timeout=30).stdout)
        except (OSError, ValueError, subprocess.SubprocessError):
            return None

    def followed(self, host, path):
        with path.open("wb") as output:
            return subprocess.Popen(["docker", "logs", "-f", "--timestamps", host], stdout=output,
                                    stderr=subprocess.STDOUT, env=self.env)

    def poll(self):
        try:
            listed = self.hosts()
        except (OSError, ValueError, subprocess.SubprocessError):
            return
        self.current = set(listed)
        for host in listed:
            if host not in self.seen:
                path = self.directory / (host[:12] + ".log")
                self.seen[host] = {"path": path, "state": None, "process": self.follow(host, path)}
            state = self.state(host)
            if state:
                self.seen[host]["state"] = state

    def start(self, interval=0.5):
        self.start_events()

        def loop():
            while not self.stopping.wait(interval):
                self.poll()
        self.thread = threading.Thread(target=loop, name="vm-hosts", daemon=True)
        self.thread.start()

    def stop(self):
        self.stopping.set()
        if self.thread is not None:
            self.thread.join(timeout=10)
        for process in [record["process"] for record in self.seen.values()] + [self.events_process]:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()

    def dump(self, out=print, lines=40):
        """Each VM host that existed during the case: its state, and its log's last lines, redacted."""
        if not self.seen:
            out("--- VM hosts: no VM host was seen ---")
        for host, record in self.seen.items():
            state = record["state"] or {}
            head = "VM host {}{}: {}".format(host[:12], "" if host in self.current else " (gone)",
                                             state.get("Status", "unknown"))
            if state.get("Status") not in (None, "running"):
                head += ", exit {}, finished {}".format(state.get("ExitCode"), state.get("FinishedAt"))
            text = record["path"].read_text(errors="replace") if record["path"].exists() else ""
            out(head + "\n" + compose.redact("\n".join(text.splitlines()[-lines:]), self.env))
        ends = []
        text = self.events_path.read_text(errors="replace") if self.events_path.exists() else ""
        for line in text.splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if event.get("Type") == "container" and event.get("Action") in ("oom", "kill", "die", "stop"):
                actor = event.get("Actor") or {}
                attributes = actor.get("Attributes") or {}
                code = " exitCode=" + attributes["exitCode"] if "exitCode" in attributes else ""
                ends.append("{} {} {}{}".format(actor.get("ID", "")[:12], attributes.get("name", "?"),
                                                event["Action"], code))
        out("--- engine events: container ends ({}) ---\n{}".format(len(ends), "\n".join(ends)))

def dump_vm_hosts(env, *, run=compose.command, out=print):
    """Every VM host of this installation with its state and last log lines, redacted; never raises."""
    try:
        listed = run(["docker", "ps", "-a", "--filter", "label=tqa.instance=" + env.get("TQA_SANDBOX_NETWORK", ""),
                      "--format", "{{.ID}} {{.State}}({{.Status}}) {{.Names}}"], env, capture=True, timeout=60).stdout
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        out("could not list the VM hosts: " + compose.redact(str(error), env))
        return
    for line in [line for line in listed.splitlines() if line.strip()]:
        out("VM host " + line)
        try:
            logs = run(["docker", "logs", "--tail", "30", line.split()[0]], env, capture=True, timeout=60)
            out(compose.redact(logs.stdout + logs.stderr, env))
        except (OSError, subprocess.SubprocessError) as error:
            out("  (logs unavailable: {})".format(compose.redact(str(error), env)))


def dump_gateway_warnings(configuration, env, *, run=compose.command, out=print):
    """The gateway's WARN and ERROR lines so far, redacted; never raises."""
    try:
        logs = run(configuration + ["logs", "--no-color", "--no-log-prefix", "gateway"], env, capture=True,
                   timeout=60).stdout
    except (OSError, subprocess.SubprocessError) as error:
        out("could not read the gateway's log: " + compose.redact(str(error), env))
        return
    warnings = []
    for line in logs.splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if isinstance(record, dict) and record.get("level") in ("WARN", "ERROR"):
            warnings.append(compose.redact(line, env))
    out("--- gateway WARN/ERROR lines ({}) ---\n{}".format(len(warnings), "\n".join(warnings[-60:])))


def came_up_failed(failure, *, prepared):
    """Whether the case failed before its stack was up (then no case ran, and the services' state says why)."""
    return failure is not None and not prepared


def dump_service_states(configuration, env, *, run=compose.command, out=print):
    """Each service's engine state, health and its last two health probes, redacted; never raises. A service that
    runs but never turned healthy is invisible to the exited-service dump."""
    try:
        ids = run(configuration + ["ps", "--all", "-q"], env, capture=True, timeout=60).stdout.split()
        inspected = json.loads(run(["docker", "inspect", *ids], env, capture=True, timeout=60).stdout) if ids else []
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        out("could not inspect the stack's services: " + compose.redact(str(error), env))
        return
    lines = []
    for container in inspected:
        state = container.get("State") or {}
        health = state.get("Health") or {}
        lines.append("{} {} exit {} started {} finished {} health {} streak {}".format(
            container.get("Name", "?").lstrip("/"), state.get("Status"), state.get("ExitCode"), state.get("StartedAt"),
            state.get("FinishedAt"), health.get("Status", "none"), health.get("FailingStreak", 0)))
        for probe in (health.get("Log") or [])[-2:]:
            lines.append("  probe exit {}: {}".format(probe.get("ExitCode"), (probe.get("Output") or "").strip()[:300]))
    out("--- service states ({}) ---\n{}".format(len(inspected), compose.redact("\n".join(lines), env)))


def dump_gateway_tail(configuration, env, *, lines=LOG_TAIL_LINES, run=compose.command, out=print):
    """The gateway's last log lines, every level, redacted; never raises."""
    try:
        logs = run(configuration + ["logs", "--no-color", "--tail", str(lines), "gateway"], env, capture=True,
                   timeout=60).stdout
    except (OSError, subprocess.SubprocessError) as error:
        out("could not read the gateway's log: " + compose.redact(str(error), env))
        return
    out("--- gateway: last {} log lines ---\n{}".format(lines, compose.redact(logs, env)))

def dump_gateway_threads(configuration, env, *, run=compose.command, out=print):
    """Whether the gateway's process still runs and, if so, each thread's state and wait channel: a gateway alive but
    unready after a start-up error is a hang, and this is where it waits. Never raises."""
    try:
        ids = run(configuration + ["ps", "-q", "gateway"], env, capture=True, timeout=60).stdout.split()
        if not ids:
            out("--- gateway threads: no gateway process running ---")
            return
        top = run(["docker", "top", ids[0], "-eLo", "pid,lwp,stat,wchan:32,etime,comm"], env, capture=True,
                  timeout=60).stdout
    except (OSError, subprocess.SubprocessError) as error:
        out("could not list the gateway's threads: " + compose.redact(str(error), env))
        return
    out("--- gateway threads (process alive) ---\n" + compose.redact(top, env))

ENGINE_TIMEOUT = "a Docker API request for the Firecracker backend timed out"
STARTUP_RETRY = "the sandbox backend's engine timed out at start-up; retrying"


def dump_startup_retries(configuration, env, *, run=compose.command, out=print):
    """After every start: how many engine calls timed out while the gateway started and how often it retried
    (GW-U-071), with those lines, redacted; never raises."""
    try:
        logs = run(configuration + ["logs", "--no-color", "--no-log-prefix", "gateway"], env, capture=True,
                   timeout=60).stdout
    except (OSError, subprocess.SubprocessError) as error:
        out("could not read the gateway's log: " + compose.redact(str(error), env))
        return
    timeouts, retries = [], []
    for line in logs.splitlines():
        try:
            message = (json.loads(line).get("fields") or {}).get("message")
        except (ValueError, AttributeError):
            continue
        if message == ENGINE_TIMEOUT:
            timeouts.append(compose.redact(line, env))
        elif message == STARTUP_RETRY:
            retries.append(compose.redact(line, env))
    out("gateway start-up: {} engine timeout(s), {} retry(ies)".format(len(timeouts), len(retries))
        + "".join("\n  " + line for line in retries))

def until(stack, conversation, kinds, seconds=180):
    """Stored events once one of `kinds` is stored (bounded, polling the replay API)."""
    import time
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        stored = events(stack, conversation)
        if any(frame.get("event", {}).get("type") in kinds for frame in stored):
            return stored
        time.sleep(0.5)
    raise AssertionError("no {} within {} s; last events: {}".format(
        " or ".join(kinds), seconds, timeline(events(stack, conversation))))


def tool_results(stored):
    return [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]


# One shell call inside the guest, then a closing answer.
SMOKE = {"model": "fixture", "responses": [
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1,
                 "arguments": {"command": "printf firecracker > proof.txt && cat proof.txt && echo && uname -r"}}],
     "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
    {"events": [{"type": "text", "text": "Done in a microVM."}], "usage": {"prompt_tokens": 20, "completion_tokens": 10}}]}


def smoke(stack):
    """SBX-F-020: the product runs a task in a microVM, end to end, through the public API."""
    stack.sign_in()
    ready = stack.request("/readyz?detail=1")
    assert ready["ready"] is True and all(ready["checks"].values()), ready
    assert ready["sandbox"]["backend"] == "firecracker", ready
    assert ready["checks"]["backend_preflight"] is True, "the microVM host preflight passed in the gateway"
    assert ready["checks"]["image_present"] is True, "the runtime template's rootfs is verified"
    conversation = stack.request("/api/v1/conversations", {"title": "Smoke", "model_route": "local"})["id"]
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Prove it", "attachments": []})
    stored = until(stack, conversation, ("run.completed", "run.failed"))
    assert stored[-1]["event"]["type"] == "run.completed", stored[-1]
    results = tool_results(stored)
    assert len(results) == 1 and results[0]["status"] == "ok", results
    lines = results[0]["content"].splitlines()
    assert lines[0].endswith("firecracker"), results[0]["content"]
    host = subprocess.run(["uname", "-r"], capture_output=True, text=True, check=True).stdout.strip()
    assert any(line.strip().startswith("6.18") for line in lines) and host not in lines, \
        "the shell ran in the guest kernel, not the host's ({})".format(host)
    hosts = stack.vm_hosts()
    assert len(hosts) == 1, "exactly one VM host for the conversation: {}".format(hosts)
    backend = compose.command(["docker", "inspect", "-f", '{{index .Config.Labels "tqa.backend"}}', hosts[0]],
                              stack.env, capture=True).stdout.strip()
    assert backend == "firecracker", backend
    assert [frame["seq"] for frame in stored] == list(range(1, len(stored) + 1)), "contiguous"
    print("SBX-F-020: a task ran in a microVM through the public API; the shell saw the guest kernel", flush=True)


# A shell call that grows until the guest's OOM killer ends it, then one that proves the sandbox and the run go on.
OOM = {"model": "fixture", "responses": [
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {
        "command": "python3 -c \"chunks = [b'x' * (64 << 20) for _ in iter(int, 1)]\""}}],
     "usage": {"prompt_tokens": 10, "completion_tokens": 10}},
    {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": "echo alive"}}],
     "usage": {"prompt_tokens": 20, "completion_tokens": 10}},
    {"events": [{"type": "text", "text": "Still here."}], "usage": {"prompt_tokens": 30, "completion_tokens": 10}}]}


def oom(stack):
    """SBX-C-003 end to end: a tool that exhausts memory is killed inside the guest; the VM and the run continue."""
    stack.sign_in()
    conversation = stack.request("/api/v1/conversations", {"title": "Memory", "model_route": "local"})["id"]
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Eat memory", "attachments": []})
    first = until(stack, conversation, ("tool.start",))
    hosts = stack.vm_hosts()
    stored = until(stack, conversation, ("run.completed", "run.failed"), 300)
    order = [frame["event"]["type"] for frame in stored]
    assert order[-1] == "run.completed" and "run.resumed" not in order, order
    results = tool_results(stored)
    assert [result["status"] for result in results] == ["error", "ok"], results
    assert results[1]["content"].strip() == "alive", results
    assert stack.vm_hosts() == hosts and len(hosts) == 1, "the same VM served the whole run"
    assert first, "the first call started"
    print("SBX-C-003: the tool was OOM-killed inside the guest; the VM and the run continued", flush=True)


# The model is never asked: the environment fails before the runtime starts.
START_FAILURE = {"model": "fixture", "responses": [
    {"events": [{"type": "text", "text": "Never reached."}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}}]}


def failure_notice(stack, conversation, seconds=240):
    """The error frame a fresh stream opens with once the task's last message has failed (bounded, reconnecting)."""
    import time
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        request = urllib.request.Request(
            stack.origin + "/api/v1/conversations/{}/events?stream=1&protocol=1".format(conversation),
            headers={"Origin": stack.origin, "Accept": "text/event-stream", "Last-Event-ID": "0"})
        with stack.client.open(request, timeout=10) as response:
            opened = time.monotonic()
            # The opening frames come at once; a notice isn't there yet if the start is still under way.
            while time.monotonic() - opened < 3:
                line = response.readline(1048577)
                if not line:
                    break
                if line.startswith(b"data:"):
                    frame = json.loads(line[5:])
                    if frame.get("type") == "error":
                        return frame
                    if frame.get("type") == "heartbeat":
                        break
        time.sleep(2)
    raise AssertionError("no failure notice within {} s; last events: {}".format(
        seconds, timeline(events(stack, conversation))))


def start_failure(stack):
    """GW-I-141: a task whose workspace disk won't mount. The guest init fails before the runtime (exit 126), and the
    gateway reports the environment's failure (sandbox_failed, retryable), never runtime_start_failed; no run starts,
    and the model is never asked."""
    stack.sign_in()
    conversation = stack.request("/api/v1/conversations", {"title": "Broken disk", "model_route": "local"})["id"]
    workspace = stack.request("/api/v1/conversations/" + conversation)["workspace_id"]
    # A workspace image of zeros, owned by the container account like the gateway's own, before the first sandbox:
    # the backend reuses an existing image, and the guest's ext4 mount finds no superblock.
    compose.command(["docker", "run", "--rm", "--network", "none", "--user", "1000:1000", "--mount",
                     "type=bind,source={},target=/s".format(stack.env["TQA_STORAGE_ROOT"]), compose.TOYBOX, "sh", "-c",
                     "mkdir -p -m 700 /s/workspaces && truncate -s 256M /s/workspaces/{}.ext4".format(workspace)],
                    stack.env)
    stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Start", "attachments": []})
    notice = failure_notice(stack, conversation)
    assert (notice["code"], notice["retryable"]) == ("sandbox_failed", True), notice
    stored = [frame.get("event", {}).get("type") for frame in events(stack, conversation)]
    assert "run.started" not in stored, stored
    print("GW-I-141: a workspace disk that won't mount ended as sandbox_failed (retryable); no run started",
          flush=True)


CASES = {"smoke": (SMOKE, smoke), "oom": (OOM, oom), "start-failure": (START_FAILURE, start_failure)}


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in CASES:
        raise SystemExit("usage: stack.py {" + ",".join(CASES) + "}")
    scenario, check = CASES[sys.argv[1]]
    run(sys.argv[1], scenario, check)


if __name__ == "__main__":
    main()
