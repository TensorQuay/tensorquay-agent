"""SEC-017 (stack half, Docker): a shared workspace member cannot reach another conversation's history.

An administrator and an invited member share a workspace, each with a task in it. The first task runs a turn; the second
takes the idle workspace (GW-I-031) and runs; from the second's sandbox the probes and the checks in
history_isolation_checks.py (H1-H3) follow; then the first task runs again and its history is compared. The probes run
with `docker exec` in the second's sandbox container, as its own user: the same filesystem view the agent has.

Run from the repository root, with no other stack running on the host: TQA_DOCKER_TESTS=1 python3
tests/e2e/history_isolation.py (TQA_GATEWAY_IMAGE names the gateway image, TQA_RUNTIME_IMAGE the runtime).
"""

import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import compose  # noqa: E402
import demo  # noqa: E402
import history_isolation_checks as checks  # noqa: E402
import robustness  # noqa: E402

DEADLINE_SECONDS = 900
RUN_SECONDS = 240
OWNER = "owner@example.invalid"
MEMBER = "member@example.invalid"
# Every turn answers with text: the probes run from outside the agent, so the model only has to make a run.
SCENARIO = {"model": "fixture", "responses": [
    {"events": [{"type": "text", "text": "Noted."}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}}
    for _ in range(4)]}


class HistoryStack(robustness.Stack):
    def prepare(self, case="history"):
        image = self.env.get("TQA_GATEWAY_IMAGE", "tqa-gateway:local")
        identity = self.command(["docker", "image", "inspect", "--format", "{{.Id}}", image])
        print("Gateway image: " + image + " " + identity, flush=True)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
            compose.prepare(self.directory, self.env, runtime=demo.runtime_image(self.env), port=port)
        compose.ownership(self.directory, self.env)
        self.origin = "http://127.0.0.1:" + str(port)
        compose.write(self.directory / "scenario.json", json.dumps(SCENARIO))
        compose.write(self.directory.parent / "robustness.json", json.dumps({"services": {
            "fake-model": {"command": ["--scenario", "/scenarios/robustness.json"],
                           "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                                        "target": "/scenarios/robustness.json", "read_only": True}]}}}))
        compose.ownership(self.directory, self.env, "1000:1000")
        self.up()

    def request(self, path, body=None, after=None):
        try:
            return super().request(path, body, after)
        except urllib.error.HTTPError as error:
            raise AssertionError("{} answered {}: {}".format(path, error.code, error.read()[:512])) from None

    def call(self, method, path, body):
        request = urllib.request.Request(self.origin + path, method=method, data=json.dumps(body).encode(),
                                         headers={"Origin": self.origin, "Content-Type": "application/json",
                                                  "Idempotency-Key": secrets.token_hex(16)})
        with self.client.open(request, timeout=15) as response:
            return json.loads(response.read(1048577) or b"{}")

    def people(self):
        """The owner (the bootstrap administrator) and an invited member, each with their own session."""
        password = secrets.token_urlsafe(24)
        self.env["TQA_STACK_PASSWORD"] = password
        self.request("/api/v1/auth/bootstrap", {"email": OWNER, "password": password,
                                                "token": self.env["TQA_BOOTSTRAP_TOKEN"]})
        self.request("/api/v1/auth/login", {"email": OWNER, "password": password})
        self.owner = self.client
        invitation = self.request("/api/v1/invitations", {"email": MEMBER, "role": "member"})
        self.member = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.client = self.member
        member_password = secrets.token_urlsafe(24)
        self.request("/api/v1/invitations/accept", {"token": invitation["token"], "password": member_password})
        self.request("/api/v1/auth/login", {"email": MEMBER, "password": member_password})

    def as_person(self, client):
        self.client = client
        return self

    def tasks(self):
        """The owner's workspace, shared for writing with the member, and a task of each in it."""
        workspace = self.as_person(self.owner).request("/api/v1/workspaces", {"name": "Shared"})["id"]
        self.call("PUT", "/api/v1/workspaces/{}/shares".format(workspace), {"email": MEMBER, "access": "write"})
        body = {"title": "Task", "model_route": "local", "workspace_id": workspace}
        self.first = self.request("/api/v1/conversations", body)["id"]
        self.second = self.as_person(self.member).request("/api/v1/conversations", body)["id"]
        print("Workspace {}: first task {} (owner), second {} (member)".format(workspace, self.first, self.second),
              flush=True)

    def events(self, conversation):
        return self.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]

    def run(self, client, conversation):
        """Sends a message and waits for the run it starts to complete."""
        self.as_person(client)
        done = sum(1 for frame in self.events(conversation)
                   if (frame.get("event") or {}).get("type") == "run.completed")
        self.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "Go on.", "attachments": []})
        deadline = time.monotonic() + RUN_SECONDS
        while time.monotonic() < deadline:
            kinds = [(frame.get("event") or {}).get("type") for frame in self.events(conversation)]
            assert kinds.count("run.failed") == 0, "the run failed: {}".format(kinds)
            if kinds.count("run.completed") > done:
                return
            time.sleep(1)
        raise AssertionError("the run of {} did not complete in {} s".format(conversation, RUN_SECONDS))

    def sandbox(self, conversation):
        found = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                              "--filter", "label=tqa.conversation=" + conversation]).split()
        assert len(found) == 1, "expected the one sandbox of {}, found {}".format(conversation, found)
        return found[0]

    def probe(self):
        """H1-H3's probes from the second's sandbox; the forged log goes into the shared workspace."""
        container = self.sandbox(self.second)
        mounts = json.loads(self.command(["docker", "inspect", "--format", "{{json .Mounts}}", container]))
        found = {"H1": checks.check_mounts(mounts, self.first, self.second)}
        control = str(uuid.uuid4())
        subprocess.run(["docker", "exec", container, "sh", "-c", checks.control_command(control)], env=self.env,
                       capture_output=True, text=True, timeout=60, check=True)
        walk = subprocess.run(["docker", "exec", container, "sh", "-c", checks.WALK], env=self.env,
                              capture_output=True, text=True, timeout=120, check=True).stdout
        found["H2"] = checks.check_walk(walk, self.first, self.second, control)
        subprocess.run(["docker", "exec", "-i", container, "sh", "-c", checks.forge_command()], env=self.env,
                       input=checks.forged_log(self.first), capture_output=True, text=True, timeout=60, check=True)
        return found


def interrupted(_number, _frame):
    raise InterruptedError("SEC-017 was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-sec017-")
    directory = compose.installation(temporary)
    stack, failure = HistoryStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare()
        stack.people()
        stack.tasks()
        stack.run(stack.owner, stack.first)
        before = stack.as_person(stack.owner).events(stack.first)
        stack.run(stack.member, stack.second)
        found = stack.probe()
        # The first takes the workspace back from the idle second and resumes from its own history.
        stack.run(stack.owner, stack.first)
        found["H3"] = checks.check_history(before, stack.as_person(stack.owner).events(stack.first))
        print("SEC-017 (stack) passed: " + json.dumps(found), flush=True)
    except BaseException as error:
        failure = error
        print("SEC-017 (stack) failed: " + compose.redact(str(error), env), flush=True)
        try:
            logs = stack.command(stack.configuration + ["logs", "--no-color", "gateway"])
            print(compose.redact(logs[-6000:], env), flush=True)
        except Exception as diagnostic:
            print("gateway logs unavailable: " + compose.redact(str(diagnostic), env), flush=True)
    finally:
        signal.alarm(0)
        for number in (signal.SIGINT, signal.SIGTERM):
            signal.signal(number, signal.SIG_IGN)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
