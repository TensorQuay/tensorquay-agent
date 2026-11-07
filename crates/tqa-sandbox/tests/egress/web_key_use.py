"""WEB-E2E-029 (connector call and Git push): with the gateway's read-audit table made unwritable mid-run, a person's
key is never used. A connector call fails as the gateway's own key_use_unrecorded and its step says so in plain words;
a push with the person's host key (plain git, ADR-0037) is refused as unrecorded and its step says so; and nothing more
reaches the connector or the origin.

The stacks of WEB-E2E-010 (web_connector.py) and the push fixture's KeyPushStack (git_push.py), with the person's
credential or host key stored through the API. The model reply whose tool call needs the key is held (TK-U-035) while the driver waits for that model
call's own key read to be recorded (D4: a row is committed before its key is used), then renames the table. So the model call is not the one refused first, and the
connector's session and the workspace's clone, which read their keys too, have already happened.
Run from the repository root: PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 python3 -m egress.web_key_use
[--push]
"""
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time

from .git_push import PUSH_KEY, KeyPushStack, push_command
from .push_checks import witness
from .stack import ROOT, compose
from .web_connector import (BROWSER_TIMEOUT_SECONDS, CONNECTOR, DEADLINE_SECONDS, EMAIL,
                            OUTPUT_TAIL_CHARACTERS, ConnectorStack)

HOLD_MS = 45000
BREAK_WAIT_SECONDS = 120
RUN_WAIT_SECONDS = 300
# The push case's branch, account and the notes its first step writes.
BRANCH = "feature/web-e2e"
PUSH_EMAIL = "ux@example.invalid"
NOTES = "# Push notes\nWritten before the push.\n"
# The page server's health wait, as the base fixture waits for postgres and the model server (stack.py).
WEB_WAIT_SECONDS = 60
# The live push check's handshake with its spec (key-use-push-stack.spec.ts), as E2E-AGENTS-020's: the spec writes
# PAGE_READY once the task is open and working; the driver then releases the push, which waits for RELEASE in its
# sandbox; on any failure here the driver writes PUSH_FAILED with the reason, and the spec fails at once with it.
PAGE_READY, PUSH_FAILED = "page-ready", "push-failed"
RELEASE = "/tmp/release-push"
# Each side's wait for the other, before the deadline scale.
HANDSHAKE_SECONDS = 240


def scale(env):
    """The harness's deadline scale (crates/tqa-testkit/src/deadline.rs): an integer from 1 to 20, else 1."""
    text = env.get("TQA_TEST_DEADLINE_SCALE", "").strip()
    return int(text) if text.isdigit() and 1 <= int(text) <= 20 else 1


def mark(markers, name, text):
    """Writes the marker `name` whole or not at all, as E2E-AGENTS-020's driver does."""
    partial = markers / (name + ".partial")
    partial.write_text(text)
    os.replace(partial, markers / name)
BREAK = "ALTER TABLE secret_reads RENAME TO secret_reads_away"
MEND = "ALTER TABLE secret_reads_away RENAME TO secret_reads"
REASON = "gateway.key_use_unrecorded 503"
# The runtime's record of the refused connector call (the gateway-refusal contract, ADR-0019 amendment).
CALL_ERROR = "key_use_unrecorded: the gateway's MCP proxy answered 503"
# The route's key reads so far, one per model call. The runtime sends no run id to the relay, so its reads have none,
# like the gateway's title reads; these fixtures name their tasks, so no title is asked for and every read is a call.
MODEL_KEY_READS = "SELECT coalesce(sum(count), 0) FROM secret_reads WHERE consumer_kind = 'route'"


class Breaker:
    """Breaks the read-audit table once the held model call (the `held_call`th of the run) has used its key."""
    held_call = 1

    def psql(self, statement):
        return self.command(self.configuration + ["exec", "-T", "postgres", "psql", "-v", "ON_ERROR_STOP=1", "-tA",
                                                  "-U", "tqa", "-d", "gateway", "-c", statement])

    def hold(self, responses):
        responses[self.held_call - 1]["wait_ms"] = HOLD_MS
        return responses

    def watch(self):
        self.broken, self.remote_at_break, self.watch_error = False, None, None
        watcher = threading.Thread(target=self.break_after_the_held_call, daemon=True)
        watcher.start()
        return watcher

    def break_after_the_held_call(self):
        try:
            deadline = time.monotonic() + BREAK_WAIT_SECONDS
            while int(self.psql(MODEL_KEY_READS)) < self.held_call:
                if time.monotonic() >= deadline:
                    raise TimeoutError("The held model call never used its key")
                time.sleep(0.2)
            self.psql(BREAK)
            self.broken = True
            self.remote_at_break = self.remote_requests()
        except BaseException as error:
            self.watch_error = error

    def joined(self, watcher):
        watcher.join(timeout=5)
        if self.watch_error is not None:
            raise self.watch_error
        assert self.broken, "The read-audit table was never broken"

    def mend(self):
        if getattr(self, "broken", False):
            self.psql(MEND)
            self.broken = False


class KeyUseConnectorStack(Breaker, ConnectorStack):
    suite = "key-use-connector"

    def scenario(self):
        super().scenario()
        path = self.directory / "scenario.json"
        document = json.loads(path.read_text())
        self.hold(document["responses"])
        compose.ownership(self.directory, self.env)
        compose.write(path, json.dumps(document))

    def start(self):
        super().start()
        self.request_method("PUT", "/api/v1/connectors/{}/credential".format(CONNECTOR), {"value": self.credential})

    def remote_requests(self):
        return len(self.upstream_get("/record")["requests"])

    def browser(self, result):
        watcher = self.watch()
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE=self.suite, TQA_STACK_RESULT=str(result),
                   TQA_CONNECTOR_CONVERSATION=self.conversation, TQA_KEY_USE_EMAIL=EMAIL)
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=BROWSER_TIMEOUT_SECONDS)
        print(self.redact(done.stdout)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        self.joined(watcher)
        assert done.returncode == 0, "The browser part of WEB-E2E-029 (connector) failed"
        return json.loads(result.read_text())


class KeyUsePushStack(Breaker, KeyPushStack):
    """The push's model reply is the run's second model call: the first writes the notes. The push is plain git with
    the person's host key (ADR-0037), refused when the key's use can't be recorded."""
    held_call = 2

    def scenario(self):
        super().scenario()
        lines = " ".join("'" + line + "'" for line in NOTES.splitlines())
        write = "printf '%s\\n' {} > NOTES.md && git checkout -b {}".format(lines, BRANCH)
        responses = []
        for turn in [("shell", {"command": write}), ("shell", {"command": self.released_push()}), "Pushed."]:
            events = ([{"type": "text", "text": turn}] if isinstance(turn, str) else
                      [{"type": "tool_call", "name": turn[0], "arguments": turn[1], "split_into": 1}])
            responses.append({"events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}})
        compose.write(self.directory / "scenario.json",
                      json.dumps({"model": "fixture", "responses": self.hold(responses)}))

    def released_push(self):
        """The push, once the driver has released it (RELEASE in the sandbox), within the handshake's bound."""
        tries = 2 * HANDSHAKE_SECONDS * scale(self.env)
        return ("for i in $(seq {tries}); do [ -f {release} ] && break; sleep 0.5; done; "
                "[ -f {release} ] || {{ echo 'the driver never released the push ({release})'; exit 3; }}; "
                "{push}").format(tries=tries, release=RELEASE, push=push_command(BRANCH))

    def start(self):
        # The page shows the push's step, so the page server starts too (ADR-0022).
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])
        # The right key this time: only the unrecorded use can stop the push.
        super().start(email=PUSH_EMAIL, title="Push with a key")

    def remote_requests(self):
        return len(witness(self))

    def run_with_page(self, result):
        """The run with its page open (the spec, started first): the message, the spec's `page-ready`, the broken
        table, then the push released; the run's end, then the spec's. Any failure here writes `push-failed` for the
        spec before it is raised. Returns the stored events."""
        markers = self.directory.parent / "key-use-markers"
        markers.mkdir(mode=0o700, exist_ok=True)
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE="key-use-push", TQA_STACK_RESULT=str(result),
                   TQA_PUSH_CONVERSATION=self.conversation, TQA_KEY_USE_EMAIL=PUSH_EMAIL,
                   TQA_KEY_USE_MARKERS=str(markers), TQA_PUSH_KEY_NAME=PUSH_KEY,
                   TQA_PUSH_HOST="{}:8443".format(self.upstream))
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        browser = subprocess.Popen(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True)
        try:
            watcher = self.watch()
            self.request(self.path + "/messages", {"text": "Write the notes and push them, please.", "attachments": []})
            self.until(lambda: (markers / PAGE_READY).exists(), "the spec never wrote " + PAGE_READY, browser)
            self.until(lambda: self.broken or self.watch_error is not None,
                       "the read-audit table was never broken", browser)
            self.command(["docker", "exec", self.container(self.conversation), "touch", RELEASE])
            stored = self.ended()
            self.joined(watcher)
        except BaseException as error:
            mark(markers, PUSH_FAILED, self.redact(str(error)) or type(error).__name__)
            self.stopped(browser)
            raise
        output, code = self.stopped(browser)
        assert code == 0, "The browser part of WEB-E2E-029 (push) failed"
        return stored

    def until(self, done, missing, browser):
        """Waits for `done`, bounded and scaled; fails naming `missing`, or the spec's own end if it ended first."""
        deadline = time.monotonic() + HANDSHAKE_SECONDS * scale(self.env)
        while not done():
            if browser.poll() is not None:
                raise AssertionError("the spec ended before {}: exit {}".format(missing, browser.returncode))
            if time.monotonic() >= deadline:
                raise TimeoutError(missing)
            time.sleep(0.2)

    def ended(self):
        """The stored events once the run has ended."""
        deadline = time.monotonic() + RUN_WAIT_SECONDS * scale(self.env)
        while True:
            stored = self.stored()
            kinds = [frame.get("event", {}).get("type") for frame in stored]
            if "run.failed" in kinds or "run.completed" in kinds:
                return stored
            if time.monotonic() >= deadline:
                raise TimeoutError("The run did not end")
            time.sleep(1)

    def stopped(self, browser):
        """The spec's output (printed, redacted) and exit code, waiting for it within its bound; stopped past it."""
        try:
            output, _ = browser.communicate(timeout=BROWSER_TIMEOUT_SECONDS * scale(self.env))
        except subprocess.TimeoutExpired:
            browser.kill()
            output, _ = browser.communicate()
        print(self.redact(output or "")[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        return output, browser.returncode


def failure_of(stored):
    return next((frame["event"] for frame in stored if frame.get("event", {}).get("type") == "run.failed"), {})


def check_connector(stack, result):
    browser = stack.browser(result)
    record = stack.upstream_get("/record")
    stack.mend()
    assert browser["rawShown"] == [], "Raw words reached the page: " + json.dumps(browser.get("rawLines", browser["rawShown"]))
    assert any(CALL_ERROR in event for event in browser["toolEvents"]), \
        "The call's error is not the gateway's own refusal: " + json.dumps(browser["toolEvents"])
    assert browser["reason"] == REASON, "The run failed otherwise: " + json.dumps(browser)
    assert record["calls"] == [], "A call reached the remote: " + json.dumps(record["calls"])
    after = len(record["requests"]) - stack.remote_at_break
    assert after == 0, "{} requests reached the remote after the table was broken".format(after)
    print("WEB-E2E-029 (connector): with the read-audit table unwritable, the connector call failed as the gateway's "
          "own key_use_unrecorded, its step and the run said so in plain words, and no request (no call) reached the "
          "remote after the break", flush=True)


def key_push_results(stored):
    """The results of the run's push: its shell calls that push with the person's key."""
    calls = {frame["event"]["call_id"] for frame in stored if frame.get("event", {}).get("type") == "tool.call"
             and frame["event"].get("name") == "shell"
             and str(frame["event"].get("arguments", {}).get("command", "")).endswith(push_command(BRANCH))}
    return [frame["event"] for frame in stored
            if frame.get("event", {}).get("type") == "tool.result" and frame["event"]["call_id"] in calls]


def check_push(stack, result):
    stored = stack.run_with_page(result)
    after = len(witness(stack)) - stack.remote_at_break
    refs = stack.upstream_get("/refs")
    stack.mend()
    results = key_push_results(stored)
    # A refused shell command is the step's failure, not the run's: the model goes on, so no run reason is asserted.
    assert [item["status"] for item in results] == ["error"], json.dumps(results)[:1000]
    assert after == 0, "{} requests reached the origin after the table was broken".format(after)
    assert "refs/heads/" + BRANCH not in refs, "The branch reached the origin"
    print("WEB-E2E-029 (push): with the read-audit table unwritable, the plain-git push with the person's host key was "
          "refused as unrecorded and its step said so; no request reached the origin after the break and the branch "
          "is not there", flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("Key-use fixture interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-web-key-use-")
    directory = compose.installation(temporary)
    env["TQA_STACK_PASSWORD"] = "synthetic-web-key-use-" + os.urandom(16).hex()
    push = "--push" in sys.argv
    stack, failure = (KeyUsePushStack if push else KeyUseConnectorStack)(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("push")
        stack.start()
        if push:
            check_push(stack, directory.parent / "browser-result.json")
        else:
            check_connector(stack, directory.parent / "browser-result.json")
    except BaseException as error:
        failure = error
        print("Key-use contract failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        try:
            stack.mend()
        except Exception as error:
            failure = failure or error
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
