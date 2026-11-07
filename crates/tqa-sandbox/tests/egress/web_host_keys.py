"""WEB-E2E-035, the web half of the host-keys stack suite (ADR-0037) on the same owned stack as G9b (`host_keys.py`):
the keyed host at its exact address, its authority in `[egress] ca_file`, the release images and no test hook. The
person's part happens in the browser (`web/e2e/host-keys-stack.spec.ts`), in two phases; this driver owns the stack,
the teammate's account and the checks of what reached the keyed host. Kept apart from G9b, so the security suite runs
without the web image and Playwright, and a failure says which side broke.

- Own key: the administrator adds `FIXTURE_KEY` for the keyed host's address in the browser, and a task of theirs calls
  it. The host received the value once; the stored events never hold it; the key's last use names that task.
- Teammate: with their own key for the same address, a teammate's task tries the first task's placeholder. Refused as
  another task's (`foreign_sandbox`), on the teammate's step in words, and again after a reload (the stored key uses);
  nothing more reached the host.

Run from the repository root, on images built from the commit under test, after `pnpm install` in web/:
    PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE=sha256:<gateway image id> \\
    TQA_RUNTIME_TEST_IMAGE=<workstation runtime image> TQA_WEB_IMAGE=sha256:<web image id> \\
    TQA_WORKSTATION_IMAGE=<workstation image id> python3 -m egress.web_host_keys
"""
from contextlib import contextmanager
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import subprocess
import tempfile
import urllib.request

from .host_keys import DEADLINE_SECONDS, NAME, SEC_048_OTHER, HostKeyStack, interrupted, results
from .stack import ROOT, compose
from .web_connector import WEB_WAIT_SECONDS

ADMIN_EMAIL = "host-keys@example.invalid"
TEAMMATE_EMAIL = "teammate@example.invalid"
# Each browser phase starts one task's environment at most, inside its own budget (web/e2e/stack-start.ts).
BROWSER_TIMEOUT_SECONDS = 420
OUTPUT_TAIL_CHARACTERS = 6000
PLACEHOLDER = re.compile(r"^" + NAME + r"=(tqa_ph_[A-Za-z0-9_-]{43})$", re.MULTILINE)


class WebHostKeyStack(HostKeyStack):
    """G9b's stack with its page server, signed in; the key itself is added in the browser."""

    def start(self):
        # The egress stack starts the gateway alone; the browser needs the page server too.
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])
        self.sign_in()

    def browser(self, phase, result, **extra):
        """Runs the spec's `phase` and returns what it wrote to `result`."""
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE="host-keys", TQA_HOST_KEY_PHASE=phase,
                   TQA_STACK_RESULT=str(result), TQA_HOST_KEY_ADDRESS=self.address, **extra)
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=BROWSER_TIMEOUT_SECONDS)
        print(self.redact(done.stdout)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        assert done.returncode == 0, "The browser's {} phase of WEB-E2E-035 failed".format(phase)
        return json.loads(result.read_text())

    def open_task(self, title, prepare):
        """A new task with `prepare` written into its workspace and no message yet: the browser sends it, so the page
        is open when the run's live notices come (they are never stored)."""
        conversation = self.request("/api/v1/conversations", {"title": title, "model_route": "local"})["id"]
        compose.ownership(self.directory, self.env)
        workspace = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / conversation
        workspace.mkdir(parents=True, exist_ok=True)
        prepare(workspace)
        compose.ownership(self.directory, self.env, "1000:1000")
        return conversation

    @contextmanager
    def person(self, client):
        """Requests made inside go out with `client`'s session; the administrator's comes back after."""
        own, self.client = self.client, client
        try:
            yield
        finally:
            self.client = own

    def teammate(self):
        """A member invited by the administrator, with the same synthetic password; their own session's client."""
        invitation = self.request("/api/v1/invitations", {"email": TEAMMATE_EMAIL, "role": "member"})
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        with self.person(client):
            self.request("/api/v1/invitations/accept",
                         {"token": invitation["token"], "password": self.env["TQA_STACK_PASSWORD"]})
            self.request("/api/v1/auth/login", {"email": TEAMMATE_EMAIL, "password": self.env["TQA_STACK_PASSWORD"]})
        return client


def own_key(stack, directory):
    """The administrator's key from the browser, used by their task: the host got its value once, and only there."""
    before = len(stack.received())
    found = stack.browser("own", directory / "own.json", TQA_HOST_KEY_EMAIL=ADMIN_EMAIL,
                          TQA_HOST_KEY_VALUE=stack.value)
    conversation = found["conversation"]
    stored = stack.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]
    assert stack.received()[before:] == ["Bearer " + stack.value], "the task's call carried the key's value once"
    assert stack.value not in json.dumps(stored), "the key's value is in the task's events"
    keys = stack.request("/api/v1/host-keys")["items"]
    added = [key for key in keys if key["name"] == NAME]
    assert len(added) == 1 and added[0]["last_use"]["conversation_id"] == conversation, \
        "the key's last use names the task: {}".format(added)
    calls = results(stored)
    placeholders = {match for call in calls for match in PLACEHOLDER.findall(call.get("content", ""))}
    assert len(placeholders) == 1, "the task saw its key as one placeholder"
    print("WEB-E2E-035 own key: added in the browser, its value reached the host once and nowhere else, and its last "
          "use names the task", flush=True)
    return placeholders.pop()


def teammate(stack, directory, placeholder):
    """With their own key for the address, the teammate's task tries the first task's placeholder: refused as another
    task's, and the host gets nothing more. The browser sends the message, so it sees the refusal as it happens."""
    before = len(stack.received())
    client = stack.teammate()
    write = lambda workspace: compose.write(workspace / "foreign-placeholder", placeholder)
    with stack.person(client):
        stack.request("/api/v1/host-keys", {"name": NAME, "address": stack.address, "scheme": "bearer",
                                            "value": "tqa-g9-teammate-" + secrets.token_hex(24)})
        conversation = stack.open_task("Tries another task's placeholder", write)
    found = stack.browser("teammate", directory / "teammate.json", TQA_HOST_KEY_TEAMMATE_EMAIL=TEAMMATE_EMAIL,
                          TQA_HOST_KEY_TEAMMATE_TASK=conversation, TQA_HOST_KEY_PHRASE=SEC_048_OTHER)
    assert "came from another task" in found["refused"], found
    assert "came from another task" in found["reloaded"], "after a reload, from the stored key uses: {}".format(found)
    with stack.person(client):
        stored = stack.request("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]
    foreign = results(stored)[-1]["content"]
    assert "status 403" in foreign and "This placeholder belongs to another task." in foreign, foreign
    assert stack.received()[before:] == [], "another task's placeholder reached the host"
    print("WEB-E2E-035 teammate: another task's placeholder was refused on the teammate's step in words, said again "
          "after a reload from the stored key uses, and never reached the host", flush=True)


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-web-host-keys-")
    directory = compose.installation(temporary)
    results_directory = Path(tempfile.mkdtemp(prefix="tqa-web-host-keys-results-"))
    stack, failure = WebHostKeyStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("host-keys")
        stack.start()
        placeholder = own_key(stack, results_directory)
        teammate(stack, results_directory, placeholder)
    except BaseException as error:
        failure = error
        print("WEB-E2E-035 failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        try:
            compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
        finally:
            # The browser's results and its own output (Playwright writes beside the result); never before the stack
            # is down, so a slip here can't leave it running.
            shutil.rmtree(results_directory, ignore_errors=True)


if __name__ == "__main__":
    main()
