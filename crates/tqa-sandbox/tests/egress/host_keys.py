"""The host-keys stack suite (ADR-0037) on an owned Docker stack with the release images: no test hook. The keyed
host is the fixture's origin at its exact address (Amendment 2), its certificate from the fixture's own authority, which
the gateway trusts only through `[egress] ca_file` (ADR-0037 §4).

SEC-047 (R2): a run uses its key through the proxy. The host receives the value; the task sees only the placeholder,
including in the host's echo of it (the proxy scrubs answers); and the value is in no event, no log, no file or
environment of the sandbox, and no plaintext table (the secret store keeps it encrypted).

Run from the repository root, on images built from the commit under test:
    PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE=sha256:<gateway image id> \\
    TQA_RUNTIME_TEST_IMAGE=<workstation runtime image> python3 -m egress.host_keys
"""
import contextlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import tempfile
import time
import urllib.request

from .git_push import PushStack
from .stack import Stack, compose

DEADLINE_SECONDS = 900
NAME = "FIXTURE_KEY"
# Each case's first message, which picks its script.
SEC_047 = "Call the keyed host"
SEC_048 = "Keep the placeholder"
SEC_048_OTHER = "Try a placeholder from another task"
SEC_049 = "Hold a keyed connection"
SEC_049_GIT = "Clone and push with the keyed host"
SEC_049_COUNT = "Count twenty-five keyed requests"
SEC_049_LATER = "Look for the later key"
SEC_046_OWNER = "Leave a key in the shared workspace"
SEC_046_TEAMMATE = "Use what a teammate left"
# SEC-046's team case (ADR-0034): the owner leads a team on their own workspace; the teammate leads one on the shared
# workspace. A helper's script is picked by its brief's first words, "You are <role>".
SEC_046_OWNER_LEAD = "Lead a keyed team"
SEC_046_TEAMMATE_LEAD = "Lead a team on the shared files"
KEYHOLDER = "keyholder"
BORROWER = "borrower"
SEC_055 = "Call across a gateway restart"
SBX_C_019 = "Check each client against the keyed host"
SEC_056_PLAIN = "Reach a private name plainly"
SEC_056_UNSET = "Reach an internal name before configuration"
SEC_056_NEVER = "Reach internal names that are never reached"
# SEC-056's probe: the proxy's answer to a CONNECT for a name, its status line only. `--noproxy ''` sends it through the
# egress proxy whatever NO_PROXY says: since loopback is left out of the proxy (SBX-U-018, TOOL-U-060), curl would take
# `localhost` straight to the sandbox's own loopback, and the proxy's refusal of a reserved address would go untested.
PROBE = "curl -skv --noproxy '' --max-time 20 https://{name}:8443/ 2>&1 | grep -m1 '^< HTTP/' | tr -d '\\r'"
# What internal_never names on the stack: any range will do; the engine's networks are refused regardless.
INTERNAL_NEVER = "192.0.2.1"
# The fixture's port that no key is for (host_key_server.py).
UNKEYED_PORT = 7443
LATER_NAME = "FIXTURE_LATER"
# SEC-046: the owner's own key and a shared key granted to the owner only; the teammate's own key for the same host.
OWNER_NAME = "FIXTURE_OWNER"
SHARED_NAME = "FIXTURE_SHARED"
TEAMMATE_NAME = "FIXTURE_TEAMMATE"
TEAMMATE = "teammate@example.invalid"
LATER_PORT = 9443
GIT_NAME = "FIXTURE_GIT"
# SEC-049's run: one keyed connection through the proxy, a request, then a wait for the test's "go" (the host's rule
# removed meanwhile), then a second request on the same connection. Bounded: a minute for the go.
HOLD = """import http.client, os, ssl, time, urllib.parse
proxy = urllib.parse.urlsplit(os.environ["HTTPS_PROXY"])
context = ssl.create_default_context(cafile=os.environ["SSL_CERT_FILE"])
connection = http.client.HTTPSConnection(proxy.hostname, proxy.port, timeout=30, context=context)
connection.set_tunnel(os.environ["HOST"], 8443)
headers = {"Authorization": "Bearer " + os.environ["%s"]}
def call():
    connection.request("GET", "/echo", headers=headers)
    response = connection.getresponse()
    return response.status, response.read(4096).decode(errors="replace")
print("first", call()[0], flush=True)
open("first-done", "w").close()
deadline = time.monotonic() + 60
while not os.path.exists("go") and time.monotonic() < deadline:
    time.sleep(0.2)
status, body = call()
print("second", status, body, flush=True)
""" % NAME

# SEC-055's run: a keyed connection and a call, then a wait for the test's "go" (the gateway restarted meanwhile), then
# a call on the held connection (gone with the gateway) and one on a fresh connection. Bounded: three minutes for go.
ACROSS = """import http.client, os, ssl, time, urllib.parse
proxy = urllib.parse.urlsplit(os.environ["HTTPS_PROXY"])
context = ssl.create_default_context(cafile=os.environ["SSL_CERT_FILE"])
headers = {"Authorization": "Bearer " + os.environ["%s"]}
def connect():
    connection = http.client.HTTPSConnection(proxy.hostname, proxy.port, timeout=30, context=context)
    connection.set_tunnel(os.environ["HOST"], 8443)
    return connection
def call(connection):
    connection.request("GET", "/echo", headers=headers)
    response = connection.getresponse()
    return response.status, response.read(4096).decode(errors="replace")
held = connect()
print("first", call(held)[0], flush=True)
open("first-done", "w").close()
deadline = time.monotonic() + 180
while not os.path.exists("go") and time.monotonic() < deadline:
    time.sleep(0.2)
try:
    print("held", call(held)[0], flush=True)
except (OSError, http.client.HTTPException) as error:
    print("held ended", type(error).__name__, flush=True)
status, body = call(connect())
print("fresh", status, body, flush=True)
""" % OWNER_NAME

# SBX-C-019's run: each client in the image calls the keyed host with the owner's placeholder, one line each; then the
# unkeyed port, without and with verification. HOST, NAME and UNKEYED are filled in; the snippets keep their braces.
MATRIX = r"""U=https://HOST:8443/echo
printf 'curl %s\n' "$(curl -sS --max-time 20 -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $NAME" "$U" 2>&1)"
python3 - "$U" <<'EOF'
import os, sys
headers = {"Authorization": "Bearer " + os.environ["NAME"]}
try:
    import urllib.request
    print("urllib", urllib.request.urlopen(urllib.request.Request(sys.argv[1], headers=headers), timeout=20).status)
except Exception as error:
    print("urllib error", type(error).__name__, str(error)[:160])
try:
    from pip._vendor import requests
    print("requests", requests.get(sys.argv[1], headers=headers, timeout=20).status_code)
except Exception as error:
    print("requests error", type(error).__name__, str(error)[:160])
EOF
timeout 60 python3 -m pip download --no-input --no-deps --disable-pip-version-check --timeout 20 --retries 0 \
  -d /tmp/pip-probe \
  --index-url https://HOST:8443/simple/ tqa-probe > /tmp/pip.out 2>&1
if grep -qiE 'ssl|certificate' /tmp/pip.out; then
  echo "pip tls-failed $(grep -iE -m1 'ssl|certificate' /tmp/pip.out | cut -c1-160)"
else
  echo "pip tls-ok"
fi
timeout 30 node -e 'fetch(process.argv[1], {headers: {Authorization: "Bearer " + process.env.NAME},
    signal: AbortSignal.timeout(20000)})
  .then((r) => console.log("node", r.status))
  .catch((e) => console.log("node error", (e.cause && e.cause.code) || e.message))' "$U"
verify=$(curl -sS --max-time 20 -o /dev/null -w '%{http_code}' https://HOST:UNKEYED/ 2>&1 | tr '\n' ' ' | cut -c1-160)
printf 'unkeyed-verify %s\n' "$verify"
issuer=$(curl -skv --max-time 20 -o /dev/null https://HOST:UNKEYED/ 2>&1 | grep -m1 'issuer:' | sed 's/^.*issuer: //')
printf 'unkeyed-issuer %s\n' "$issuer"
"""


class HostKeyStack(PushStack):
    """The egress stack with the keyed host in place of the plain upstream, its authority in `[egress] ca_file`."""

    def overlay(self):
        Stack.overlay(self)
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "upstream.py", Path(__file__).with_name("host_key_server.py").read_text())
        # A value no fixture shares, so finding it anywhere is this run's leak. The keyed host's Git repositories take
        # it as their credential (outside every sandbox).
        self.value = "tqa-g9-value-" + secrets.token_hex(24)
        compose.write(self.directory / "host-key-value", self.value + "\n")
        # The Git repositories live in a private tmpfs: the service's root stays read-only.
        path = self.directory.parent / "egress.json"
        overlay = json.loads(path.read_text())
        overlay["services"]["upstream"]["tmpfs"] = ["/tmp:rw,noexec,nosuid,size=64m,mode=1777"]
        overlay["services"]["upstream"]["mem_limit"] = "128m"
        compose.write(path, json.dumps(overlay))
        compose.ownership(self.directory, self.env, "1000:1000")

    def scenario(self):
        self.address = "https://{}:8443".format(self.upstream)
        # Each task follows its own script by its first message's phrase and turn ("select": "prompt"), so the cases
        # never take each other's replies whatever the order of requests.
        call = 'curl -sS -w "\\nstatus %{{http_code}}\\n" -H "Authorization: Bearer {}" ' + self.address + "/echo"
        shell = lambda when, command: {"when": when, "events": [
            {"type": "tool_call", "name": "shell", "arguments": {"command": command}, "split_into": 1}]}
        text = lambda when, words: {"when": when, "events": [{"type": "text", "text": words}]}
        tool = lambda when, name, arguments: {"when": when, "events": [
            {"type": "tool_call", "name": name, "arguments": arguments, "split_into": 1}]}
        # Which of the three keys a run holds, as "NAME=count" for each.
        held = "for n in {} {} {}; do printf '%s=%s ' $n $(env | grep -c \"^$n=\"); done".format(
            OWNER_NAME, SHARED_NAME, TEAMMATE_NAME)

        def start(when, role, workspace, objective):
            return tool(when, "start_member", {"role": role, "workspace": workspace, "profile": None, "brief": {
                "objective": objective, "output": "A report of what each call answered.", "sources": None,
                "boundaries": "Change no file."}})

        def lead_waits(when):
            # The lead ends its turn, so its slot is free for the helper (the stack has few); the helper's report
            # wakes it for a second run, which reads the report and ends the team.
            return [text(when, "I'll wait for my helper's report."),
                    tool(when, "read_reports", {"wait": 30, "limit": None}),
                    tool(when, "done", {"summary": "The helper tried the keyed host."}), text(when, "The team is done.")]

        def helper(role, commands):
            when = "You are " + role
            return [shell(when, command) for command in commands] + [
                tool(when, "send_to_lead", {"type": "report", "text": "I tried the keyed host.", "question_seq": None,
                                            "files": None}),
                text(when, "I reported to my lead.")]

        responses = [
            # SEC-047: one keyed call, then the run's variable.
            shell(SEC_047, call.format("${" + NAME + "}")),
            shell(SEC_047, "env | grep -E '^" + NAME + "=' || true"),
            text(SEC_047, "The keyed call is done."),
            # SEC-048: a run keeps its placeholder in a file and uses it; the task's next run tries the old one.
            shell(SEC_048, 'printf %s "${' + NAME + '}" > old-placeholder && ' + call.format("${" + NAME + "}")),
            text(SEC_048, "The first run is done."),
            shell(SEC_048, call.format("$(cat old-placeholder)")),
            text(SEC_048, "The second run is done."),
            # SEC-048: another task tries the first task's placeholder, copied into its workspace.
            shell(SEC_048_OTHER, call.format("$(cat foreign-placeholder)")),
            text(SEC_048_OTHER, "The other task is done."),
            # SEC-049: a connection held across the removal of its host's rule.
            shell(SEC_049, "HOST={} python3 hold.py".format(self.upstream)),
            text(SEC_049, "The held call is done."),
            # SEC-049, R4: 25 keyed calls in one run; the next run's call while uses can't be recorded.
            shell(SEC_049_COUNT, "for i in $(seq 25); do " + call.format("${" + NAME + "}") + " | tail -1; done "
                                 "| sort | uniq -c"),
            text(SEC_049_COUNT, "The calls are done."),
            shell(SEC_049_COUNT, call.format("${" + NAME + "}")),
            text(SEC_049_COUNT, "The unrecorded call is done."),
            # SEC-049, R5: a key for a port not on the list yet, then allowed between the task's runs.
            shell(SEC_049_LATER, "env | grep -c '^" + LATER_NAME + "=' || true"),
            text(SEC_049_LATER, "Looked once."),
            shell(SEC_049_LATER, "env | grep -c '^" + LATER_NAME + "=' || true"),
            text(SEC_049_LATER, "Looked again."),
            # SEC-046: the owner's run leaves its placeholder in the shared workspace; the teammate's run on it looks
            # for the owner's keys, then calls the host with what the owner left.
            shell(SEC_046_OWNER, 'printf %s "${' + OWNER_NAME + '}" > owner-placeholder && '
                  + call.format("${" + OWNER_NAME + "}")),
            text(SEC_046_OWNER, "The owner's call is done."),
            shell(SEC_046_TEAMMATE, "for n in {} {} {}; do printf '%s=%s ' $n $(env | grep -c \"^$n=\"); done".format(
                OWNER_NAME, SHARED_NAME, TEAMMATE_NAME)),
            shell(SEC_046_TEAMMATE, call.format("$(cat owner-placeholder)")),
            text(SEC_046_TEAMMATE, "The teammate's call is done."),
            # SEC-046's team case: the owner's lead leaves its placeholder, starts a helper on a copy of its files and
            # waits for its report; the helper says which keys it holds, tries the lead's placeholder, then its own.
            shell(SEC_046_OWNER_LEAD, 'printf %s "${' + OWNER_NAME + '}" > lead-placeholder'),
            start(SEC_046_OWNER_LEAD, KEYHOLDER, "own_copy", "Try the keyed host with what you hold."),
            *lead_waits(SEC_046_OWNER_LEAD),
            *helper(KEYHOLDER, [held, call.format("$(cat lead-placeholder)"), call.format("${" + OWNER_NAME + "}")]),
            # The teammate's lead starts a helper on the shared workspace itself, where the owner's run left its
            # placeholder; the helper says which keys it holds, then tries the owner's placeholder.
            start(SEC_046_TEAMMATE_LEAD, BORROWER, "my_files", "Try the keyed host with what is in the files."),
            *lead_waits(SEC_046_TEAMMATE_LEAD),
            *helper(BORROWER, [held, call.format("$(cat owner-placeholder)")]),
            # SBX-C-019: every client in the image against the keyed host, then an unkeyed port.
            shell(SBX_C_019, MATRIX.replace("HOST", self.upstream).replace("NAME", OWNER_NAME)
                  .replace("UNKEYED", str(UNKEYED_PORT))),
            text(SBX_C_019, "Every client has been checked."),
            # SEC-056: named internal servers: the fixture by its service name, then loopback by name.
            shell(SEC_056_PLAIN, PROBE.format(name="upstream")),
            text(SEC_056_PLAIN, "Probed plainly."),
            shell(SEC_056_UNSET, PROBE.format(name="upstream")),
            text(SEC_056_UNSET, "Probed before configuration."),
            shell(SEC_056_NEVER, PROBE.format(name="upstream")),
            shell(SEC_056_NEVER, PROBE.format(name="localhost")),
            text(SEC_056_NEVER, "Probed after configuration."),
            # SEC-055: keyed calls across a gateway restart.
            shell(SEC_055, "HOST={} python3 across.py".format(self.upstream)),
            text(SEC_055, "The calls across the restart are done."),
            # SEC-049, Git: the key's own settings inside its prefix; the key sent by hand outside it.
            shell(SEC_049_GIT, GIT_SCRIPT.format(origin=self.address, authority=self.address[len("https://"):])),
            text(SEC_049_GIT, "The Git work is done."),
        ]
        # The fake model picks a script by a phrase in the task's first message, so no phrase may hold another.
        phrases = {response["when"] for response in responses}
        assert not [(a, b) for a in phrases for b in phrases if a != b and a.lower() in b.lower()], phrases
        for response in responses:
            response["usage"] = {"prompt_tokens": 10, "completion_tokens": 10}
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json",
                      json.dumps({"model": "fixture", "select": "prompt", "responses": responses}))
        storage = Path(self.env["TQA_STORAGE_ROOT"])
        compose.write(storage / "fixture-ca.pem", (self.directory / "fixture-ca.pem").read_text())
        self.ca_file = str(storage / "fixture-ca.pem")

    def check_config(self):
        """The base prepare wrote `[egress]` with the fixture authority (`self.ca_file`); the templates go in here,
        before the file is checked and any service starts."""
        compose.ownership(self.directory, self.env)
        config = self.directory / "gateway.toml"
        text = config.read_text()
        # The file's entry wins over the built-in of that name, which follows whatever `tqa-runtime:workstation` names
        # on this engine host (an older build here, without the install CA's bundle) and would become the default
        # while the cases run. Every task runs the image under test.
        # Once the file names templates it must name the default too: Minimal, as the stack has without the file's list.
        for name, label, default in (("minimal", "Minimal", "true"), ("workstation", "Workstation", "false")):
            text += ("\n[[templates]]\nname={}\ndisplay_name={}\nimage={}\ncapabilities=[\"shell\"]\n"
                     "default={}\n").format(
                json.dumps(name), json.dumps(label), json.dumps(self.runtime), default)
        # Agent teams on (off by default), for SEC-046's team case.
        text += "\n[agent_teams]\nenabled = true\n"
        compose.write(config, text)
        compose.ownership(self.directory, self.env, "1000:1000")
        super().check_config()

    def start(self):
        self.sign_in()
        self.add_key()

    def sign_in(self):
        """The administrator, bootstrapped and signed in; their id in `user`. A subclass that adds the key another way
        (the web suite, through the browser) calls this alone."""
        password = "synthetic-host-keys-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": "host-keys@example.invalid", "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.user = self.request("/api/v1/auth/login", account)["id"]

    def add_key(self):
        """The key `NAME` for the keyed host's exact address, with this run's value; the answer in `key`."""
        self.key = self.request("/api/v1/host-keys", {"name": NAME, "address": self.address, "scheme": "bearer",
                                                      "value": self.value})

    def task(self, title, words, prepare=None, workspace=None):
        """A new task, its stream open, and its first message sent; returns (conversation, path, stream). `prepare`,
        if given, writes into the task's workspace first, as the base stack seeds its README; `workspace`, if given,
        is the existing workspace the task runs on."""
        body = {"title": title, "model_route": "local"}
        if workspace:
            body["workspace_id"] = workspace
        conversation = self.request("/api/v1/conversations", body)["id"]
        path = "/api/v1/conversations/" + conversation
        if prepare:
            compose.ownership(self.directory, self.env)
            workspace = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / conversation
            workspace.mkdir(parents=True, exist_ok=True)
            prepare(workspace)
            compose.ownership(self.directory, self.env, "1000:1000")
        stream = self.request(path + "/events?stream=1&protocol=1", after=0)
        self.request(path + "/messages", {"text": words, "attachments": []})
        return conversation, path, stream

    def container(self, conversation):
        """The task's sandbox container, checked to run the image under test."""
        found = self.command(["docker", "ps", "-q", "--filter", "label=tqa.conversation=" + conversation]).split()
        assert len(found) == 1, "one running sandbox for the task: {}".format(found)
        image = self.command(["docker", "inspect", "--format", "{{.Image}}", found[0]]).strip()
        assert image == self.runtime, "the task runs the image under test, not {}".format(image)
        return found[0]

    def received(self):
        """What the keyed host received, one Authorization per request, in order."""
        logs = self.command(self.configuration + ["logs", "--no-color", "--no-log-prefix", "upstream"])
        return [json.loads(line.split(" ", 1)[1])["authorization"] for line in logs.splitlines()
                if line.startswith("received ")]

    def git_requests(self):
        """The keyed host's Git requests, in order: method, path, and whether the credential was the key's value."""
        logs = self.command(self.configuration + ["logs", "--no-color", "--no-log-prefix", "upstream"])
        return [json.loads(line.split(" ", 1)[1]) for line in logs.splitlines() if line.startswith("git ")]

    @contextlib.contextmanager
    def acting(self, client):
        """Requests and streams opened inside go as the person signed in on `client` (their own cookie jar)."""
        own, self.client = self.client, client
        try:
            yield
        finally:
            self.client = own

    def teammate(self):
        """A member invited and signed in on their own client; returns (their id, the client)."""
        invitation = self.request("/api/v1/invitations", {"email": TEAMMATE, "role": "member"})
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        password = "synthetic-teammate-" + secrets.token_hex(16)
        with self.acting(client):
            joined = self.request("/api/v1/invitations/accept", {"token": invitation["token"], "password": password})
        return joined["id"], client

    def redact(self, text):
        # Before the overlay there is no value yet: a failure that early still reports its own error.
        text = compose.redact(text, self.env)
        value = getattr(self, "value", None)
        return text.replace(value, "<the key's value>") if value else text


# How long a run's stream may go without a frame: one tool call sends none while it runs, and a case's longest (25
# keyed requests, or every client in turn) takes past the stream's default 30 s on a loaded host.
RUN_PATIENCE = 120


def finished(stack, path, stream, runs=1):
    """The task's stored events once `runs` more runs have completed on `stream`, counted from this call."""
    deadline, completed = time.monotonic() + 180, 0
    stream.patience = max(stream.patience, RUN_PATIENCE)
    while time.monotonic() < deadline:
        event = stream.next().get("event", {})
        assert event.get("type") != "run.failed", "a keyed run failed: {}".format(event)
        if event.get("type") == "run.completed":
            completed += 1
            if completed == runs:
                return stack.request(path + "/events?after=0&limit=1000")["items"]
    raise AssertionError("the keyed runs did not complete")


def results(stored):
    return [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]


def sec_047(stack, stored):
    """R2: the host got the value; the task saw only the placeholder; the value is nowhere else."""
    calls = results(stored)
    assert len(calls) == 2 and all(call["status"] == "ok" for call in calls), calls
    echo, environment = calls[0]["content"], calls[1]["content"]
    placeholder = environment.strip().split("=", 1)[1] if "=" in environment else ""
    assert placeholder.startswith("tqa_ph_"), "the run holds the placeholder: " + environment
    assert "Bearer " + placeholder in echo, "the host's echo is scrubbed back to the placeholder: " + echo
    assert stack.received() == ["Bearer " + stack.value], "the host received the value, once (the proxy swapped it)"
    leaks = []
    if stack.value in json.dumps(stored):
        leaks.append("the task's events")
    for service in ("gateway", "fake-model", "postgres"):
        if stack.value in stack.command(stack.configuration + ["logs", "--no-color", service]):
            leaks.append(service + " log")
    sandbox = stack.container(stack.sec_047)
    if stack.value in stack.command(["docker", "logs", sandbox]):
        leaks.append("the runtime's log")
    if stack.value in stack.command(["docker", "exec", sandbox, "env"]):
        leaks.append("the sandbox's environment")
    found = stack.command(["docker", "exec", sandbox, "sh", "-c",
                           "grep -rlF -- '{}' /workspace /run/tqa /tmp /home 2>/dev/null || true".format(stack.value)])
    if found.strip():
        leaks.append("sandbox files: " + found.strip())
    dump = stack.command(stack.configuration + ["exec", "-T", "postgres", "pg_dump", "-U", "tqa", "-d", "gateway",
                                                "--data-only"])
    if stack.value in dump:
        leaks.append("a plaintext table")
    assert not leaks, "the value leaked into: " + ", ".join(leaks)
    print("SEC-047: the host received the value once; the task saw only the placeholder (its echo scrubbed); the "
          "value is in no event, log, sandbox file or environment, or plaintext table", flush=True)


def sec_048(stack):
    """R3: a placeholder from the task's earlier run reads `expired`, and one copied from another task
    `foreign_sandbox`; neither reaches the host. (`foreign_run` needs two runs of one task at once, which a task never
    has; the rule's own tests cover it, GW-U-110.)"""
    before = len(stack.received())
    conversation, path, stream = stack.task("Keeps a placeholder", SEC_048)
    finished(stack, path, stream)
    stack.container(conversation)
    first = results(stack.request(path + "/events?after=0&limit=1000")["items"])
    assert first[0]["status"] == "ok" and "status 200" in first[0]["content"], first
    stack.request(path + "/messages", {"text": "Now use the old one.", "attachments": []})
    stored = finished(stack, path, stream)
    old = results(stored)[-1]["content"]
    assert "status 403" in old and "This placeholder's run has ended." in old, old
    # Another task, its own sandbox, with the first task's placeholder copied into its workspace.
    placeholder = stack.command(["docker", "exec", stack.container(conversation), "cat", "/workspace/old-placeholder"])
    write = lambda workspace: compose.write(workspace / "foreign-placeholder", placeholder)
    _, other_path, other_stream = stack.task("Tries another task's placeholder", SEC_048_OTHER, prepare=write)
    foreign = results(finished(stack, other_path, other_stream))[-1]["content"]
    assert "status 403" in foreign and "This placeholder belongs to another task." in foreign, foreign
    assert stack.received()[before:] == ["Bearer " + stack.value], "only the first run's own call reached the host"
    print("SEC-048: an earlier run's placeholder read expired and another task's foreign_sandbox; neither reached "
          "the host", flush=True)


def sec_049(stack):
    """R4 and R5: the task's list decides each request, not only each connection; every use is a counted row.
    A connection open when the host's rule is removed has its next request refused (`not_allowed`), and only the first
    reaches the host. Then the key's uses across every case, refusals included, are counted by outcome."""
    before = len(stack.received())
    write = lambda workspace: compose.write(workspace / "hold.py", HOLD)
    conversation, path, stream = stack.task("Holds a keyed connection", SEC_049, prepare=write)
    deadline = time.monotonic() + 120
    while True:
        assert time.monotonic() < deadline, "the held connection's first request never finished"
        found = stack.command(["docker", "ps", "-q", "--filter", "label=tqa.conversation=" + conversation]).split()
        if found and stack.command(["docker", "exec", found[0], "sh", "-c",
                                    "test -f /workspace/first-done && echo yes || true"]).strip() == "yes":
            break
        time.sleep(0.5)
    rule = [item for item in stack.request("/api/v1/admin/egress")["items"] if item["host"] == stack.upstream]
    assert len(rule) == 1, "one rule for the keyed host: {}".format(rule)
    stack.request("/api/v1/admin/egress/{}?version={}".format(stack.upstream, rule[0]["version"]), method="DELETE")
    stack.command(["docker", "exec", stack.container(conversation), "touch", "/workspace/go"])
    output = results(finished(stack, path, stream))[-1]["content"]
    assert "first 200" in output, output
    assert "second 403" in output and "This site isn't on this task's allowed sites any more." in output, output
    assert len(stack.received()) == before + 1, "only the first request reached the host"
    uses = {}
    for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(stack.key["id"]))["items"]:
        assert row["consumer_kind"] == "host" and row["conversation_id"], row
        uses[row["outcome"]] = uses.get(row["outcome"], 0) + row["count"]
    expected = {"ok": 3, "expired": 1, "foreign_sandbox": 1, "not_allowed": 1}
    assert uses == expected, "the key's uses by outcome: {} (expected {})".format(uses, expected)
    print("SEC-049: a request on a connection open when its host's rule went was refused not_allowed and never reached "
          "the host; every use of the key, refusals included, is a counted row (ok 3, expired, foreign_sandbox and "
          "not_allowed 1 each)", flush=True)


GIT_SCRIPT = """set -e
git clone -q {origin}/team/repo.git team
cd team
git -c user.name=Fixture -c user.email=fixture@example.invalid commit -q --allow-empty -m keyed
git push -q origin HEAD:refs/heads/keyed
echo "pushed $(git ls-remote origin refs/heads/keyed | wc -l)"
git push -q "https://git:$%s@{authority}/other/repo.git" HEAD:refs/heads/keyed 2>../other.err \\
  || echo "outside: $(grep -o 'error: [0-9]*' ../other.err | head -1)"
""" % GIT_NAME


def run_of(stored, after=0):
    """The run id of the task's run that started after `after` stored frames (numbered within its task)."""
    started = [frame["event"]["run_id"] for frame in stored[after:]
               if frame.get("event", {}).get("type") == "run.started"]
    assert len(started) == 1, "one run started: {}".format(started)
    return started[0]


def postgres(stack, statement):
    """One statement on the stack's own database, as its owner."""
    stack.command(stack.configuration + ["exec", "-T", "postgres", "psql", "-v", "ON_ERROR_STOP=1", "-U", "tqa", "-d",
                                         "gateway", "-c", statement])


def sec_049_count(stack):
    """R4, as the ADR's row: 25 keyed requests in one run are one `ok` row counting 25, with the run, the sender and
    the host; then, with key uses unrecordable (a constraint refusing only `host` rows, the rest of the audit
    untouched), the next run's request is refused `unrecorded` in words and nothing reaches the host. The keyed host's
    rule, removed by SEC-049's first case, comes back first."""
    stack.request("/api/v1/admin/egress/" + stack.upstream, {"ports": [8443], "version": None}, method="PUT")
    before = len(stack.received())
    conversation, path, stream = stack.task("Calls many times", SEC_049_COUNT)
    stored = finished(stack, path, stream)
    counted = results(stored)[-1]["content"]
    assert counted.split() == ["25", "status", "200"], counted
    run = run_of(stored)
    rows = [row for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(stack.key["id"]))["items"]
            if (row["conversation_id"], row["run_id"]) == (conversation, run)]
    assert len(rows) == 1, rows
    assert (rows[0]["outcome"], rows[0]["count"], rows[0]["consumer"]) == ("ok", 25, stack.upstream + ":8443"), rows
    assert rows[0]["user_id"] == stack.user, rows
    assert len(stack.received()) == before + 25, "the host received each of the 25"
    postgres(stack, "ALTER TABLE secret_reads ADD CONSTRAINT sec_049_unrecorded CHECK (consumer_kind <> 'host') "
                    "NOT VALID")
    try:
        stack.request(path + "/messages", {"text": "Once more.", "attachments": []})
        stored = finished(stack, path, stream)
    finally:
        postgres(stack, "ALTER TABLE secret_reads DROP CONSTRAINT sec_049_unrecorded")
    refused = results(stored)[-1]["content"]
    assert "status 403" in refused and "its use couldn't be recorded" in refused, refused
    assert len(stack.received()) == before + 25, "nothing reached the host while uses couldn't be recorded"
    print("SEC-049 (R4): 25 keyed requests in one run are one ok row counting 25 with its run, sender and host; with "
          "uses unrecordable the next request was refused unrecorded and nothing reached the host", flush=True)


def sec_049_later(stack):
    """R5, as the ADR's row: a key for a host and port not on the task's list gives a run no placeholder; once the
    administrator allows it, the task's next run has one."""
    address = "https://{}:{}".format(stack.upstream, LATER_PORT)
    stack.request("/api/v1/host-keys", {"name": LATER_NAME, "address": address, "scheme": "bearer",
                                        "value": "tqa-g9-later-" + secrets.token_hex(16)})
    _, path, stream = stack.task("Looks for a later key", SEC_049_LATER)
    first = results(finished(stack, path, stream))[-1]["content"]
    assert first.strip() == "0", "no placeholder for a port not on the list: " + first
    rule = [item for item in stack.request("/api/v1/admin/egress")["items"] if item["host"] == stack.upstream]
    assert len(rule) == 1, rule
    allowed = {"ports": [8443, LATER_PORT], "version": rule[0]["version"]}
    stack.request("/api/v1/admin/egress/" + stack.upstream, allowed, method="PUT")
    stack.request(path + "/messages", {"text": "Look again.", "attachments": []})
    second = results(finished(stack, path, stream))[-1]["content"]
    assert second.strip() == "1", "the next run after it was allowed has its placeholder: " + second
    print("SEC-049 (R5): a key for a port not on the list gave no placeholder; the next run after it was allowed had "
          "one", flush=True)


def allow_keyed_port(stack, port=8443):
    """The keyed host's rule allows `port` again, whatever the cases before left of it."""
    rule = [item for item in stack.request("/api/v1/admin/egress")["items"] if item["host"] == stack.upstream]
    if rule and port in rule[0]["ports"]:
        return
    ports = sorted(set((rule[0]["ports"] if rule else []) + [port]))
    stack.request("/api/v1/admin/egress/" + stack.upstream,
                  {"ports": ports, "version": rule[0]["version"] if rule else None}, method="PUT")


def sec_046(stack):
    """R1 and R6, as the ADR's row: a member's key serves only that member's runs. The owner (the administrator) has a
    key of their own and a shared key granted to them only; a teammate, invited, has their own key for the same host
    (so their calls to it go through the keyed proxy, a first-slice limit). The owner's task on a workspace shared with
    the teammate leaves its placeholder in a file; the teammate's task on that workspace holds none of the owner's keys,
    and its call with the owner's placeholder is refused `foreign_sandbox` in words, a row with the teammate as who,
    and nothing reaches the host. The owner's own call succeeds. The teammate's own task's key uses show their row
    without the key's name. Team runs are [`sec_046_team`]."""
    allow_keyed_port(stack)
    owner_key = stack.request("/api/v1/host-keys", {"name": OWNER_NAME, "address": stack.address, "scheme": "bearer",
                                                    "value": stack.value})
    stack.request("/api/v1/admin/host-keys", {"name": SHARED_NAME, "address": stack.address, "scheme": "bearer",
                                              "value": stack.value, "users": [stack.user]})
    teammate, client = stack.teammate()
    with stack.acting(client):
        stack.request("/api/v1/host-keys", {"name": TEAMMATE_NAME, "address": stack.address, "scheme": "bearer",
                                            "value": "tqa-g9-teammate-" + secrets.token_hex(16)})
    workspace = stack.request("/api/v1/workspaces", {"name": "Shared with a teammate"})["id"]
    stack.request("/api/v1/workspaces/{}/shares".format(workspace), {"email": TEAMMATE, "access": "write"},
                  method="PUT")
    before = len(stack.received())
    _, path, stream = stack.task("Leaves a key in the shared workspace", SEC_046_OWNER, workspace=workspace)
    own = results(finished(stack, path, stream))[-1]["content"]
    assert "status 200" in own, "the owner's own call succeeds: " + own
    assert stack.received()[before:] == ["Bearer " + stack.value], "the owner's call reached the host, swapped"
    with stack.acting(client):
        conversation, path, stream = stack.task("Uses what a teammate left", SEC_046_TEAMMATE, workspace=workspace)
        calls = results(finished(stack, path, stream))
        uses = stack.request(path + "/key-uses")["items"]
    held, used = calls[-2]["content"], calls[-1]["content"]
    expected = "{}=0 {}=0 {}=1".format(OWNER_NAME, SHARED_NAME, TEAMMATE_NAME)
    assert held.split() == expected.split(), "the teammate's run holds only their own key: " + held
    assert "status 403" in used and "This placeholder belongs to another task." in used, used
    assert len(stack.received()) == before + 1, "nothing of the teammate's call reached the host"
    rows = [row for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(owner_key["id"]))["items"]
            if row["conversation_id"] == conversation]
    assert [(row["outcome"], row["user_id"], row["count"]) for row in rows] == [("foreign_sandbox", teammate, 1)], rows
    assert [(row["outcome"], row["sender_id"], row["name"]) for row in uses] == [("foreign_sandbox", teammate, None)], \
        uses
    print("SEC-046 (R1, R6): on a shared workspace the teammate's run held none of the owner's keys; its call with the "
          "owner's placeholder was refused foreign_sandbox in words, a row with the teammate as who, nothing reached "
          "the host, and their key uses name no key; the owner's own call succeeded", flush=True)
    return {"owner_key": owner_key, "workspace": workspace, "teammate": teammate, "client": client}


def helper_of(stack, lead_path, role, lead_stored):
    """The path of the lead's helper named `role`, and its stored events once its run has completed; a failure says
    what the lead's calls answered."""
    deadline, kinds = time.monotonic() + 180, []
    while True:
        team = stack.request(lead_path + "/team")
        found = [helper for helper in team["helpers"] if helper["name"] == role]
        assert time.monotonic() < deadline, "the {} helper's run never completed: the team {} ({}), its events {}, " \
            "the lead's results {}".format(role, team["state"], team.get("stop_reason"), kinds,
                                           [result["content"][:200] for result in results(lead_stored)])
        if found:
            path = "/api/v1/conversations/" + found[0]["conversation"]
            stored = stack.request(path + "/events?after=0&limit=1000")["items"]
            kinds = [frame.get("event", {}).get("type") for frame in stored]
            assert "run.failed" not in kinds, "the {} helper's run failed: {}".format(role, kinds)
            if "run.completed" in kinds:
                return path, stored
        time.sleep(1)


def sec_046_team(stack, shared):
    """SEC-046's team case (ADR-0034; ADR-0037 §2). In v1.1 a team's helpers are its person's own agents, and each
    helper's turn is sent as that person, so a helper's run gets the same keys as its lead's run, and no other person's.
    The owner leads a team on their own workspace: the lead leaves its placeholder in a file, and a helper on a copy of
    those files holds the owner's keys; its call with the lead's placeholder is refused `foreign_sandbox` (another
    sandbox's), with nothing reaching the host, and its call with its own placeholder succeeds. The teammate leads a
    team on the workspace the owner shared: their helper, on those files, holds none of the owner's keys, and its call
    with the placeholder the owner's run left there is refused `foreign_sandbox`, a row with the teammate as who."""
    owner_key, teammate, client = shared["owner_key"], shared["teammate"], shared["client"]
    reads = "/api/v1/admin/host-keys/{}/reads?limit=100".format(owner_key["id"])
    before = len(stack.received())
    _, lead, stream = stack.task("Leads a keyed team", SEC_046_OWNER_LEAD)
    path, stored = helper_of(stack, lead, KEYHOLDER, finished(stack, lead, stream, runs=2))
    calls = [result["content"] for result in results(stored)]
    held, foreign, own = calls[0], calls[1], calls[2]
    expected = "{}=1 {}=1 {}=0".format(OWNER_NAME, SHARED_NAME, TEAMMATE_NAME)
    assert held.split() == expected.split(), "the owner's helper holds the owner's keys, as its lead: " + held
    assert "status 403" in foreign and "This placeholder belongs to another task." in foreign, foreign
    assert "status 200" in own, "the helper's own placeholder works: " + own
    assert stack.received()[before:] == ["Bearer " + stack.value], "only the helper's own call reached the host"
    conversation = path.rsplit("/", 1)[1]
    rows = sorted((row["outcome"], row["user_id"], row["count"]) for row in stack.request(reads)["items"]
                  if row["conversation_id"] == conversation)
    assert rows == [("foreign_sandbox", stack.user, 1), ("ok", stack.user, 1)], rows
    before = len(stack.received())
    with stack.acting(client):
        _, lead, stream = stack.task("Leads a team on the shared files", SEC_046_TEAMMATE_LEAD,
                                     workspace=shared["workspace"])
        path, stored = helper_of(stack, lead, BORROWER, finished(stack, lead, stream, runs=2))
    calls = [result["content"] for result in results(stored)]
    held, used = calls[0], calls[1]
    expected = "{}=0 {}=0 {}=1".format(OWNER_NAME, SHARED_NAME, TEAMMATE_NAME)
    assert held.split() == expected.split(), "the teammate's helper holds only the teammate's key: " + held
    assert "status 403" in used and "This placeholder belongs to another task." in used, used
    assert len(stack.received()) == before, "nothing of the teammate's helper's call reached the host"
    conversation = path.rsplit("/", 1)[1]
    rows = [(row["outcome"], row["user_id"], row["count"]) for row in stack.request(reads)["items"]
            if row["conversation_id"] == conversation]
    assert rows == [("foreign_sandbox", teammate, 1)], rows
    print("SEC-046, team runs (ADR-0034): the owner's helper held the owner's keys as its lead did, its call with "
          "the lead's placeholder was refused foreign_sandbox with nothing sent, and its own call succeeded; the "
          "teammate's helper on the shared files held none of the owner's keys, and its call with the owner's "
          "placeholder was refused foreign_sandbox, a row with the teammate as who", flush=True)


def wait_for_file(stack, conversation, name, seconds=120):
    """Until the task's sandbox has `name` in its workspace."""
    deadline = time.monotonic() + seconds
    while True:
        assert time.monotonic() < deadline, "the run never wrote " + name
        found = stack.command(["docker", "ps", "-q", "--filter", "label=tqa.conversation=" + conversation]).split()
        if found and stack.command(["docker", "exec", found[0], "sh", "-c",
                                    "test -f /workspace/{} && echo yes || true".format(name)]).strip() == "yes":
            return
        time.sleep(0.5)


def sec_055(stack):
    """A gateway restart during a keyed call (ADR-0037's supporting tests). A run holds a keyed
    connection and has made one call; the gateway stops (the task's stream ends with `restarting`) and starts again
    (healthy, the task's stream reopened). The held connection ends with the gateway's process, never a hang or an
    answer from nowhere; a fresh connection's call with the same placeholder, made at once, is swapped and answered:
    the proxy knows the sandbox and its run from the store, not from the runtime's link. The same runtime goes on and
    the run completes. The host received each call once; the key's uses count both."""
    allow_keyed_port(stack)
    before = len(stack.received())
    write = lambda workspace: compose.write(workspace / "across.py", ACROSS)
    conversation, path, stream = stack.task("Calls across a restart", SEC_055, prepare=write)
    wait_for_file(stack, conversation, "first-done")
    sandbox = stack.container(conversation)
    stack.command(stack.configuration + ["stop", "--timeout", "10", "gateway"])
    stream.notice("restarting")
    stack.up()
    replay = stack.request(path + "/events?stream=1&protocol=1", after=stream.last)
    stack.command(["docker", "exec", sandbox, "touch", "/workspace/go"])
    output = results(finished(stack, path, replay))[-1]["content"]
    assert "first 200" in output and "held ended" in output, output
    assert "fresh 200" in output and "Bearer tqa_ph_" in output, "the fresh call was swapped and scrubbed: " + output
    assert stack.container(conversation) == sandbox, "the same runtime went on after the restart"
    assert stack.received()[before:] == ["Bearer " + stack.value] * 2, "the host got each call once, swapped"
    owner = [key for key in stack.request("/api/v1/host-keys")["items"] if key["name"] == OWNER_NAME]
    rows = [row for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(owner[0]["id"]))["items"]
            if row["conversation_id"] == conversation]
    assert [(row["outcome"], row["count"]) for row in rows] == [("ok", 2)], rows
    print("SEC-055: across a gateway restart the held keyed connection ended, and a fresh call with the run's "
          "placeholder, made at once, was swapped and answered; the same runtime completed the run; the host got each "
          "call once and the key counts both", flush=True)


def sbx_c_019(stack):
    """SBX-C-019, the trust matrix on Docker: curl, Python's urllib, requests (pip's own copy: the image has no
    other), pip and Node each trust the install CA from the bundle the run's shell is given and reach the keyed host
    with the value swapped in; git does so in SEC-049. A port of the same host that no key is for is a plain tunnel:
    the client sees the host's own certificate (the fixture's authority, not the install CA), which the sandbox doesn't
    trust. A keyed host whose certificate fails verification is refused, never a fallback: GW-I-166."""
    allow_keyed_port(stack)
    allow_keyed_port(stack, UNKEYED_PORT)
    before = len(stack.received())
    _, path, stream = stack.task("Checks each client", SBX_C_019)
    output = results(finished(stack, path, stream))[-1]["content"]
    lines = dict(line.split(" ", 1) for line in output.strip().splitlines() if " " in line)
    for client in ("curl", "urllib", "requests", "node"):
        assert lines.get(client) == "200", "{} reaches the keyed host, swapped: {}".format(client, output)
    assert lines.get("pip") == "tls-ok", "pip trusts the install CA: " + output
    # The four keyed calls carried the value; pip's index request and the unkeyed probe carried none.
    since = stack.received()[before:]
    assert since.count("Bearer " + stack.value) == 4, "four keyed calls, each swapped"
    assert all(header in ("", "Bearer " + stack.value) for header in since), "no other credential reached the host"
    # curl's own error comes first on the line (its status, 000, last): verification failed on the certificate.
    assert "certificate" in lines.get("unkeyed-verify", ""), "the sandbox doesn't trust an unkeyed port: " + output
    # The installation is the containers' while they run: take it back to read the fixture's authority, then return it.
    compose.ownership(stack.directory, stack.env)
    try:
        authority = stack.command(["openssl", "x509", "-noout", "-subject", "-nameopt", "RFC2253", "-in",
                                   str(stack.directory / "fixture-ca.pem")]).strip().removeprefix("subject=")
    finally:
        compose.ownership(stack.directory, stack.env, "1000:1000")
    issuer = lines.get("unkeyed-issuer", "")
    assert authority and authority.split(",")[0] in issuer, \
        "the unkeyed port shows the host's own certificate ({}): {}".format(authority, output)
    print("SBX-C-019: curl, urllib, requests, pip and Node trusted the install CA and reached the keyed host swapped; "
          "an unkeyed port showed the host's own certificate and wasn't trusted", flush=True)


def internal_rule(stack, host, internal):
    """The rule for `host` on 8443, an internal server's or a plain one."""
    existing = [item for item in stack.request("/api/v1/admin/egress")["items"] if item["host"] == host]
    body = {"ports": [8443], "internal": internal}
    if existing:
        body["version"] = existing[0]["version"]
    stack.request("/api/v1/admin/egress/" + host, body, method="PUT")


def probes(stack, title, phrase):
    """The proxy's status lines for the probes of a new task's run."""
    _, path, stream = stack.task(title, phrase)
    return [result["content"].strip() for result in results(finished(stack, path, stream))]


def sec_056(stack):
    """SEC-056 (on the stack, ADR-0035 amendment): named internal servers on the release images and the real engine.
    The fixture by its service name resolves to its address on a Docker network: a plain rule is refused as private;
    an internal rule is refused while `internal_never` is missing; once it is set (the gateway restarted to read it),
    the same rule is still refused because the address is on one of the engine's networks, read from the engine
    itself, and an internal rule for `localhost` is refused as a reserved address, as any rule's is. Nothing reaches
    the fixture. Being reached needs a server on another machine; in-process SEC-050..054 hold it."""
    before = len(stack.received())
    internal_rule(stack, "upstream", False)
    plain = probes(stack, "Reaches a private name", SEC_056_PLAIN)
    assert len(plain) == 1 and "private or reserved address" in plain[0], plain
    internal_rule(stack, "upstream", True)
    unset = probes(stack, "Reaches an internal name too soon", SEC_056_UNSET)
    assert len(unset) == 1 and "internal servers are off here" in unset[0], unset
    compose.ownership(stack.directory, stack.env)
    config = stack.directory / "gateway.toml"
    text = config.read_text()
    assert text.count("\n[egress]\n") == 1, "one [egress] table"
    compose.write(config, text.replace("\n[egress]\n", "\n[egress]\ninternal_never=" + json.dumps([INTERNAL_NEVER])
                                       + "\n", 1))
    compose.ownership(stack.directory, stack.env, "1000:1000")
    stack.command(stack.configuration + ["stop", "--timeout", "10", "gateway"])
    stack.up()
    internal_rule(stack, "localhost", True)
    never = probes(stack, "Reaches internal names", SEC_056_NEVER)
    # The engine's network is this machine's own ground; loopback is a reserved address, refused as any rule's is.
    assert len(never) == 2 and "internal servers never use here" in never[0], never
    assert "private or reserved address" in never[1], never
    assert len(stack.received()) == before, "nothing reached the fixture"
    print("SEC-056: on the release images a private name was refused plainly, an internal rule refused until "
          "internal_never was set, and then refused for the engine's network and for loopback; nothing reached the "
          "fixture", flush=True)


def sec_049_git(stack):
    """R5 for Git, as the ADR's row: a `basic` key for a path prefix clones and pushes inside it with only the
    settings the run is given (git's credential helper for that prefix), and a push outside it with the placeholder as
    the URL's password is refused `wrong_host`; the value never reaches the host outside the prefix. Git asks without
    a credential first, so the host does see unauthorised requests; none outside the prefix carries the value. The
    first key goes, so the prefix key is the host's only one."""
    stack.request("/api/v1/host-keys/{}?version={}".format(stack.key["id"], stack.key["version"]), method="DELETE")
    key = stack.request("/api/v1/host-keys", {"name": GIT_NAME, "address": stack.address + "/team",
                                              "scheme": "basic", "user_name": "git", "value": stack.value})
    before = len(stack.git_requests())
    _, path, stream = stack.task("Clones and pushes", SEC_049_GIT)
    output = results(finished(stack, path, stream))[-1]["content"]
    assert "pushed 1" in output, "{}\nthe host's Git requests: {}".format(output, stack.git_requests()[before:])
    assert "outside: error: 403" in output, output
    requests = stack.git_requests()[before:]
    authorised = [request for request in requests if request["authorised"]]
    assert authorised and all(request["path"].startswith("/team/") for request in authorised), requests
    assert any(request["path"].endswith("/git-receive-pack") and request["method"] == "POST"
               for request in authorised), requests
    assert all(not request["authorised"] for request in requests if request["path"].startswith("/other/")), requests
    uses = {}
    for row in stack.request("/api/v1/admin/host-keys/{}/reads?limit=100".format(key["id"]))["items"]:
        uses[row["outcome"]] = uses.get(row["outcome"], 0) + row["count"]
    assert uses.get("wrong_host") == 1 and uses.get("ok", 0) >= 3 and set(uses) == {"ok", "wrong_host"}, uses
    print("SEC-049 (Git): a basic key for /team cloned and pushed with the run's own settings ({} authorised requests, "
          "all inside the prefix); a push to /other with the placeholder was refused wrong_host and the value never "
          "reached the host outside the prefix".format(len(authorised)), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("Host-keys fixture interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-host-keys-")
    directory = compose.installation(temporary)
    stack, failure = HostKeyStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("host-keys")
        stack.start()
        stack.sec_047, path, stream = stack.task("Host keys", SEC_047)
        sec_047(stack, finished(stack, path, stream))
        sec_048(stack)
        sec_049(stack)
        sec_049_count(stack)
        sec_049_later(stack)
        sec_049_git(stack)
        sec_046_team(stack, sec_046(stack))
        sec_055(stack)
        sbx_c_019(stack)
        sec_056(stack)
    except BaseException as error:
        failure = error
        print("Host-keys contract failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
