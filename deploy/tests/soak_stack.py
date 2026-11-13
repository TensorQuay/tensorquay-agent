"""SOAK-002: bounded HTTP and Docker operations against one temporary installation."""
import http.cookiejar
import json
from pathlib import Path
import os
import re
import secrets
import socket
import subprocess
import threading
import time
import tomllib
import urllib.error
import urllib.request

import compose
from engine_proxy import EngineProxy
from soak_disk import BoundedStorage, engine_child_pid
import soak_mix
from soak_mix_audit import MixProbe
from soak_orphans import parse_rows
import soak_real
import demo
from soak_audit import require
from soak_chaos import (DATABASE_QUERY, PROCESS_HELPER, database_sample, observer_arguments, process_sample,
                        runtime_identity)
from soak_scenario import RESPONSES, scenario


LOG_CAP = 8 * 1024 * 1024
# The harness's own fault commands (kill, pause, network connect and disconnect): a loaded engine took over 15 s to
# kill a container (the candidate 6 rehearsal), which failed the soak for the harness, not the product. The fault's
# own record keeps when it actually happened.
ENGINE_COMMAND_SECONDS = 180
# A harness engine command slower than this is recorded as a measurement (never a failure).
SLOW_ENGINE_SECONDS = 15
# What a retried command's error says when its first, timed-out attempt took effect after all.
ALREADY_DONE = re.compile(r"is not running|is already paused|is not paused|is not connected|already exists in network")
# The resource sample's own commands (a helper container, the database size query): a busy host's engine may take
# long to start a container, and a slow sample is better than a missed one.
RESOURCE_COMMAND_SECONDS = 90
POOL_SANDBOXES = 16  # compose.prepare's /24 pool of /29 networks
# The demo's services, where the mix's lane model takes its definition from (soak_mix.lane_service).
DEMO_COMPOSE = Path(__file__).resolve().parents[1] / "compose" / "demo.yml"


def firecracker_stack():
    """The microVM stack's own helpers (deploy/firecracker/stack.py), loaded only for a microVM soak."""
    import importlib.util
    source = Path(__file__).resolve().parents[1] / "firecracker" / "stack.py"
    spec = importlib.util.spec_from_file_location("firecracker_stack", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def max_live(slots):
    """Live sandboxes the soak's installation admits: every conversation, plus as many again for a retiring
    generation's idle sandbox or a replacement after a kill. With fewer, admission evicts an idle conversation's
    sandbox, which the soak would report as a replaced runtime."""
    require(2 * slots <= POOL_SANDBOXES, "the sandbox network pool holds {} sandboxes".format(POOL_SANDBOXES))
    return 2 * slots


def image_references(env):
    """The images a stack runs: each one's TQA_*_IMAGE (a candidate's tag file names every one by ID, the test model
    included), or the local build's tag when it isn't set."""
    return {"gateway": compose.required_image(env, "TQA_GATEWAY_IMAGE"),
            "runtime": compose.required_image(env, "TQA_RUNTIME_IMAGE"),
            "web": compose.required_image(env, "TQA_WEB_IMAGE"),
            "fake-model": compose.required_image(env, "TQA_FAKE_MODEL_IMAGE")}


def wants_no_swap(env):
    """TQA_SOAK_NO_SWAP=1: the launcher's hard memory cap, off unless set."""
    value = env.get("TQA_SOAK_NO_SWAP")
    require(value in (None, "1"), "TQA_SOAK_NO_SWAP takes 1, or is left unset")
    return value == "1"


def no_swap(overlay, rendered):
    """The soak's overlay with every service's swap held to its memory (`memswap_limit` = `mem_limit`), so no service
    swaps; a service without a memory limit has no hard cap and refuses the run. Sandboxes never swap: the gateway sets
    their swap to their memory."""
    for name, service in sorted(rendered["services"].items()):
        limit = service.get("mem_limit")
        require(bool(limit), "service {} has no mem_limit: no hard memory cap".format(name))
        overlay.setdefault("services", {}).setdefault(name, {})["memswap_limit"] = limit
    return overlay


def pages(fetch, max_pages=100):
    """Read through an empty final page; reject stalled or inconsistent pagination."""
    after = 0
    for _ in range(max_pages):
        page = fetch(after)
        items, following = page["items"], page["next_after"]
        require(isinstance(items, list) and len(items) <= 1000, "invalid storage page")
        if not items:
            require(following is None, "empty page has a cursor")
            return
        require(type(following) is int and following > after and following == items[-1]["seq"],
                "storage pagination did not advance to its last item")
        yield items
        after = following
    require(False, "storage pagination exceeded its page cap")


def cleanup_actions(actions, failure):
    """Try every owner even after partial startup, injection failure or interruption."""
    for action in actions:
        try:
            action()
        except BaseException as error:
            print("Owned cleanup failed: {}".format(type(error).__name__), flush=True)
            if failure is None:
                failure = error
    return failure


# How long a service may list no running container (compose recreating or restarting it) before the soak fails.
SERVICE_SECONDS = 30


def verb(arguments):
    """The engine command's name for the record: `kill`, or `network disconnect`."""
    return " ".join(arguments[1:3]) if arguments[1] == "network" else arguments[1]


def io_full():
    """The host's I/O pressure now ("full" avg10, %), or None where the kernel doesn't report it."""
    try:
        with open("/proc/pressure/io") as pressure:
            line = next(line for line in pressure if line.startswith("full"))
        return float(line.split("avg10=")[1].split()[0])
    except (OSError, StopIteration, IndexError, ValueError):
        return None


class Stack:
    """All identities, mounts and actions derive from this invocation's Compose project."""
    def __init__(self, directory, env, output, backend="docker", model="fake", mix=None):
        self.directory, self.env, self.output, self.backend, self.model_kind = directory, env, output, backend, model
        # Agent teams and Desktop tasks in the mix (soak_mix), off unless asked; the Desktop image under test, pinned.
        self.mix, self.desktop, self.network_pool = mix or soak_mix.MixOptions(), None, None
        self.model_route = None
        self.configuration = demo.COMPOSE + ["-f", str(directory.parent / "soak.json")]
        if backend == "firecracker":
            microvm = firecracker_stack()
            # First, so that tearing down after any failure can still interpolate the overlay.
            # The real network is drawn in to_microvm, once the engine is asked (firecracker/stack.py claim_network).
            self.env.update(microvm.UNDRAWN)
            self.configuration = demo.COMPOSE + microvm.OVERLAY + ["-f", str(directory.parent / "soak.json")]
        self.jar = http.cookiejar.CookieJar()
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))
        # The harness's session: the gateway ends it session_ttl after sign-in (24 h by default) however much it is
        # used, and the soak runs longer, so a 401 signs in again (sign_in, renew). The streams read `cookie` anew.
        self.account, self.cookie, self.sign_ins = None, None, 0
        self.session_lock = threading.Lock()
        # Its known end: sign-in time plus the stack's session_ttl (read in prepare). A 401 renews only past it; an
        # earlier one is the product's failure. `on_renewal` gets each renewal's age for the journal.
        self.session_ttl, self.signed_in_at, self.on_renewal = SESSION_TTL_DEFAULT, None, None
        # TQA_SOAK_SESSION_TTL shortened it (the renewal rehearsal): the run is then never gate evidence.
        self.session_override = False
        self.images, self.runtimes, self.paused, self.cuts = {}, {}, {}, {}
        self.database_stopped = False
        # The harness's slow engine commands (measurements); the driver logs each as it happens.
        self.slow_commands, self.on_slow = [], None
        self.ready_wait_seconds = None  # wait_active's bound, read in prepare from the stack's gateway.toml

    def command(self, arguments, timeout=90):
        return compose.command(arguments, self.env, capture=True, timeout=timeout).stdout.strip()

    def engine(self, arguments, timeout=ENGINE_COMMAND_SECONDS):
        """The harness's own engine command (kill, stop, start, pause, inspect, network), on an engine that may be
        saturated: bounded, retried once after a timeout, and recorded when slow. Only both tries timing out fails; a
        retry whose error says the first try took effect after all counts as done."""
        started, output = time.monotonic(), ""
        for attempt in (1, 2):
            try:
                output = self.command(arguments, timeout=timeout)
                break
            except subprocess.TimeoutExpired:
                if attempt == 2:
                    self.slow(arguments, time.monotonic() - started, attempt)
                    require(False, "the engine didn't finish `docker {}` in two tries of {} s each (io full {} %)"
                            .format(verb(arguments), timeout, io_full()))
            except subprocess.CalledProcessError as error:
                if attempt == 2 and ALREADY_DONE.search(error.stderr or ""):
                    break
                raise
        seconds = time.monotonic() - started
        if seconds > SLOW_ENGINE_SECONDS or attempt > 1:
            self.slow(arguments, seconds, attempt)
        return output

    def slow(self, arguments, seconds, attempts):
        """Records a slow harness engine command as a measurement: the command, its seconds, the I/O pressure then."""
        measurement = {"command": verb(arguments), "seconds": round(seconds, 1), "attempts": attempts,
                       "io_full": io_full()}
        self.slow_commands.append(measurement)
        if self.on_slow is not None:
            self.on_slow(measurement)

    def start_engine_proxy(self):
        """The gateway reaches the engine through the stall proxy, from a directory only this user can enter."""
        upstream = self.env.get("TQA_DOCKER_SOCKET")
        require(bool(upstream), "TQA_DOCKER_SOCKET must name the engine socket for the stall proxy")
        private = self.directory.parent / "engine"
        private.mkdir(mode=0o700)
        private.chmod(0o700)
        self.proxy = EngineProxy(private / "proxy.sock", upstream)
        self.proxy.start()
        self.env["TQA_DOCKER_SOCKET"] = str(private / "proxy.sock")

    def stop_engine_proxy(self):
        if getattr(self, "proxy", None) is not None:
            self.proxy.stop()

    def bound_storage(self, mib):
        """The storage root on a `mib` MiB tmpfs in the engine's namespace, before anything starts."""
        runtime_dir = os.environ.get("XDG_RUNTIME_DIR") or "/run/user/{}".format(os.getuid())
        self.storage = BoundedStorage(self.env["TQA_STORAGE_ROOT"], mib, engine_child_pid(runtime_dir),
                                      self.images["runtime"], lambda arguments: self.command(arguments, timeout=120))
        self.storage.mount()

    def unbound_storage(self):
        if getattr(self, "storage", None) is not None:
            self.storage.unmount()

    def prepare(self, rehearsal, slots=4, engine_proxy=False, storage_mib=None):
        compose.preflight(self.env)
        self.images = {name: demo.runtime_image(self.env, reference)
                       for name, reference in image_references(self.env).items()}
        self.env.update(TQA_GATEWAY_IMAGE=self.images["gateway"], TQA_WEB_IMAGE=self.images["web"])
        self.desktop = soak_mix.desktop_image(self.env, self.mix.desktops, self.image_record)
        if self.desktop is not None:
            self.images["desktop"] = self.desktop["id"]
        if engine_proxy:
            self.start_engine_proxy()
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=self.images["runtime"], port=port,
                            max_live=soak_mix.max_live(slots, self.mix.teams, self.mix.desktops),
                            **({"pool_prefix": soak_mix.pool_prefix(self.mix)} if self.mix.on else {}))
        self.origin = "http://127.0.0.1:" + str(port)
        self.env["TQA_DEMO_HOST"] = "127.0.0.1"
        compose.ownership(self.directory, self.env)
        self.configure(rehearsal)
        if self.backend == "firecracker":
            self.to_microvm()
        # Read now: once the folder is the gateway's (1000:1000), this user can't read gateway.toml (soak 5, 8 Oct).
        gateway_toml = (self.directory / "gateway.toml").read_text()
        self.ready_wait_seconds = ready_wait(gateway_toml)
        self.session_ttl = session_ttl(gateway_toml)
        if self.desktop is not None:
            soak_mix.check_pinned(gateway_toml, self.desktop["id"])
        self.network_pool = soak_mix.network_pool(gateway_toml) if self.mix.on else None
        compose.ownership(self.directory, self.env, "1000:1000")
        if storage_mib:
            self.bound_storage(storage_mib)
        if wants_no_swap(self.env):
            overlay = self.directory.parent / "soak.json"
            rendered = json.loads(self.command(self.configuration + ["config", "--format", "json"]))
            compose.write(overlay, json.dumps(no_swap(json.loads(overlay.read_text()), rendered)))
        wait = "120" if self.backend == "firecracker" else "60"
        self.start_services(wait)
        self.gateway = self.service("gateway")
        self.database = self.service("postgres")
        self.model = self.service("fake-model")
        self.mix_model = self.service(soak_mix.MIX_SERVICE) if self.mix.on else None
        self.wait_ready()
        password = "synthetic-soak-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "soak@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.account = account
        self.sign_in()

    def start_services(self, wait):
        """`compose up`; when it fails, every service's log is kept in the evidence first (startup.log), since a
        service that exits at start goes with the installation, and its reason with it."""
        try:
            self.command(self.configuration + ["up", "-d", "--no-build", "--wait", "--wait-timeout", wait], timeout=180)
        except (OSError, RuntimeError, subprocess.SubprocessError):
            try:
                logs = self.command(self.configuration + ["logs", "--no-color", "--timestamps"], timeout=60)
                compose.write(self.output / "startup.log", compose.redact(logs, self.env)[-LOG_CAP:])
            except (OSError, RuntimeError, subprocess.SubprocessError):
                print("Could not keep the services' logs after the failed start", flush=True)
            raise

    def sign_in(self):
        """Signs in as the run's account; the jar keeps the new session, and `cookie` is its header for the streams. Its
        start is taken before the call, so the known end is never later than the gateway's."""
        started = time.time()
        self.request("/api/v1/auth/login", self.account)
        self.signed_in_at = started
        self.cookie = "; ".join(cookie.name + "=" + cookie.value for cookie in self.jar)
        self.sign_ins += 1

    def renew(self, stale):
        """After a 401: signs in again unless another caller already replaced the `stale` cookie; returns the current
        one. Only past the session's known end, less the margin: a session refused earlier fails the run."""
        with self.session_lock:
            if self.cookie == stale:
                age = time.time() - self.signed_in_at
                require(age >= self.session_ttl - session_margin(self.session_ttl),
                        "session refused before its end: {:.0f} s after sign-in, of {} s".format(age, self.session_ttl))
                self.sign_in()
                if self.on_renewal is not None:
                    self.on_renewal({"age_seconds": round(age, 1), "session_ttl_seconds": self.session_ttl})
            return self.cookie

    def evidence(self, rehearsal):
        """Whether the run is gate evidence: never a rehearsal, never with the session's lifetime shortened."""
        return not rehearsal and not self.session_override

    def to_microvm(self):
        """The installation on the microVM backend: the pinned VM host, the kernel and the runtime's rootfs."""
        microvm = firecracker_stack()
        vmm_image = microvm.image_id(self.env["TQA_VMM_IMAGE"], self.env)
        self.images["vmm"] = vmm_image
        config = self.directory / "gateway.toml"
        gateway = microvm.claim_network(self.env)
        compose.write(config, microvm.firecracker_config(config.read_text(), vmm_image, gateway))
        microvm.artefacts(self.directory / "storage", self.env, self.images["runtime"], vmm_image)

    def configure(self, rehearsal):
        # The renewal rehearsal (SOAK-001): a shortened session, as gateway.toml's first line,
        # since a top-level key after a table header would belong to that table. Checked before anything is written.
        if "TQA_SOAK_SESSION_TTL" in self.env:
            ttl = self.env["TQA_SOAK_SESSION_TTL"]
            require(ttl.isdigit() and int(ttl) >= 60, "TQA_SOAK_SESSION_TTL must be whole seconds, at least 60")
            gateway = self.directory / "gateway.toml"
            compose.write(gateway, "session_ttl={}\n".format(int(ttl)) + gateway.read_text())
            self.session_override = True
        compose.write(self.directory / "scenario.json", json.dumps(scenario(rehearsal)))
        if self.mix.teams:
            gateway = self.directory / "gateway.toml"
            compose.write(gateway, soak_mix.gateway_text(gateway.read_text(), teams=True))
        compose.write(self.directory.parent / "soak.json", json.dumps({"services": {"fake-model": {
            "image": self.images["fake-model"], "command": ["--scenario", "/scenarios/soak.json"],
            "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                         "target": "/scenarios/soak.json", "read_only": True}]}}}))
        config = self.directory / "gateway.toml"
        models = self.directory / "models.toml"
        if self.model_kind == "real":
            require(self.env.get("TQA_SOAK_MODEL_URL") and self.env.get("TQA_SOAK_MODEL_NAME"),
                    "the real-model soak needs TQA_SOAK_MODEL_URL and TQA_SOAK_MODEL_NAME")
            self.model_route = {"url": self.env["TQA_SOAK_MODEL_URL"], "model": self.env["TQA_SOAK_MODEL_NAME"],
                                "context_window": int(self.env.get("TQA_SOAK_MODEL_CONTEXT", "32768"))}
            compose.write(models, soak_real.route(self.model_route["url"], self.model_route["model"],
                                                  self.model_route["context_window"]))
        else:
            compose.write(models, models.read_text() + "context_window=1000000\nstream_usage=true\n")
        if self.mix.on:
            self.configure_mix(models)

    def configure_mix(self, models):
        """The lanes' own scripted model (soak_mix.lane_service) beside the slots', and its route."""
        source = self.directory / "mix.json"
        compose.write(source, json.dumps(soak_mix.lane_scenario(self.mix.teams, self.mix.desktops)))
        overlay = self.directory.parent / "soak.json"
        services = json.loads(overlay.read_text())
        services["services"][soak_mix.MIX_SERVICE] = soak_mix.lane_service(str(source), self.images["fake-model"],
                                                                           str(DEMO_COMPOSE))
        compose.write(overlay, json.dumps(services))
        compose.write(models, soak_mix.models_text(models.read_text()))

    def request(self, path, body=None):
        """One API call. A 401 means the session ended (it lasts session_ttl from sign-in, not from use): it signs in
        again and retries once; a 401 after that, or on the sign-in calls themselves, stands."""
        stale = self.cookie
        try:
            return self.call(path, body)
        except urllib.error.HTTPError as error:
            if error.code != 401 or path.startswith("/api/v1/auth/") or self.account is None:
                raise
        self.renew(stale)
        return self.call(path, body)

    def delete(self, path):
        """A DELETE, answered 204; a 401 signs in again as `request` does."""
        stale = self.cookie
        try:
            return self.call_delete(path)
        except urllib.error.HTTPError as error:
            if error.code != 401 or self.account is None:
                raise
        self.renew(stale)
        return self.call_delete(path)

    def call_delete(self, path):
        request = urllib.request.Request(self.origin + path, method="DELETE", headers={
            "Origin": self.origin, "Idempotency-Key": secrets.token_hex(16)})
        with self.client.open(request, timeout=15) as response:
            require(response.status == 204, "DELETE {} answered {}".format(path.split("?")[0], response.status))

    def image_record(self, reference):
        return json.loads(self.command(["docker", "image", "inspect", reference]))[0]

    def mix_probe(self, helpers):
        """The mix audit's listings now (soak_mix_audit), with the helper tasks the team lane saw."""
        return MixProbe(self, self.desktop["id"] if self.desktop else None, compose.TOYBOX)(helpers)

    def call(self, path, body=None):
        headers = {"Origin": self.origin, "Content-Type": "application/json"}
        if body is not None:
            headers["Idempotency-Key"] = secrets.token_hex(16)
        request = urllib.request.Request(self.origin + path, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        with self.client.open(request, timeout=15) as response:
            data = response.read(1048577)
            require(len(data) <= 1048576, "HTTP response exceeded its cap")
            return json.loads(data)

    def stored(self, conversation):
        path = "/api/v1/conversations/{}/events?limit=1000&after=".format(conversation)
        return pages(lambda after: self.request(path + str(after)))

    def wait_ready(self):
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(self.origin + "/readyz", timeout=3) as response:
                    if response.status == 200:
                        return
            except (urllib.error.URLError, OSError):
                pass
            time.sleep(0.25)
        raise TimeoutError("owned gateway did not become ready")

    def service(self, name):
        """The service's running container. Right after a restart, `compose ps` can briefly list none; it is asked
        again for up to SERVICE_SECONDS (never with -a: a stopped container must not pass for a running one)."""
        deadline = time.monotonic() + SERVICE_SECONDS
        while True:
            identity = self.command(self.configuration + ["ps", "-q", name])
            if re.fullmatch(r"[0-9a-f]{12,64}", identity) or time.monotonic() >= deadline:
                break
            time.sleep(1)
        require(re.fullmatch(r"[0-9a-f]{12,64}", identity) is not None,
                "the {} service has no running container after {} s".format(name, SERVICE_SECONDS))
        record = self.inspect(identity)
        require(record["Config"]["Labels"].get("com.docker.compose.project") == self.env["COMPOSE_PROJECT_NAME"],
                "service does not belong to the soak")
        return record["Id"]

    def inspect(self, identity):
        require(re.fullmatch(r"[0-9a-f]{12,64}", identity), "invalid container identity")
        return json.loads(self.engine(["docker", "inspect", identity]))[0]

    def sandbox(self, conversation):
        identity = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                                 "--filter", "label=tqa.conversation=" + conversation])
        record = self.inspect(identity)
        require(record["State"]["Running"], "runtime is no longer running")
        prior = self.runtimes.setdefault(conversation, record["Id"])
        require(record["Id"] == prior, "a healthy runtime was replaced")
        return record

    def pause(self, conversation):
        record = self.sandbox(conversation)
        require(conversation not in self.paused and not record["State"]["Paused"], "runtime already paused")
        identity = runtime_identity(record, self.env["TQA_SANDBOX_NETWORK"])
        self.paused[conversation] = identity
        self.engine(["docker", "pause", record["Id"]])
        require(self.inspect(record["Id"])["State"]["Paused"], "runtime was not paused")
        return identity

    def unpause(self, conversation):
        identity = self.paused[conversation]
        self.engine(["docker", "unpause", identity["container"]])
        record = self.sandbox(conversation)
        require(not record["State"]["Paused"] and runtime_identity(record, self.env["TQA_SANDBOX_NETWORK"]) == identity,
                "unpause changed runtime identity, address or credential binding")
        del self.paused[conversation]

    def kill_gateway(self):
        """SIGKILL, no drain, then the same container starts again."""
        self.engine(["docker", "kill", self.gateway])
        self.engine(["docker", "start", self.gateway])
        self.wait_ready()
        require(self.service("gateway") == self.gateway, "the kill replaced the gateway container")

    def database_ready(self):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            try:
                self.command(["docker", "exec", self.database, "pg_isready", "-U", "tqa", "-d", "gateway"], timeout=10)
                return
            except (OSError, subprocess.SubprocessError, RuntimeError):
                time.sleep(1)
        raise TimeoutError("the database did not come back")

    def database_fault(self, kind):
        """db_restart: a clean restart; db_crash: SIGKILL, then a start that runs crash recovery."""
        if kind == "db_restart":
            self.engine(["docker", "restart", "--time", "10", self.database])
        else:
            self.engine(["docker", "kill", self.database])
            self.engine(["docker", "start", self.database])
        self.database_ready()

    def stop_database(self):
        self.database_stopped = True
        self.engine(["docker", "stop", "--time", "10", self.database])

    def start_database(self):
        self.engine(["docker", "start", self.database])
        self.database_ready()
        self.database_stopped = False

    def kill_sandbox(self, conversation):
        """SIGKILL of the conversation's runtime; its replacement becomes the one `sandbox()` expects."""
        record = self.sandbox(conversation)
        self.engine(["docker", "kill", record["Id"]])
        self.runtimes.pop(conversation, None)
        return record["Id"]

    def replacement(self, conversation, killed):
        """The running runtime that replaced `killed`, or None yet."""
        identity = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                                 "--filter", "label=tqa.conversation=" + conversation])
        if not identity:
            return None
        record = self.inspect(identity)
        if record["Id"] == killed or not record["State"]["Running"]:
            return None
        self.runtimes[conversation] = record["Id"]
        return record["Id"]

    def cut_link(self, conversation):
        """Disconnect the runtime from its networks; the process keeps running."""
        record = self.sandbox(conversation)
        networks = {name: settings.get("IPAddress", "") for name, settings in record["NetworkSettings"]["Networks"].items()}
        cut = {"container": record["Id"], "networks": networks}
        self.cuts[conversation] = cut
        for name in networks:
            self.engine(["docker", "network", "disconnect", name, record["Id"]])
        return cut

    def restore_link(self, conversation):
        """Reconnect with the same addresses where the network allows fixed addresses."""
        cut = self.cuts.pop(conversation)
        for name, address in cut["networks"].items():
            try:
                self.command(["docker", "network", "connect", "--ip", address, name, cut["container"]],
                             timeout=ENGINE_COMMAND_SECONDS)
            except (OSError, subprocess.SubprocessError, RuntimeError):
                self.engine(["docker", "network", "connect", name, cut["container"]])
        record = self.inspect(cut["container"])
        return {name: settings.get("IPAddress", "") for name, settings in record["NetworkSettings"]["Networks"].items()}

    def inventory(self):
        """What this installation holds now: sandbox containers, admission rows, per-sandbox networks, volumes."""
        label = "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"]
        containers = []
        for identity in self.command(["docker", "ps", "-aq", "--filter", label]).split():
            record = self.inspect(identity)
            containers.append({"id": record["Id"][:12], "state": record["State"]["Status"],
                               "conversation": record["Config"]["Labels"].get("tqa.conversation", ""),
                               "networks": sorted(record["NetworkSettings"]["Networks"])})
        rows = parse_rows(self.command(["docker", "exec", self.database, "psql", "-U", "tqa", "-d", "gateway", "-At",
                                        "-c", "select conversation_id, phase from sandbox_instances"], timeout=15))
        networks = []
        for identity in self.command(["docker", "network", "ls", "-q", "--filter", label, "--filter",
                                      "label=tqa.subnet"]).split():
            info = json.loads(self.command(["docker", "network", "inspect", identity]))[0]
            networks.append({"name": info["Name"], "attached": len(info.get("Containers") or {})})
        volumes = [{"name": name, "in_use": bool(self.command(["docker", "ps", "-aq", "--filter", "volume=" + name]))}
                   for name in self.command(["docker", "volume", "ls", "-q", "--filter", label]).split()]
        return containers, rows, networks, volumes

    def cleanup_faults(self):
        """Reconnect cut links and start a stopped database, so teardown finds a whole stack."""
        actions = [lambda conversation=conversation: self.restore_link(conversation) for conversation in list(self.cuts)]
        if getattr(self, "database_stopped", False):
            actions.append(self.start_database)
        if getattr(self, "storage", None) is not None and self.storage.filled:
            actions.append(self.storage.free)
        failure = cleanup_actions(actions, None)
        if failure is not None:
            raise failure

    def cleanup_pauses(self):
        actions = [lambda conversation=conversation: self.unpause(conversation) for conversation in list(self.paused)]
        failure = cleanup_actions(actions, None)
        if failure is not None:
            raise failure

    def helper(self, arguments):
        # Labels make even a timed-out docker-run process reclaimable by this invocation.
        arguments[2:2] = ["--label", "tqa.soak.helper=" + self.env["COMPOSE_PROJECT_NAME"]]
        return self.command(arguments, timeout=RESOURCE_COMMAND_SECONDS)

    def cleanup_helpers(self):
        if "COMPOSE_PROJECT_NAME" in self.env:
            identities = self.command(["docker", "ps", "-aq", "--filter",
                                       "label=tqa.soak.helper=" + self.env["COMPOSE_PROJECT_NAME"]]).split()
            require(all(re.fullmatch(r"[0-9a-f]{12,64}", identity) for identity in identities), "invalid helper IDs")
            if identities:
                self.command(["docker", "rm", "--force"] + identities)

    def restart(self):
        self.engine(["docker", "restart", "--time", "10", self.gateway])
        self.wait_ready()
        require(self.service("gateway") == self.gateway, "restart replaced the gateway container")

    def resources(self):
        arguments = observer_arguments(self.images["runtime"], self.gateway)
        process = process_sample(self.helper(arguments + ["-c", PROCESS_HELPER]))
        database = database_sample(self.command(["docker", "exec", self.database, "psql", "-U", "tqa", "-d", "gateway",
                                                 "-At", "-c", DATABASE_QUERY], timeout=RESOURCE_COMMAND_SECONDS))
        return {"at": time.time(), **process, **database}

    def logs(self, identity, since="0"):
        result = compose.command(["docker", "logs", "--since", str(since), "--tail", "10000", identity],
                                 self.env, capture=True, timeout=15)
        text = result.stdout + result.stderr
        require(len(text.encode()) <= 8 * 1024 * 1024, "container log exceeded its cap")
        return compose.redact(text, self.env)

    def window_logs(self, identity, since, until):
        """A container's logs between two Unix times, cut at 8 MiB with a marker instead of failing the run."""
        result = compose.command(["docker", "logs", "--since", "{:.0f}".format(since), "--until",
                                  "{:.0f}".format(until), identity], self.env, capture=True, timeout=60)
        data = (result.stdout + result.stderr).encode()
        text = data[:LOG_CAP].decode(errors="ignore") + ("\n[cut at 8 MiB]\n" if len(data) > LOG_CAP else "")
        return compose.redact(text, self.env)

    def exists(self, identity):
        try:
            self.command(["docker", "inspect", "--format", "{{.Id}}", identity], timeout=15)
            return True
        except (OSError, subprocess.SubprocessError):
            return False

    def save_logs(self):
        """The logs of every container still there; a runtime already removed (rotated, evicted or killed) is skipped,
        since its logs went with it and that is not a cleanup failure."""
        identities = {"gateway": getattr(self, "gateway", None), "fake-model": getattr(self, "model", None),
                      soak_mix.MIX_SERVICE: getattr(self, "mix_model", None)}
        identities.update({"runtime-" + key: value for key, value in self.runtimes.items()})
        actions = [lambda name=name, identity=identity: compose.write(self.output / (name + ".log"), self.logs(identity))
                   for name, identity in identities.items() if identity is not None and self.exists(identity)]
        failure = cleanup_actions(actions, None)
        if failure is not None:
            raise failure

    def verify_model(self):
        records = [json.loads(line) for line in self.logs(self.model).splitlines() if line.startswith("{")]
        turns = [record["turn"] for record in records if record.get("turn") is not None]
        require(bool(turns) and max(turns) < RESPONSES, "model script exhausted or never used")
        return {"requests": len(turns), "highest_response_index": max(turns)}


# A new conversation is ready once its sandbox has started, its runtime registered and both clients have their first
# `session.ready`. The gateway may take its own bounds for that (gateway.toml [sandbox]): the container create with its
# retries on a busy engine, `create_budget_seconds` (each create bounded at 60 s and retried within the budget), then
# the runtime's registration, `ready_seconds`. The margin covers the first message and the clients' connects. Soak 4
# (7 Oct 2026) failed on a fixed 60 s, which gave up before the gateway's own retry of a create slowed by host IO.
READY_MARGIN_SECONDS = 30
# The gateway's own default session_ttl (config.rs): a session ends this long after sign-in, however much it is used.
SESSION_TTL_DEFAULT = 86400


def session_ttl(config_text):
    """How long the stack's sessions last: gateway.toml's top-level session_ttl, else the gateway's default; refused
    where the gateway would refuse it (an integer of at least 60 s)."""
    value = tomllib.loads(config_text).get("session_ttl", SESSION_TTL_DEFAULT)
    require(isinstance(value, int) and not isinstance(value, bool) and value >= 60,
            "gateway.toml session_ttl must be an integer of at least 60 seconds")
    return value


def session_margin(ttl):
    """How early before its known end a 401 still counts as the session's end: a minute, or a tenth of a short one."""
    return min(60, ttl // 10)


def ready_wait(config_text):
    """How long `wait_active` waits for new conversations and their clients: the stack's own start bounds, read from
    the gateway.toml it runs with, plus the margin."""
    sandbox = tomllib.loads(config_text).get("sandbox", {})
    for key in ("create_budget_seconds", "ready_seconds"):
        value = sandbox.get(key)
        require(isinstance(value, int) and not isinstance(value, bool) and value > 0,
                "gateway.toml [sandbox] {} must be set: the soak's readiness wait is read from it".format(key))
    return sandbox["create_budget_seconds"] + sandbox["ready_seconds"] + READY_MARGIN_SECONDS
