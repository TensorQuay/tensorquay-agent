"""INST-I-001: deploy/install.sh run as a person runs it, on this engine, for the images under test.

The installer stays exactly as users run it: it pulls the release its checkout is tagged with, through
deploy/pull-images.sh. So the check gives it a release of the images under test:
- a registry of its own on a free loopback port, holding TQA_RUNTIME_IMAGE, TQA_GATEWAY_IMAGE and TQA_WEB_IMAGE
  under the release's names;
- a clone of this checkout's commit with one release tag of its own (the clone has no remote, so the tag goes
  nowhere).

Then it runs the installer with --yes and --fake-model under its own name, pool, port and folder, and checks the
person's view:
- a port in use is refused, with nothing written;
- the install ends "Ready.";
- the administrator signs in;
- every readiness check is true;
- one task runs end to end on the test model;
- a second run is refused and changes nothing;
- the folder and its secret files are private;
- no secret appears in anything the installer printed.

The engine is shared, so the check moves no one else's tags for good: pull-images.sh retags tqa-runtime:base and
:minimal, and the installer builds tqa-fake-model:local. Their IDs are written to the output folder before the first
change and restored at the end; after a hard kill, that file is the restore by hand. Everything the check starts
carries its name and is removed at the end.

Usage: TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE=<id> TQA_WEB_IMAGE=<id> TQA_RUNTIME_IMAGE=<id> \
  python3 deploy/tests/install_check.py --output DIR
"""
import argparse
import hashlib
import http.cookiejar
import ipaddress
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import compose  # noqa: E402
import shell  # noqa: E402

# The registry image, by digest, never pulled here: the check needs no network beyond this engine.
REGISTRY = "registry:2@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
TOYBOX = "tianon/toybox:0.8.14@sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7"
# The shared tags the installer's steps move: pull-images.sh's runtime tags and the installer's test model build.
SHARED_TAGS = ("tqa-runtime:base", "tqa-runtime:minimal", "tqa-fake-model:local")
IMAGES = {"runtime": "TQA_RUNTIME_IMAGE", "gateway": "TQA_GATEWAY_IMAGE", "web": "TQA_WEB_IMAGE"}
EMAIL = "inst-check@example.org"
INSTALL_SECONDS, TASK_SECONDS = 1500, 180
# A sandbox pool outside the first installation's 10.213.0.0/16 (the installer refuses that one) and the test
# stacks' slices of it.
POOLS = [ipaddress.ip_network("10.{}.0.0/16".format(second)) for second in range(230, 251)]


class CheckFailed(Exception):
    pass


def require(condition, message):
    if not condition:
        raise CheckFailed(message)


def run(args, *, env=None, cwd=None, timeout=300, check=True):
    done = subprocess.run(args, env=env, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    if check and done.returncode != 0:
        raise shell.CommandFailed(args, done.returncode, done.stderr)
    return done


def image_id(reference):
    done = run(["docker", "image", "inspect", "--format", "{{.Id}}", reference], check=False)
    return done.stdout.strip() if done.returncode == 0 else None


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def mounted_configs(env):
    """The gateway.toml of every gateway container on the engine, running or stopped: an installation may keep its
    folder anywhere, and a stopped one still owns its pool."""
    names = run(["docker", "ps", "-aq"], env=env).stdout.split()
    if not names:
        return []
    done = run(["docker", "inspect", "--format",
                '{{range .Mounts}}{{if eq .Destination "/etc/tqa/gateway.toml"}}{{.Source}}{{"\\n"}}{{end}}{{end}}',
                *names], env=env, check=False)
    return [Path(line) for line in done.stdout.splitlines() if line.strip()]


def kept_pools(env=None):
    """Pools named in installations' gateway.toml files: they are no network until a sandbox exists. Found under
    ~/.local/share and in every gateway container's mount (a folder elsewhere, as a developer's own installation)."""
    found = []
    configs = set(Path.home().glob(".local/share/*/gateway.toml"))
    if env is not None:
        configs.update(mounted_configs(env))
    for config in sorted(configs):
        try:
            text = config.read_text(errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            if line.replace(" ", "").startswith("cidr="):
                try:
                    found.append(ipaddress.ip_network(line.split("=", 1)[1].strip().strip("\"'"), strict=False))
                except ValueError:
                    pass
    return found


def free_pool(env):
    taken = [ipaddress.ip_network(subnet, strict=False) for subnet in compose.engine_subnets(env)] + kept_pools(env)
    free = [pool for pool in POOLS if not any(pool.version == other.version and pool.overlaps(other)
                                               for other in taken)]
    require(free, "no free 10.230-250 /16 sandbox pool on this engine")
    return str(free[secrets.randbelow(len(free))])


def tree_state(directory):
    """Each entry's mode, size and change time, and the content's hash where this account can read it (storage and
    the two secret files belong to the container's account)."""
    state, paths = {}, [directory]
    for top, folders, files in os.walk(directory):  # skips what this account can't list
        paths += [Path(top) / name for name in folders + files]
    for path in sorted(paths):
        info = path.lstat()
        digest = None
        if path.is_file() and os.access(path, os.R_OK):
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
        state[str(path.relative_to(directory))] = (oct(info.st_mode), info.st_size, info.st_mtime_ns, digest)
    return state


class Session:
    """The person's browser, as far as the API goes: a cookie, the Origin, an idempotency key on each write."""

    def __init__(self, origin):
        self.origin = origin
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def call(self, method, path, body=None, timeout=30):
        headers = {"Origin": self.origin, "Content-Type": "application/json"}
        if method != "GET":
            headers["Idempotency-Key"] = secrets.token_hex(16)
        request = urllib.request.Request(self.origin + path, method=method, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        try:
            with self.opener.open(request, timeout=timeout) as response:
                text = response.read().decode()
                return response.status, json.loads(text) if text.strip() else None
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode(errors="replace")[:300]


class Check:
    def __init__(self, output, env):
        self.output = output
        self.env = env
        self.name = "tqa-inst-" + secrets.token_hex(3)
        self.version = "0.0.0-inst." + secrets.token_hex(3)
        self.scratch = Path(tempfile.mkdtemp(prefix="tqa-install-check-"))
        self.install_dir = self.scratch / "install"
        self.clone = self.scratch / "clone"
        self.password_file = self.scratch / "admin-password"
        self.password = secrets.token_urlsafe(18)
        self.registry_name = self.name + "-registry"
        self.registry_port = None
        self.saved = {}
        self.printed = []
        self.steps = []

    def say(self, line):
        print("== {} {}".format(time.strftime("%H:%M:%S"), line), flush=True)
        self.steps.append(line)

    # ---- What the check sets up ------------------------------------------------------------------------------------

    def save_tags(self):
        self.saved = {tag: image_id(tag) for tag in SHARED_TAGS}
        (self.output / "shared-tags-before.json").write_text(json.dumps(self.saved, indent=2) + "\n")
        self.say("shared tags saved to {}".format(self.output / "shared-tags-before.json"))

    def mirror(self):
        """The images under test, as a release on a registry of the check's own."""
        run(["docker", "run", "-d", "--pull", "never", "--name", self.registry_name, "--label", "tqa.install-check=1",
             "-p", "127.0.0.1::5000", REGISTRY])
        published = run(["docker", "port", self.registry_name, "5000/tcp"]).stdout.split()[0]
        self.registry_port = int(published.rsplit(":", 1)[1])
        registry = "127.0.0.1:{}/tensorquay".format(self.registry_port)
        for short, variable in IMAGES.items():
            name = "{}/tensorquay-agent-{}:{}".format(registry, short, self.version)
            run(["docker", "tag", compose.required_image(self.env, variable), name])
            run(["docker", "push", "--quiet", name], timeout=900)
            run(["docker", "image", "rm", name])
        self.say("images under test pushed to {} as release {}".format(registry, self.version))
        return registry

    def checkout(self):
        """This checkout's commit, cloned with one release tag of its own; deploy/ and the test model must be
        committed, since a clone sees commits only."""
        dirty = run(["git", "status", "--porcelain", "--", "deploy", "images/fake-model"], cwd=ROOT).stdout
        require(not dirty.strip(), "deploy/ or images/fake-model has uncommitted changes; the clone would not "
                                   "have them:\n" + dirty)
        commit = run(["git", "rev-parse", "HEAD"], cwd=ROOT).stdout.strip()
        run(["git", "clone", "--quiet", "--shared", "--no-tags", "--no-checkout", str(ROOT), str(self.clone)])
        run(["git", "remote", "remove", "origin"], cwd=self.clone)
        run(["git", "-c", "advice.detachedHead=false", "checkout", "--quiet", "--detach", commit], cwd=self.clone)
        run(["git", "tag", "v" + self.version], cwd=self.clone)
        described = run(["git", "describe", "--tags", "--exact-match"], cwd=self.clone).stdout.strip()
        require(described == "v" + self.version, "the clone describes itself as {}".format(described))
        self.say("commit {} cloned as release {}".format(commit[:9], described))
        return commit

    def installer(self, registry, port, pool, *, timeout=INSTALL_SECONDS):
        self.password_file.write_text(self.password + "\n")
        self.password_file.chmod(0o600)
        env = dict(self.env, TQA_IMAGE_REGISTRY=registry)
        args = ["deploy/install.sh", "--yes", "--fake-model", "--admin-email", EMAIL, "--admin-password-file",
                str(self.password_file), "--install-dir", str(self.install_dir), "--name", self.name,
                "--network-pool", pool, "--port", str(port)]
        done = subprocess.run(args, cwd=self.clone, env=env, capture_output=True, text=True, timeout=timeout)
        printed = done.stdout + done.stderr
        self.printed.append(printed)
        return done.returncode, printed

    # ---- The person's view -----------------------------------------------------------------------------------------

    def task(self, session):
        status, conversation = session.call("POST", "/api/v1/conversations",
                                            {"title": "INST-I-001", "model_route": "local"})
        require(status == 201, "creating a task answered {}: {}".format(status, conversation))
        path = "/api/v1/conversations/" + conversation["id"]
        status, _ = session.call("POST", path + "/messages", {"text": "Say hello in one short line."})
        require(status == 202, "sending the message answered {}".format(status))
        deadline = time.monotonic() + TASK_SECONDS
        while True:
            status, state = session.call("GET", path)
            if status == 200 and state.get("run_status") in ("completed", "failed", "cancelled"):
                break
            require(time.monotonic() < deadline, "the task's run did not end within {} s".format(TASK_SECONDS))
            time.sleep(2)
        require(state["run_status"] == "completed", "the task's run ended {}".format(state["run_status"]))
        status, events = session.call("GET", path + "/events")
        require(status == 200, "reading the task's events answered {}".format(status))
        kinds = [item["event"]["type"] for item in events["items"]]
        results = [item["event"].get("status") for item in events["items"] if item["event"]["type"] == "tool.result"]
        require("run.completed" in kinds, "no run.completed among {}".format(sorted(set(kinds))))
        require(results and all(result == "ok" for result in results), "tool results {}".format(results))
        answer = "".join(item["event"].get("text", "") for item in events["items"]
                         if item["event"]["type"] == "text.delta")
        self.say("one task ran end to end: {} tool results ok, answer {!r}".format(len(results), answer[-80:]))

    def check(self):
        self.save_tags()
        registry = self.mirror()
        self.checkout()
        pool = free_pool(self.env)

        with socket.socket() as busy:
            busy.bind(("127.0.0.1", 0))
            busy.listen()
            code, printed = self.installer(registry, busy.getsockname()[1], pool, timeout=300)
        require(code == 2, "a port in use: exit {}\n{}".format(code, printed))
        require(not self.install_dir.exists(), "a port in use: the installer wrote {}".format(self.install_dir))
        self.say("a port in use is refused (exit 2), nothing written")

        port = free_port()
        started = time.monotonic()
        code, printed = self.installer(registry, port, pool)
        (self.output / "install.log").write_text(printed)
        require(code == 0, "the install exited {}:\n{}".format(code, printed))
        origin = "http://localhost:{}".format(port)
        ready = "Ready. Open {} and sign in as {}.".format(origin, EMAIL)
        require(printed.rstrip().endswith(ready), "the install's last line is not {!r}".format(ready))
        self.say("installed in {:.0f} s: {}".format(time.monotonic() - started, ready))

        session = Session(origin)
        status, _ = session.call("POST", "/api/v1/auth/login", {"email": EMAIL, "password": self.password})
        require(status == 200, "signing in answered {}".format(status))
        status, me = session.call("GET", "/api/v1/auth/me")
        require(status == 200 and me.get("role") == "admin", "the administrator: {} {}".format(status, me))
        status, detail = session.call("GET", "/readyz?detail=1")
        require(status == 200 and detail.get("ready") is True and all(detail.get("checks", {}).values()),
                "readiness: {} {}".format(status, detail))
        self.say("the administrator signs in; readiness checks all true ({})".format(", ".join(sorted(
            detail["checks"]))))
        self.task(session)

        before = tree_state(self.install_dir)
        code, printed = self.installer(registry, free_port(), pool, timeout=300)
        require(code == 2 and "There is already an installation in {}".format(self.install_dir) in printed,
                "a second run: exit {}\n{}".format(code, printed))
        require(tree_state(self.install_dir) == before, "a second run changed the installation's files")
        self.say("a second run is refused (exit 2) and changes nothing")

        modes = {name: oct((self.install_dir / name).stat().st_mode & 0o777)
                 for name in (".", "compose.env", "secrets.toml", "install.key")}
        require(modes == {".": "0o700", "compose.env": "0o600", "secrets.toml": "0o600", "install.key": "0o600"},
                "modes {}".format(modes))
        self.say("the folder is 0700 and its secret files 0600")

    def secrets_printed(self):
        """How many times the password, the bootstrap token and the database password appear in what it printed."""
        values = self.secret_values()
        return sum(text.count(value) for text in self.printed for value in values), len(values)

    # ---- Leaving the engine as it was ------------------------------------------------------------------------------

    def teardown(self):
        quiet = dict(check=False, timeout=300)
        compose_env = self.install_dir / "compose.env"
        if compose_env.exists():
            logs = run(["docker", "compose", "--env-file", str(compose_env), "-f", "deploy/compose/docker.yml", "logs",
                        "--no-color", "gateway"], cwd=self.clone, **quiet)
            (self.output / "gateway.log").write_text(shell.redact(logs.stdout + logs.stderr, {}, self.secret_values()))
        sandboxes = run(["docker", "ps", "-aq", "--filter", "name=^tqa-sandboxes-{}-".format(self.name)],
                        **quiet).stdout.split()
        if sandboxes:
            run(["docker", "rm", "-f", *sandboxes], **quiet)
        run(["docker", "rm", "-f", "tqa-fake-model-" + self.name], **quiet)
        if compose_env.exists():
            run(["docker", "compose", "--env-file", str(compose_env), "-f", "deploy/compose/docker.yml", "down", "-v"],
                cwd=self.clone, **quiet)
        run(["docker", "network", "rm", "tqa-sandboxes-" + self.name], **quiet)
        run(["docker", "rm", "-f", "-v", self.registry_name], **quiet)
        if self.registry_port:
            prefix = "127.0.0.1:{}/".format(self.registry_port)
            mirrored = [line for line in run(["docker", "images", "--format", "{{.Repository}}:{{.Tag}}"],
                                             **quiet).stdout.split() if line.startswith(prefix)]
            if mirrored:
                run(["docker", "image", "rm", *mirrored], **quiet)
        if self.install_dir.exists():
            # Its storage and secret files belong to the container's account: give them back before removing them.
            run(["docker", "run", "--rm", "--network", "none", "--user", "0:0", "--mount",
                 "type=bind,source={},target=/install".format(self.install_dir), TOYBOX, "chown", "-R", "0:0",
                 "/install"], **quiet)
        for tag, saved in self.saved.items():
            moved = image_id(tag)
            if saved and moved and moved != saved:
                run(["docker", "tag", saved, tag], **quiet)
                # The check's own build, now untagged: removed unless another tag or a container still holds it
                # (never forced).
                run(["docker", "image", "rm", moved], **quiet)
            elif saved is None and moved:
                run(["docker", "image", "rm", tag], **quiet)
        restored = {tag: image_id(tag) for tag in SHARED_TAGS}
        run(["rm", "-rf", str(self.scratch)], **quiet)
        return restored == self.saved, restored

    def secret_values(self):
        values = [self.password]
        compose_env = self.install_dir / "compose.env"
        if compose_env.exists():
            values += [line.partition("=")[2] for line in compose_env.read_text().splitlines()
                       if line.partition("=")[0] in shell.SECRET_NAMES]
        return [value for value in values if value]


def main(argv=None):
    parser = argparse.ArgumentParser(description="INST-I-001: deploy/install.sh on this engine")
    parser.add_argument("--output", required=True, help="a folder for the result, the logs and the saved tags")
    args = parser.parse_args(argv)
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    for variable in IMAGES.values():
        compose.required_image(env, variable)
    check = Check(output, env)
    status, failure = "passed", None
    try:
        check.check()
    except Exception as error:  # every failure is reported in result.json, and the engine is still put back
        status = "failed"
        failure = shell.redact(str(error) if not isinstance(error, shell.CommandFailed) else "{}: {}".format(
            " ".join(map(str, error.cmd)), (error.stderr or "")[-2000:]), {}, check.secret_values())
        print("FAILED: " + failure, flush=True)
    finally:
        hits, checked = check.secrets_printed()
        restored, tags = check.teardown()
    if status == "passed" and hits:
        status, failure = "failed", "{} secret value(s) appear in the installer's output".format(hits)
    if status == "passed" and not restored:
        status, failure = "failed", "the shared tags were not restored: {}".format(tags)
    check.say("secrets in the installer's output: {} (of {} values checked); shared tags restored: {}".format(
        hits, checked, restored))
    result = {"id": "INST-I-001", "status": status, "failure": failure, "steps": check.steps}
    (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print("INST-I-001 {}".format(status), flush=True)
    return 0 if status == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
