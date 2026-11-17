"""The one-command install (deploy/install.sh): asks three things, writes what the install guide's steps make by hand
(deploy/README.md, steps 0 to 3), starts the stack and creates the first administrator. Python 3 standard library only;
deploy/install.sh checks the Docker versions and hands over. INST-U-001 tests it without Docker.

Exit status: 0 ready; 2 a refused answer or an existing installation (nothing written); 3 a step failed (files written
so far stay); 4 ready, but the first administrator couldn't be created."""
import argparse
import getpass
import io as text_io
import ipaddress
import json
import os
from pathlib import Path
import re
import shlex
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
import install_files  # noqa: E402
import shell  # noqa: E402

REPOSITORY = Path(__file__).resolve().parents[2]
TOYBOX = "tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7"
DEFAULT_PORT = 8080
DEFAULT_CONTEXT = 32768
READY_SECONDS = 180
PROBE_SECONDS = 60
FAKE_IMAGE = "tqa-fake-model:local"
DEFAULT_POOL = ipaddress.ip_network("10.213.0.0/16")
ADDRESS_HELP = ("Use http://localhost:8080 (this computer; any free port from 1024) or the https:// address your TLS "
                "proxy serves, such as https://agent.example.org.")
UPGRADE = ("The installer doesn't upgrade an installation: an upgrade moves the database and the images at once, so "
           "follow the install guide's Upgrading steps, which take a backup first.")
STRICT_FIX = ("  With strict tools the server dropped the line break from a tool's text. Fix: in Settings › Models, "
              "set Strict tool schemas to Never on the route local, then check again.")


class Refused(Exception):
    """An answer the installer can't take; the message says why and what would do."""


class StepFailed(Exception):
    """A step after the questions failed; the message names it."""


class Address:
    def __init__(self, origin, port, https):
        self.origin, self.port, self.https = origin, port, https


# ---- The questions' checks -------------------------------------------------------------------------------------------

def check_address(text, port_free, port=None):
    """Where people open it: http://localhost:PORT, or an https:// origin served from a local port behind a proxy."""
    text = text.strip() or "http://localhost:{}".format(port or DEFAULT_PORT)
    if "://" not in text:
        raise Refused("That isn't an address. " + ADDRESS_HELP)
    parts = urllib.parse.urlsplit(text)
    try:
        url_port = parts.port
    except ValueError:
        raise Refused("That port isn't a port number. " + ADDRESS_HELP) from None
    if (parts.scheme not in ("http", "https") or not parts.hostname or parts.username or parts.password
            or parts.path not in ("", "/") or parts.query or parts.fragment):
        raise Refused("Give the address alone, with no path. " + ADDRESS_HELP)
    if parts.scheme == "http":
        if parts.hostname != "localhost":
            raise Refused("An http:// address is for this computer only. " + ADDRESS_HELP)
        if url_port is None or not 1024 <= url_port <= 65535:
            raise Refused("Give http://localhost a port from 1024 to 65535. " + ADDRESS_HELP)
        if port is not None and port != url_port:
            raise Refused("--port {} isn't the address's port {}. ".format(port, url_port) + ADDRESS_HELP)
        local, origin = url_port, "http://localhost:{}".format(url_port)
    else:
        local = port or DEFAULT_PORT
        if not 1024 <= local <= 65535:
            raise Refused("--port must be from 1024 to 65535.")
        origin = "https://" + parts.hostname.lower() + (":{}".format(url_port) if url_port else "")
    if not port_free(local):
        raise Refused("Port {} is already in use on this computer. Choose another (with --port for an https:// "
                      "address).".format(local))
    return Address(origin, local, parts.scheme == "https")


def check_model_url(text):
    text = text.strip()
    parts = urllib.parse.urlsplit(text)
    if parts.scheme not in ("http", "https") or not parts.hostname or re.search(r"\s", text):
        raise Refused("Give the server's chat-completions address with http:// or https://, such as "
                      "https://models.example/v1 or http://host.docker.internal:8000/v1.")
    return text


def is_local(url):
    """A server on this computer or the owner's network, as the gateway tells it (a local route's check)."""
    host = (urllib.parse.urlsplit(url).hostname or "").rstrip(".").lower()
    if host in ("localhost", "host.docker.internal") or host.endswith((".localhost", ".local", ".internal")):
        return True
    try:
        return not ipaddress.ip_address(host).is_global
    except ValueError:
        return "." not in host


def check_model_name(text):
    text = text.strip()
    if not text or "\n" in text or "\r" in text or len(text) > 200:
        raise Refused("Give the model's name as the server knows it, on one line.")
    return text


def check_context(text):
    text = text.strip()
    if not text:
        return DEFAULT_CONTEXT
    if not text.isdigit() or not 1024 <= int(text) <= 10_000_000:
        raise Refused("Give the model's context in tokens as the server sets it, such as 32768.")
    return int(text)


def check_key_file(text, repository):
    path = Path(text.strip()).expanduser().resolve()
    if path.is_relative_to(Path(repository).resolve()):
        raise Refused("Keep the key file outside this checkout.")
    if not path.is_file():
        raise Refused("{} doesn't exist.".format(path))
    info = path.stat()
    if info.st_mode & 0o777 != 0o600:
        raise Refused("{} must be private: chmod 600 {}".format(path, shlex.quote(str(path))))
    if info.st_size > install_files.KEY_FILE_BYTES:
        raise Refused("{} is larger than 8192 bytes; it holds the key alone.".format(path))
    if not path.read_text().rstrip("\r\n"):
        raise Refused("{} is empty.".format(path))
    return path


def check_email(text):
    text = text.strip()
    if not re.fullmatch(r"[^@\s]+@[^@\s.]+(\.[^@\s.]+)+", text):
        raise Refused("Give an email address, such as ana@example.org.")
    return text


def check_password(text):
    if len(text) < 12:
        raise Refused("The password needs 12 characters or more.")
    return text


# ---- The command line ------------------------------------------------------------------------------------------------

def parse(argv):
    parser = argparse.ArgumentParser(
        prog="deploy/install.sh", allow_abbrev=False,
        description="Install TensorQuay Agent on this computer from a release checkout: it asks where people will "
                    "open it, which model server to use (or none, for a first try) and who the first administrator "
                    "is, then pulls the release's images, starts it and creates that administrator.")
    add = parser.add_argument
    add("--yes", action="store_true", help="take every default and ask nothing; the administrator must be given")
    add("--address", help="where people open it: http://localhost:PORT or an https:// address behind your proxy")
    add("--model-url", help="the model server's chat-completions address")
    add("--model", help="the model's name on that server")
    add("--key-file", help="a mode 0600 file holding the server's key")
    add("--context", help="the model's context in tokens (a local server; default 32768)")
    add("--fake-model", action="store_true", help="the scripted test model, built here, instead of a model server")
    add("--admin-email", help="the first administrator's email")
    add("--admin-password-file", help="a file holding the first administrator's password (never the password itself)")
    add("--workstation", action="store_true", help="also pull the Workstation environment")
    add("--desktop", action="store_true", help="also pull the Desktop environment")
    add("--install-dir", help="where the installation goes (default ~/.local/share/tensorquay)")
    add("--name", help="a second installation's own name (its Compose project)")
    add("--port", type=int, help="the local port the gateway listens on")
    add("--network-pool", help="a second installation's sandbox network pool, such as 10.214.0.0/16")
    add("--dry-run", action="store_true", help="ask and check, then say what it would write and run")
    add("--upgrade", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def default_install_dir():
    data = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(data) / "tensorquay"


# ---- The host --------------------------------------------------------------------------------------------------------

class TerminalIO:
    def ask(self, prompt):
        try:
            return input(prompt + " ")
        except EOFError:
            raise Refused("No answer was given.") from None

    def secret(self, prompt):
        return getpass.getpass(prompt + " ")

    def say(self, line):
        print(line, flush=True)


class HostSystem:
    """The real commands and HTTP calls, from the repository root; the HTTP session keeps its cookie."""

    def __init__(self, repository):
        self.repository = repository
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())

    def run(self, args, *, env=None, capture=False, timeout=None):
        try:
            done = subprocess.run(args, cwd=self.repository, env=env, capture_output=True, text=True,
                                  timeout=timeout or 120)
        except subprocess.TimeoutExpired as error:
            raise shell.CommandFailed(args, -1, "timed out after {} s".format(error.timeout)) from None
        except OSError as error:
            raise shell.CommandFailed(args, -1, str(error)) from None
        if done.returncode != 0:
            raise shell.CommandFailed(args, done.returncode, done.stderr)
        return done.stdout

    def http(self, method, url, *, body=None, headers=None, timeout=None):
        request = urllib.request.Request(url, data=None if body is None else body.encode(), method=method,
                                         headers=dict(headers or {}))
        try:
            with self.opener.open(request, timeout=timeout or 10) as answer:
                status, data = answer.status, answer.read()
        except urllib.error.HTTPError as error:
            status, data = error.code, error.read()
        try:
            return status, json.loads(data) if data else None
        except ValueError:
            return status, None

    def port_free(self, port):
        with socket.socket() as probe:
            try:
                probe.bind(("127.0.0.1", port))
            except OSError:
                return False
        return True

    def sleep(self, seconds):
        time.sleep(seconds)


# ---- The install -----------------------------------------------------------------------------------------------------

class Installer:
    def __init__(self, args, io, system, repository):
        self.args, self.io, self.system, self.repository = args, io, system, Path(repository)
        self.install_dir = Path(args.install_dir or default_install_dir()).expanduser().absolute()
        self.compose_env = self.install_dir / "compose.env"
        self.secrets_env = {}
        self.extra = ()

    # One answer: from its flag (checked once), or asked until it passes; --yes takes the default.
    def answer(self, given, prompt, check, default=""):
        if given is not None:
            return check(given)
        if self.args.yes:
            return check(default)
        while True:
            try:
                return check(self.io.ask(prompt))
            except Refused as refused:
                self.io.say(str(refused))

    def secret_answer(self, prompt, check):
        while True:
            try:
                return check(self.io.secret(prompt))
            except Refused as refused:
                self.io.say(str(refused))

    def compose(self, *more):
        return ["docker", "compose", "--env-file", str(self.compose_env), "-f", "deploy/compose/docker.yml", *more]

    def refuse_existing(self):
        if self.compose_env.exists():
            port = DEFAULT_PORT
            match = re.search(r"^TQA_HTTP_PORT=(\d+)$", self.compose_env.read_text(), re.M)
            if match:
                port = int(match[1])
            raise Refused(
                "There is already an installation in {dir}, at http://localhost:{port}. The installer never changes "
                "one. To upgrade it, follow the install guide's Upgrading steps: deploy/pull-images.sh for the new "
                "release, then its up line with the new images. To remove the stack and keep its files: {down}".format(
                    dir=self.install_dir, port=port, down=" ".join(self.compose("down"))))
        if self.install_dir.exists():
            empty = not any(self.install_dir.iterdir())
            raise Refused("{} is already there{}. The installer makes its own folder: choose another with "
                          "--install-dir.".format(self.install_dir, " (empty)" if empty else " and isn't empty"))

    def gather(self):
        args = self.args
        if self.install_dir.is_relative_to(self.repository.resolve()):
            raise Refused("Keep the installation outside this checkout (--install-dir).")
        if args.yes and not (args.admin_email and args.admin_password_file):
            raise Refused("With --yes, give the first administrator with --admin-email and --admin-password-file.")
        if args.name is not None:
            if not re.fullmatch(r"[a-z0-9][a-z0-9_-]{0,62}", args.name):
                raise Refused("--name takes lower-case letters, digits, - and _, such as tensorquay-staging.")
            if args.network_pool is None:
                raise Refused("A second installation needs its own sandbox network pool: add --network-pool with one "
                              "nothing else uses, such as 10.214.0.0/16 (the install guide's Sandbox networks).")
        if args.network_pool is not None:
            try:
                pool = ipaddress.ip_network(args.network_pool)
            except ValueError:
                raise Refused("--network-pool takes an IPv4 network, such as 10.214.0.0/16.") from None
            if pool.version != 4 or not 8 <= pool.prefixlen <= 24 or pool.overlaps(DEFAULT_POOL):
                raise Refused("--network-pool takes an IPv4 network from /8 to /24 that isn't the first "
                              "installation's 10.213.0.0/16, such as 10.214.0.0/16.")
        self.address = self.answer(
            args.address, "Where will people open TensorQuay Agent? [http://localhost:{}]".format(
                args.port or DEFAULT_PORT),
            lambda text: check_address(text, self.system.port_free, args.port))
        self.fake = args.fake_model or (args.model_url is None and args.yes)
        if not self.fake:
            url = self.answer(args.model_url, "Which model server? Its chat-completions address, or Enter for the "
                                              "test model:", lambda text: "" if not text.strip() else
                              check_model_url(text))
            self.fake = not url
        self.key_file = None
        if self.fake:
            self.model_url = "http://{}:8080/v1".format(self.fake_name())
            self.model, self.context = "fixture", DEFAULT_CONTEXT
        else:
            self.model_url = url
            self.model = self.answer(args.model, "  Model name:", check_model_name)
            self.key_file = self.answer(args.key_file, "  Key file (0600), or Enter if the server needs none:",
                                        lambda text: None if not text.strip() else
                                        check_key_file(text, self.repository))
            self.context = DEFAULT_CONTEXT
            if is_local(url) or args.context is not None:
                self.context = self.answer(args.context, "  Context window in tokens [32768]:", check_context)
        self.email = self.answer(args.admin_email, "First administrator's email:", check_email)
        if args.admin_password_file:
            try:
                text = Path(args.admin_password_file).expanduser().read_text()
            except OSError:
                raise Refused("Can't read {}.".format(args.admin_password_file)) from None
            self.password = check_password(text.splitlines()[0] if text.splitlines() else "")
        else:
            self.password = self.secret_answer("Password (12 characters or more, not shown):", check_password)
        self.extra = (self.password,)

    def fake_name(self):
        return "tqa-fake-model" + ("-" + self.args.name if self.args.name else "")

    def control_network(self):
        return (self.args.name or "tqa") + "_control"

    def detect(self):
        """Rootless Docker's socket and its group, as step 1's block finds them."""
        found = {}
        options = self.system.run(["docker", "info", "--format", "{{.SecurityOptions}}"], capture=True)
        if "name=rootless" in options:
            runtime = os.environ.get("XDG_RUNTIME_DIR") or "/run/user/{}".format(os.getuid())
            found["TQA_DOCKER_SOCKET"] = runtime + "/docker.sock"
            found["TQA_DOCKER_GID"] = self.system.run(
                ["docker", "run", "--rm", "--network", "none", "--mount",
                 "type=bind,source={},target=/var/run/docker.sock,readonly".format(found["TQA_DOCKER_SOCKET"]),
                 TOYBOX, "stat", "-c", "%g"], capture=True).strip()
        return found

    def pull_command(self):
        command = [str(self.repository / "deploy" / "pull-images.sh")]
        if self.args.workstation:
            command.append("--workstation")
        if self.args.desktop:
            command.append("--desktop")
        return command

    def step(self, name, args, timeout):
        try:
            return self.system.run(args, capture=True, timeout=timeout)
        except subprocess.CalledProcessError as error:
            report = text_io.StringIO()
            shell.command_failure(args, self.secrets_env, error, file=report, extra=self.extra)
            raise StepFailed(name + " failed.\n" + report.getvalue().rstrip()) from None

    def environment(self, images, detected, key_file):
        env = {
            "TQA_INSTALL_DIR": str(self.install_dir), "TQA_PROVIDER_KEY_FILE": str(key_file),
            "TQA_MODEL_BASE_URL": self.model_url, "TQA_MODEL": self.model, "TQA_MODEL_CONTEXT": str(self.context),
            "TQA_HTTP_PORT": str(self.address.port), "TQA_PUBLIC_ORIGIN": self.address.origin, **images, **detected,
        }
        if self.args.name:
            env.update(COMPOSE_PROJECT_NAME=self.args.name, TQA_GATEWAY_CONTAINER="tqa-gateway-" + self.args.name,
                       TQA_SANDBOX_NETWORK="tqa-sandboxes-" + self.args.name)
        return env

    def write_files(self, images, detected):
        with tempfile.TemporaryDirectory(prefix="tqa-install-") as scratch:
            key_file = self.key_file
            if key_file is None:
                # A server that needs no key still gets a placeholder (step 1's rule); it lives only for this write.
                key_file = Path(scratch) / "model-key"
                key_file.write_text("local\n")
                key_file.chmod(0o600)
            install_files.create(self.environment(images, detected, key_file), self.repository)
        if self.args.network_pool:
            with (self.install_dir / "gateway.toml").open("a") as toml:
                toml.write('[sandbox.network_pool]\ncidr = "{}"\nprefix = 29\n'.format(self.args.network_pool))
        for line in self.compose_env.read_text().splitlines():
            name, _, value = line.partition("=")
            if name in shell.SECRET_NAMES:
                self.secrets_env[name] = value

    def run(self):
        args = self.args
        if args.upgrade:
            raise Refused(UPGRADE)
        self.refuse_existing()
        self.gather()
        detected = self.detect()
        if args.dry_run:
            self.io.say("Would write in {}: install.key, secrets.toml, models.toml, gateway.toml, compose.env and "
                        "storage/.".format(self.install_dir))
            self.io.say("Would run: " + " ".join(self.pull_command()) + (", the test model's build" if self.fake
                                                                          else "") + ", " + " ".join(
                self.compose("up", "-d", "--no-build", "--wait")) + ".")
            return 0
        self.io.say("Pulling the release's images (a few minutes)…")
        exports = self.step("Pulling the images", self.pull_command(), 3600)
        images = dict(re.findall(r"^export (TQA_(?:RUNTIME|GATEWAY|WEB)_IMAGE)='([^']+)'$", exports, re.M))
        if len(images) != 3:
            raise StepFailed("Pulling the images failed: deploy/pull-images.sh printed no image lines.")
        if self.fake:
            self.io.say("Building the test model…")
            self.step("Building the test model", ["docker", "build", "-f", "images/fake-model/Dockerfile", "-t",
                                                  FAKE_IMAGE, "."], 3600)
        self.write_files(images, detected)
        self.io.say("Created {}.".format(self.install_dir))
        return self.start(images)

    def start(self, images):
        if not images["TQA_RUNTIME_IMAGE"].startswith("sha256:"):
            self.step("Pulling the runtime image", ["docker", "pull", images["TQA_RUNTIME_IMAGE"]], 3600)
        self.step("Giving the container account its files", [
            "docker", "run", "--rm", "--network", "none", "--user", "0:0", "--mount",
            "type=bind,source={},target=/install".format(self.install_dir), TOYBOX,
            "chown", "-R", "1000:1000", "/install/storage", "/install/install.key", "/install/secrets.toml"], 300)
        gateway = images["TQA_GATEWAY_IMAGE"]
        if not (gateway.startswith("sha256:") or gateway.endswith(":local")):
            self.step("Pulling the gateway and web images", self.compose("pull", "gateway", "web"), 3600)
        self.io.say("Starting…")
        self.step("Starting the stack", self.compose("up", "-d", "--no-build", "--wait"), 900)
        if self.fake:
            self.step("Starting the test model", ["docker", "run", "-d", "--name", self.fake_name(), "--network",
                                                  self.control_network(), FAKE_IMAGE], 300)
        base = "http://127.0.0.1:{}".format(self.address.port)
        waited = 0
        while not self.ready(base):
            if waited >= READY_SECONDS:
                raise StepFailed("The gateway didn't answer ready within 3 minutes. Its log: {}".format(
                    " ".join(self.compose("logs", "gateway"))))
            self.system.sleep(2)
            waited += 2
        self.io.say("Ready in {} s.".format(waited))
        status, _ = self.system.http("POST", base + "/api/v1/auth/bootstrap", body=json.dumps(
            {"email": self.email, "password": self.password, "token": self.secrets_env["TQA_BOOTSTRAP_TOKEN"]}),
            headers={"Origin": self.address.origin, "Content-Type": "application/json"}, timeout=30)
        if status not in (200, 201):
            self.io.say("It is running, but the first administrator couldn't be created (the gateway answered {}). "
                        "Open {}/setup and create it there with TQA_BOOTSTRAP_TOKEN in {}.".format(
                            status, self.address.origin, self.compose_env))
            return 4
        self.report_detail(base)
        self.check_route(base)
        if self.address.https:
            self.io.say("Behind your TLS proxy, send {} to http://127.0.0.1:{} and pass the Host and Origin headers "
                        "through.".format(self.address.origin, self.address.port))
        self.io.say("Ready. Open {} and sign in as {}.".format(self.address.origin, self.email))
        return 0

    def ready(self, base):
        try:
            status, _ = self.system.http("GET", base + "/readyz", timeout=5)
        except OSError:
            return False
        return status == 200

    def report_detail(self, base):
        try:
            status, detail = self.system.http("GET", base + "/readyz?detail=1", timeout=10)
        except OSError:
            return
        failing = sorted(name for name, ok in ((detail or {}).get("checks") or {}).items() if ok is False)
        if status == 200 and failing:
            self.io.say("The gateway reports a problem: {}. Settings › Status says more.".format(", ".join(failing)))

    def check_route(self, base):
        if self.fake:
            self.io.say("The test model needs no check.")
            return
        lead = "Checking the model server… "
        try:
            status, answer = self.system.http(
                "POST", base + "/api/v1/admin/routes/local/probe",
                headers={"Origin": self.address.origin, "Idempotency-Key": str(uuid.uuid4())}, timeout=PROBE_SECONDS)
        except (OSError, TimeoutError):
            self.io.say(lead + "no verdict within a minute; run Check in Settings › Models.")
            return
        probe = (answer or {}).get("probe") if status == 200 else None
        if not probe:
            self.io.say(lead + "the check couldn't run (the gateway answered {}); run Check in Settings › "
                               "Models.".format(status))
            return
        observed = probe.get("observed") or {}
        if probe.get("state") == "verified":
            self.io.say(lead + "verified: it works for tasks (answered in {} s).".format(
                round((observed.get("total_ms") or 0) / 1000)))
            return
        code = (observed.get("failure") or {}).get("code", "unknown")
        self.io.say(lead + "the check failed ({}).".format(code))
        self.io.say(STRICT_FIX if code == "strict_line_breaks_lost" else
                    "  Settings › Models › Check names the reason and what to try.")


def main(argv=None, io=None, system=None, repository=None):
    args = parse(sys.argv[1:] if argv is None else argv)
    repository = Path(repository or REPOSITORY)
    io = io or TerminalIO()
    installer = Installer(args, io, system or HostSystem(repository), repository)
    try:
        return installer.run()
    except Refused as refused:
        io.say(str(refused))
        return 2
    except StepFailed as failed:
        for line in str(failed).splitlines():
            io.say(line)
        if installer.compose_env.exists():
            io.say("The files stay in {}. Once the cause is fixed, start it with: {}".format(
                installer.install_dir, " ".join(installer.compose("up", "-d", "--no-build", "--wait"))))
        return 3


if __name__ == "__main__":
    os.umask(0o077)
    sys.exit(main())
