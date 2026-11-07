"""The push fixture: a real authenticated Git origin with its CA and egress rule (SEC-019), and KeyPushStack, a task
that pushes to it with plain git and the person's host key (ADR-0037)."""
import json
import os
from pathlib import Path
import secrets
import subprocess
import time
import urllib.request

from .stack import Stack, compose

BRANCH, RESTART = "feature/e2e", "feature/restart"
# Every form of the credential (web_connector.forms: raw, URL-encoded, base64 at any alignment, so an HTTP Basic header
# too) comes on stdin as a JSON list, so the search never places it in its own argv or environment. Prints each place
# holding one.
SEARCH_TEMPLATE = r"""
import json, os, sys
forms = [form.encode() for form in json.load(sys.stdin)]
def found(data):
    return any(form in data for form in forms)
hits = []
for base in ROOTS:
    for directory, names, files in os.walk(base, followlinks=False):
        for name in files:
            path = os.path.join(directory, name)
            try:
                with open(path, "rb") as source:
                    if found(source.read(16 * 1024 * 1024)):
                        hits.append(path)
            except OSError:
                pass
try:
    pids = os.listdir(PROC)
except OSError:
    pids = []
for pid in pids:
    if pid.isdigit() and int(pid) != os.getpid():
        for part in ("environ", "cmdline"):
            place = os.path.join(PROC, pid, part)
            try:
                with open(place, "rb") as source:
                    if found(source.read()):
                        hits.append(place)
            except OSError:
                pass
print("\n".join(hits))
"""
SEARCH_ROOTS = ("/workspace", "/tmp", "/run", "/home", "/etc", "/usr/local")


def search_program(roots=SEARCH_ROOTS, proc="/proc"):
    """The in-sandbox search over `roots` and `proc` (the defaults are the sandbox's own)."""
    return "ROOTS = {!r}\nPROC = {!r}\n".format(list(roots), proc) + SEARCH_TEMPLATE


SEARCH = search_program()


class PushStack(Stack):
    def overlay(self):
        super().overlay()
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "upstream.py", Path(__file__).with_name("push_server.py").read_text())
        self.secret = "synthetic-push-credential-" + secrets.token_hex(24)
        compose.write(self.directory / "push-token", self.secret + "\n")
        path = self.directory.parent / "egress.json"
        overlay = json.loads(path.read_text())
        overlay["services"]["upstream"]["tmpfs"] = ["/tmp:rw,noexec,nosuid,size=64m,mode=1777"]
        overlay["services"]["upstream"]["mem_limit"] = "128m"
        compose.write(path, json.dumps(overlay))
        compose.ownership(self.directory, self.env, "1000:1000")

    def certificate(self):
        """A throwaway CA and a separate server certificate; TLS stacks refuse a CA as the server's own."""
        compose.ownership(self.directory, self.env)
        d = self.directory
        compose.write(d / "leaf.cnf", "[extensions]\nsubjectAltName=IP:" + self.upstream
                      + "\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment"
                      + "\nextendedKeyUsage=serverAuth\n")
        steps = [["openssl", "req", "-x509", "-nodes", "-newkey", "rsa:2048", "-days", "1", "-subj", "/CN=push-fixture-ca",
                  "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                  "-keyout", str(d / "ca-key.pem"), "-out", str(d / "fixture-ca.pem")],
                 ["openssl", "req", "-new", "-nodes", "-newkey", "rsa:2048", "-subj", "/CN=push-fixture",
                  "-keyout", str(d / "fixture-key.pem"), "-out", str(d / "leaf.csr")],
                 ["openssl", "x509", "-req", "-days", "1", "-in", str(d / "leaf.csr"), "-CA", str(d / "fixture-ca.pem"),
                  "-CAkey", str(d / "ca-key.pem"), "-CAcreateserial", "-extfile", str(d / "leaf.cnf"),
                  "-extensions", "extensions", "-out", str(d / "fixture-leaf.pem")]]
        for step in steps:
            compose.command(step, self.env)
        compose.write(d / "fixture-chain.pem", (d / "fixture-leaf.pem").read_text() + (d / "fixture-ca.pem").read_text())
        (d / "ca-key.pem").unlink()
        compose.write(d / "certificate-ready", "ready")
        # The origin runs as 1000:1000 and waits for certificate-ready; scenario() polls it before writing anything.
        compose.ownership(self.directory, self.env, "1000:1000")

    def upstream_get(self, path):
        program = ("import urllib.request; print(urllib.request.urlopen('http://127.0.0.1:8081" + path
                   + "',timeout=5).read(262145).decode())")
        return json.loads(self.command(self.configuration + ["exec", "-T", "upstream", "python3", "-c", program]))

    def scenario(self):
        deadline = time.monotonic() + 60
        while True:
            try:
                self.commit = self.upstream_get("/refs")["refs/heads/main"]
                break
            except Exception:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)
        self.remote = "https://{}:8443/repo.git".format(self.upstream)
        # The supported push, plain git in the task's shell (ADR-0037; the git_push tool is gone). A subclass that adds
        # the person's host key and sends its own messages writes its own scenario over this one.
        calls = [("shell", {"command": "git checkout -b " + BRANCH}), ("shell", {"command": push_command(BRANCH)}), None,
                 ("shell", {"command": "git checkout -b " + RESTART}), ("shell", {"command": push_command(RESTART)}),
                 None]
        responses = []
        for call in calls:
            events = ([{"type": "text", "text": "The push step is finished."}] if call is None else
                      [{"type": "tool_call", "name": call[0], "arguments": call[1], "split_into": 1}])
            responses.append({"events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}})
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": responses}))
        storage = Path(self.env["TQA_STORAGE_ROOT"])
        compose.write(storage / "fixture-ca.pem", (self.directory / "fixture-ca.pem").read_text())
        # The base prepare() writes it into `[egress] ca_file` next, then hands the tree to the gateway user itself.
        self.ca_file = str(storage / "fixture-ca.pem")

    def request_method(self, method, path, body):
        request = urllib.request.Request(
            self.origin + path, method=method, data=json.dumps(body).encode(),
            headers={"Origin": self.origin, "Content-Type": "application/json",
                     "Idempotency-Key": secrets.token_hex(16)})
        with self.client.open(request, timeout=15) as response:
            data = response.read(1048577)
            return json.loads(data) if data else None

    def seed(self):
        """The workspace starts as an existing clone of the origin, made outside every sandbox."""
        compose.ownership(self.directory, self.env)
        workspace = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / self.workspace
        workspace.mkdir(parents=True, exist_ok=True)
        compose.ownership(self.directory, self.env, "1000:1000")
        script = ('read -r token; git -c "http.extraHeader=Authorization: Bearer $token" '
                  'clone --quiet "$REMOTE" /workspace')
        arguments = ["docker", "run", "--rm", "-i", "--user", "1000:1000", "--network", self.env["TQA_SANDBOX_NETWORK"],
                     "--read-only", "--tmpfs", "/tmp", "-e", "HOME=/tmp", "-e", "REMOTE=" + self.remote,
                     "-e", "GIT_SSL_CAINFO=/ca.pem",
                     "--mount", "type=bind,source={},target=/ca.pem,readonly".format(self.directory / "fixture-ca.pem"),
                     "--mount", "type=bind,source={},target=/workspace".format(workspace),
                     "--entrypoint", "sh", self.runtime, "-c", script]
        subprocess.run(arguments, input=self.secret + "\n", env=self.env, check=True, timeout=60,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        # Requests before this point are the fixture's own checkout, not the run's.
        self.seeded = len(self.upstream_get("/witness"))

    def stored(self):
        return self.request(self.path + "/events?after=0&limit=1000")["items"]

    def gateway_address(self):
        gateway = self.command(self.configuration + ["ps", "-q", "gateway"])
        networks = json.loads(self.command(["docker", "inspect", "--format", "{{json .NetworkSettings.Networks}}", gateway]))
        return networks[self.env["TQA_SANDBOX_NETWORK"]]["IPAddress"]

    def search(self, container):
        from .web_connector import forms
        result = subprocess.run(["docker", "exec", "-i", container, "python3", "-c", SEARCH],
                                input=json.dumps(forms(self.secret)),
                                env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=120)
        assert result.returncode == 0, "The credential search could not run"
        return [line for line in result.stdout.splitlines() if line]

    # The upstream's own account of the requests it saw: the Git origin's witness; a stack that replaces the origin
    # with another fixture names that fixture's route.
    upstream_record = "/witness"

    def diagnostics(self):
        super().diagnostics()
        try:
            print(json.dumps(self.upstream_get(self.upstream_record))[-4000:], flush=True)
        except Exception as error:
            print("Upstream {} unavailable: ".format(self.upstream_record) + compose.redact(str(error), self.env),
                  flush=True)


# The person's own host key for the origin (ADR-0037): a task sees its name as a placeholder, which the egress proxy
# swaps for the value on the way to that host. The Git relay and its credentials are retired (ADR-0037 §7).
PUSH_KEY = "FIXTURE_ORIGIN"


def push_command(branch):
    """The supported push: plain git, the key's placeholder in its Authorization header."""
    return 'git -c "http.extraHeader=Authorization: Bearer ${%s}" push -q origin HEAD:refs/heads/%s' % (PUSH_KEY, branch)


class KeyPushStack(PushStack):
    """The supported push (ADR-0037): PushStack's origin, CA and seeded clone, with the person's own bearer host key
    for the origin instead of a Git credential, a workspace without a git address (the clone is seeded), and the task
    pushing with plain git (`push_command`). `start` signs in, adds the key and makes the workspace and its task; a
    subclass sends the first message."""

    def add_key(self, value):
        """The person's key `PUSH_KEY` for the origin's exact address, with `value`; the answer in `key`."""
        self.key = self.request("/api/v1/host-keys", {"name": PUSH_KEY, "address": "https://{}:8443".format(
            self.upstream), "scheme": "bearer", "value": value})

    def sign_in(self, email):
        password = self.env.setdefault("TQA_STACK_PASSWORD", "synthetic-push-" + secrets.token_hex(16))
        account = {"email": email, "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)

    def redact(self, text):
        """The stack's own redaction, and the key's value: a setup that fails before the value exists has none."""
        text = compose.redact(text, self.env)
        secret = getattr(self, "secret", None)
        return text.replace(secret, "[REDACTED]") if secret else text

    def container(self, conversation):
        """The task's sandbox container, checked to run the image under test."""
        found = self.command(["docker", "ps", "-q", "--filter", "label=tqa.conversation=" + conversation]).split()
        assert len(found) == 1, "one running sandbox for the task: {}".format(found)
        image = self.command(["docker", "inspect", "--format", "{{.Image}}", found[0]]).strip()
        assert image == self.runtime, "the task runs the image under test, not {}".format(image)
        return found[0]

    def start(self, email="push@example.invalid", title="Push contract"):
        self.sign_in(email)
        self.add_key(self.secret)
        self.workspace = self.request("/api/v1/workspaces", {"name": "push fixture"})["id"]
        self.seed()
        conversation = self.request("/api/v1/conversations", {"title": title, "model_route": "local",
                                                              "workspace_id": self.workspace})
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation
