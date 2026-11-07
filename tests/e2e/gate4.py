"""E2E-TEAM-011: the Gate 4 demo as an automated scenario over the public API (DEV-PLAN Phase 4).

Two users; A shares a workspace with B; B's run waits for the lease that A's run holds, B forks and runs in the fork
in parallel, and both push a new branch with plain git and their own host key (ADR-0037) to a local authenticated
origin. The origin, its CA, the egress rule and the credential search are the RT-I-016 push fixture's.

Run from the repository root: TQA_DOCKER_TESTS=1 python3 tests/e2e/gate4.py
"""

from datetime import datetime, timezone
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "crates" / "tqa-sandbox" / "tests"))
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
from egress.git_push import PushStack  # noqa: E402  (the RT-I-016 fixture, reused read-only)
from egress.push_checks import receive_packs, sandbox_run, storage_hits  # noqa: E402
from egress.stack import compose  # noqa: E402
import robustness  # noqa: E402
import gate4_evidence as evidence  # noqa: E402

DEADLINE_SECONDS = 600
RESPONSE_BYTES = 1048576
STREAM_PATIENCE = 180


class User:
    """One account: a cookie session for the session-only routes, then its own Bearer token for everything else."""

    def __init__(self, stack, email):
        self.stack, self.email = stack, email
        self.password = secrets.token_urlsafe(24)
        self.session = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.bearer = urllib.request.build_opener()
        self.token = None

    def _send(self, opener, method, path, body, headers, timeout=15):
        headers = dict(headers, **{"Content-Type": "application/json"})
        if method != "GET":
            headers["Idempotency-Key"] = secrets.token_hex(16)
        request = urllib.request.Request(self.stack.origin + path, method=method, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        with opener.open(request, timeout=timeout) as response:
            data = response.read(RESPONSE_BYTES + 1)
            assert len(data) <= RESPONSE_BYTES, "A response exceeded its bound"
            return json.loads(data) if data else None

    def cookie(self, method, path, body=None):
        return self._send(self.session, method, path, body, {"Origin": self.stack.origin})

    def api(self, method, path, body=None):
        return self._send(self.bearer, method, "/api/v1" + path, body, {"Authorization": "Bearer " + self.token})

    def sign_in(self):
        self.cookie("POST", "/api/v1/auth/login", {"email": self.email, "password": self.password})
        self.token = self.cookie("POST", "/api/v1/tokens", {"name": "gate4", "expires_in_days": 1})["token"]
        # The person's own host key for the origin (ADR-0037); the retired Git credentials answer 410.
        self.cookie("POST", "/api/v1/host-keys", {"name": evidence.KEY, "address": "https://{}:8443".format(
            self.stack.upstream), "scheme": "bearer", "value": self.stack.secret})

    def conversation(self, workspace, title, text):
        """Creates a conversation on the workspace, sends the task, and opens its event stream from the start."""
        conversation = self.api("POST", "/conversations", {"title": title, "model_route": "local",
                                                           "workspace_id": workspace})["id"]
        self.api("POST", "/conversations/{}/messages".format(conversation), {"text": text, "attachments": []})
        request = urllib.request.Request(
            self.stack.origin + "/api/v1/conversations/{}/events?stream=1&protocol=1".format(conversation),
            headers={"Authorization": "Bearer " + self.token, "Last-Event-ID": "0"})
        stream = robustness.Stream(self.bearer.open(request, timeout=robustness.STREAM_READ_TIMEOUT), 0)
        stream.patience = STREAM_PATIENCE
        self.stack.streams.append(stream)
        self.stack.tracked = getattr(self.stack, "tracked", []) + [(self, conversation)]
        return conversation, stream

    def stored(self, conversation):
        return self.api("GET", "/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]

    def ready_workspace(self, workspace):
        deadline = time.monotonic() + 120
        while True:
            view = self.api("GET", "/workspaces/" + workspace)
            if view["state"] == "ready":
                return view
            assert view["state"] != "failed" and time.monotonic() < deadline, "The fork did not become ready"
            time.sleep(0.5)


class Gate4Stack(PushStack):
    def scenario(self):
        """The RT-I-016 origin and CA, with this demo's replies and the wait policy."""
        super().scenario()
        compose.write(self.directory / "scenario.json", json.dumps(evidence.replies()))
        config = self.directory / "gateway.toml"
        compose.write(config, config.read_text() + "\n[workspaces]\nbusy_policy='wait'\n")
        # The base prepare() appends to gateway.toml next and hands the tree to the gateway user itself; the origin
        # has already loaded its certificate.

    def diagnostics(self):
        super().diagnostics()
        for user, conversation in getattr(self, "tracked", []):
            try:
                kinds = [evidence.kind(frame) for frame in user.stored(conversation)]
                print("Events of {} ({}): {}".format(conversation, user.email, ", ".join(kinds[-40:])), flush=True)
            except Exception as error:
                print("Events unavailable: " + compose.redact(str(error), self.env), flush=True)
        try:
            logs = self.command(self.configuration + ["logs", "--no-color", "gateway"])
            problems = [line for line in logs.splitlines() if '"level":"WARN"' in line or '"level":"ERROR"' in line]
            print(compose.redact("\n".join(problems[-30:]), self.env)[-8000:], flush=True)
        except Exception as error:
            print("Gateway warnings unavailable: " + compose.redact(str(error), self.env), flush=True)

    def container(self, conversation):
        found = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                              "--filter", "label=tqa.conversation=" + conversation]).split()
        assert len(found) == 1, "Expected one live sandbox for the conversation"
        return found[0]

    def users(self):
        a, b = User(self, "owner@example.invalid"), User(self, "member@example.invalid")
        a.cookie("POST", "/api/v1/auth/bootstrap", {"email": a.email, "password": a.password,
                                                    "token": self.env["TQA_BOOTSTRAP_TOKEN"]})
        a.cookie("POST", "/api/v1/auth/login", {"email": a.email, "password": a.password})
        invitation = a.cookie("POST", "/api/v1/invitations", {"email": b.email, "role": "member"})["token"]
        b.cookie("POST", "/api/v1/invitations/accept", {"token": invitation, "password": b.password})
        a.sign_in()
        b.sign_in()
        return a, b


def queued(stream):
    """Reads B's stream until its sandbox is queued behind the workspace lease; a run start first fails the demo."""
    while True:
        frame = stream.next()
        state = (frame.get("sandbox") or {}).get("state") if frame["type"] == "session.ready" else frame.get("state")
        if frame["type"] in ("session.ready", "sandbox.state") and state == "queued":
            reason = (frame.get("queue") or {}).get("reason")
            assert reason in (None, "workspace"), "Queued for another reason: " + str(reason)
            return frame
        assert evidence.kind(frame) != "run.started", "The waiting run started while the lease was held"


def sandbox_clean(stack, container):
    """The RT-I-016 search, with a planted copy first so that an empty result means something."""
    plant = subprocess.run(["docker", "exec", "-i", container, "sh", "-c", "cat > /tmp/planted-control"],
                           input=stack.secret, env=stack.env, text=True, timeout=30)
    assert plant.returncode == 0 and stack.search(container) == ["/tmp/planted-control"], "The search misses a planted copy"
    stack.command(["docker", "exec", container, "rm", "-f", "/tmp/planted-control"])
    assert stack.search(container) == [], "The credential is readable inside a sandbox"


def demo(stack):
    notes = []
    a, b = stack.users()
    notes.append("A (admin) invited B (member); B accepted; each signed in and added their own host key for the origin")
    # The workspace starts as a clone of the origin made outside every sandbox (seed); no git address is needed.
    workspace = a.api("POST", "/workspaces", {"name": "Gate 4 shared"})["id"]
    stack.workspace = workspace
    stack.seed()
    share = a.api("PUT", "/workspaces/{}/shares".format(workspace), {"email": b.email, "access": "write"})
    assert share["access"] == "write", "The share was not granted"
    notes.append("A shared the workspace with B (write)")

    holder, a_stream = a.conversation(workspace, "A: push " + evidence.BRANCH_A, evidence.MESSAGE_A)
    # Its first step (the checkout) completes; its push step is the held model reply, so the run keeps the lease.
    a_stream.until("step.completed")
    a_stream.patience = evidence.HOLD_MS // 1000 + STREAM_PATIENCE
    notes.append("A's run checked out its branch and is held before its push, keeping the workspace lease")

    waiting, b_wait = b.conversation(workspace, "B: shared workspace", evidence.MESSAGE_WAIT)
    frame = queued(b_wait)
    lease = b.api("GET", "/workspaces/" + workspace)["lease"]
    assert lease and lease["title"] is None, "B must see the lease without A's task title"
    notes.append("B's run on the shared workspace is queued ({}); B sees the workspace in use by another task".format(
        json.dumps(frame.get("queue"))))

    fork = b.api("POST", "/workspaces/{}/fork".format(workspace), {"name": "Gate 4 fork"})["id"]
    b.ready_workspace(fork)
    forked, f_stream = b.conversation(fork, "B: push " + evidence.BRANCH_B, evidence.MESSAGE_FORK)
    f_stream.until("run.completed")
    fork_sandbox = stack.container(forked)
    sandbox_clean(stack, fork_sandbox)
    # A copy planted through the live sandbox proves that the host-storage search at the end finds one.
    sandbox_run(stack, fork_sandbox, "cat > /workspace/planted-control", stdin=stack.secret)
    assert storage_hits(stack) == 1, "The storage search misses a planted copy"
    sandbox_run(stack, fork_sandbox, "rm -f /workspace/planted-control")
    notes.append("B forked the workspace and pushed {} from the fork while A's run still held the lease".format(
        evidence.BRANCH_B))
    notes.append("The origin credential is absent from B's pushing sandbox (search verified by a planted copy)")

    # A's push is the held reply: its run ends up to HOLD_MS after it was held, then pushes.
    a_stream.until("run.completed", within=evidence.HOLD_MS // 1000 + STREAM_PATIENCE)
    b_wait.patience = evidence.HOLD_MS // 1000 + STREAM_PATIENCE
    b_wait.until("run.completed", within=evidence.HOLD_MS // 1000 + STREAM_PATIENCE)
    notes.append("A's held push went on and pushed {}; B's queued run then started on the shared workspace".format(
        evidence.BRANCH_A))
    assert storage_hits(stack) == 0, "The credential reached host storage"
    notes.append("The origin credential is absent from host storage: both workspaces, state and journals")

    conversations = [("A on the shared workspace", a.stored(holder)), ("B on the shared workspace", b.stored(waiting)),
                     ("B on the fork", b.stored(forked))]
    return a, b, conversations, notes, {"workspace": workspace, "fork": fork,
                                        "conversations": {"holder": holder, "waiting": waiting, "fork": forked}}


def verify(stack, a, b, conversations, notes, identities):
    (_, held), (_, waited), (_, forked) = conversations
    record = dict(identities, order=evidence.check_order(held, forked, waited),
                  pushes={evidence.BRANCH_A: evidence.check_push(held, evidence.BRANCH_A),
                          evidence.BRANCH_B: evidence.check_push(forked, evidence.BRANCH_B)},
                  refs=evidence.check_refs(stack.upstream_get("/refs"), stack.commit),
                  uploads=len(receive_packs(stack)), notes=notes)
    assert record["uploads"] == 2, "Expected exactly two uploads to the origin"
    text = {"events": json.dumps(conversations), "transcript": evidence.transcript(conversations, notes),
            "result": evidence.result(record)}
    evidence.check_clean(text, [stack.secret, a.password, b.password, a.token, b.token])
    directory = ROOT / "tests" / "e2e" / "results" / ("gate4-" + datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    directory.mkdir(parents=True)
    for (title, stored), name in zip(conversations, ("holder", "waiting", "fork")):
        (directory / "events-{}.jsonl".format(name)).write_text("".join(json.dumps(frame) + "\n" for frame in stored))
    (directory / "transcript.md").write_text(text["transcript"])
    (directory / "result.json").write_text(text["result"])
    print("E2E-TEAM-011: two users, a share, a lease wait, a fork in parallel and two pushes; evidence in "
          + str(directory.relative_to(ROOT)), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("The Gate 4 demo was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    env.setdefault("TQA_RUNTIME_TEST_IMAGE", "tqa-runtime:workstation")
    if not env.get("TQA_GATEWAY_IMAGE", "").startswith("sha256:"):
        env["TQA_GATEWAY_IMAGE"] = compose.command(["docker", "image", "inspect", "--format", "{{.Id}}",
                                                    env.get("TQA_GATEWAY_IMAGE") or "tqa-gateway:local"],
                                                   env, capture=True).stdout.strip()
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-gate4-")
    directory = compose.installation(temporary)
    stack, failure = Gate4Stack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("push")
        verify(stack, *demo(stack))
    except BaseException as error:
        failure = error
        print("Gate 4 demo failed: " + compose.redact(str(error), env), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
