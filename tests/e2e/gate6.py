"""E2E-EXT-015: the Gate 6 demo as an automated scenario over the public API (DEV-PLAN Phase 6).

An administrator registers an issue-tracker MCP server with its credential in the secret store; a member creates a
profile that uses it; the agent files an issue; the credential appears nowhere in the sandbox. The runbook and the
expected evidence (E1-E6) are in gate6_evidence.py. The stack, the remote MCP server over HTTPS, the capture and every
sandbox search are the SEC-013 fixture (egress.connector_isolation), reused unchanged; only who stores the credential
(the administrator, in the secret store) and who runs the task (a member, from a profile) differ.

Run from the repository root, with no other stack running on the host: TQA_DOCKER_TESTS=1 python3 tests/e2e/gate6.py
"""

import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "crates" / "tqa-sandbox" / "tests"))
sys.path.insert(0, str(ROOT / "deploy" / "tests"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from egress import connector_isolation as isolation  # noqa: E402  (the SEC-013 fixture, reused read-only)
from egress.stack import compose  # noqa: E402
from egress.web_connector import EMAIL, forms, found  # noqa: E402
import gate6_evidence as evidence  # noqa: E402

DEADLINE_SECONDS = 600
READS_WAIT_SECONDS = 60


class DemoStack(isolation.IsolationStack):
    def scenario(self):
        super().scenario()
        config = self.directory / "gateway.toml"
        compose.ownership(self.directory, self.env)
        compose.write(config, evidence.seeded(config.read_text()))
        isolation.settled(config.read_text())

    def keep(self, answer):
        """Every API answer either person receives, for the absence search (E5)."""
        self.answers.append(json.dumps(answer))
        return answer

    def call(self, method, path, body):
        """A JSON request with a body for methods robustness.Stack lacks; returns the parsed answer."""
        request = urllib.request.Request(self.origin + path, method=method, data=json.dumps(body).encode(),
                                         headers={"Origin": self.origin, "Content-Type": "application/json",
                                                  "Idempotency-Key": os.urandom(16).hex()})
        with self.client.open(request, timeout=15) as response:
            text = response.read().decode()
        return self.keep(json.loads(text) if text else {})

    def sign_in(self, email, password):
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.keep(self.request("/api/v1/auth/login", {"email": email, "password": password}))

    def start(self):
        """Runbook steps 2-4: the administrator stores and allows the credential; the member makes the profile."""
        self.answers = []
        admin = {"email": EMAIL, "password": self.env["TQA_STACK_PASSWORD"]}
        self.keep(self.request("/api/v1/auth/bootstrap", dict(admin, token=self.env["TQA_BOOTSTRAP_TOKEN"])))
        self.sign_in(admin["email"], admin["password"])
        before = self.keep(self.request("/api/v1/admin/connectors"))
        self.call("PUT", "/api/v1/admin/secrets/" + evidence.SECRET_NAME, {"value": self.credential})
        listed = self.keep(self.request("/api/v1/admin/secrets"))
        version = next(item["version"] for item in listed["items"]
                       if item["ref"] in (evidence.SECRET_NAME, evidence.SECRET_REF))
        self.call("PUT", "/api/v1/admin/secrets/{}/consumers".format(evidence.SECRET_NAME),
                  {"consumers": [{"kind": "connector", "name": evidence.CONNECTOR}], "version": version})
        self.registration = evidence.check_registration(before, self.keep(self.request("/api/v1/admin/connectors")),
                                                        self.keep(self.request("/api/v1/admin/secrets")))
        invitation = self.keep(self.request("/api/v1/invitations", {"email": evidence.MEMBER_EMAIL, "role": "member"}))
        member_password = "test-member-" + secrets.token_hex(16)
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.keep(self.request("/api/v1/invitations/accept",
                               {"token": invitation["token"], "password": member_password}))
        self.sign_in(evidence.MEMBER_EMAIL, member_password)
        own = self.keep(self.request("/api/v1/connectors"))
        control = self.keep(self.request("/api/v1/conversations",
                                         {"title": "Without a profile", "model_route": "local"}))
        made = self.keep(self.request("/api/v1/profiles", evidence.profile()))
        assert made.get("usable") is True, made
        task = self.keep(self.request("/api/v1/conversations", {"title": "File the crash", "profile": made["id"]}))
        evidence.check_member(own, control, task)
        self.conversation = task["id"]
        self.path = "/api/v1/conversations/" + self.conversation

    def diagnostics(self):
        """The base diagnostics, plus each stream's frame trail and every gateway warning or error (redacted)."""
        super().diagnostics()
        for stream in getattr(self, "streams", []):
            print("Stream trail: " + json.dumps(stream.trail), flush=True)
        try:
            logs = self.command(self.configuration + ["logs", "--no-color", "gateway"])
            print(self.redact("\n".join(line for line in logs.splitlines()
                                         if '"level":"WARN"' in line or '"level":"ERROR"' in line)[-12000:]), flush=True)
        except Exception as error:
            print("Gateway logs unavailable: " + self.redact(str(error)), flush=True)

    def reads(self):
        """E6, as the administrator: the secret's read records, waited for (they are written in the background)."""
        self.sign_in(EMAIL, self.env["TQA_STACK_PASSWORD"])
        deadline = time.monotonic() + READS_WAIT_SECONDS
        while True:
            page = self.keep(self.request("/api/v1/admin/secrets/{}/reads?limit=100".format(evidence.SECRET_NAME)))
            try:
                return evidence.check_reads(page, self.conversation)
            except AssertionError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(2)


def check(stack):
    # E3, E4 and the sandbox half of E5: the SEC-013 driver's run and checks, unchanged (as the member).
    isolation.check(stack)
    count = stack.reads()
    values = forms(stack.credential)
    assert found(values, "x" + values[0] + "x") == [values[0]], "The form search misses a planted copy"
    dump = stack.dump()
    assert evidence.SECRET_NAME in dump and evidence.CONNECTOR in dump, "The dump does not hold the secret's record"
    answers = "\n".join(stack.answers)
    assert evidence.SECRET_REF in answers, "The collected answers do not hold the admin view, so they prove nothing"
    for place, text in (("the database dump", dump), ("an API answer", answers)):
        assert found(values, text) == [], "The credential reached " + place
    print("E2E-EXT-015 passed: the administrator stored the tracker credential in the secret store and allowed the "
          "connector to use it ({}); the member, with no credential of their own, made a profile that alone brought "
          "the connector; its call filed issue #7 with the stored credential, injected by the gateway; the "
          "store recorded {} read(s) by the connector for the task; the credential and its encoded forms are absent "
          "from the sandbox's traffic, processes and files, the events, the logs, the database dump and {} API "
          "answers".format(json.dumps(stack.registration["consumers"]), count, len(stack.answers)), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("E2E-EXT-015 was interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-gate6-")
    directory = compose.installation(temporary)
    env["TQA_STACK_PASSWORD"] = "synthetic-gate6-" + os.urandom(16).hex()
    stack, failure = DemoStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("push")
        stack.start()
        check(stack)
    except BaseException as error:
        failure = error
        print("E2E-EXT-015 failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for number in (signal.SIGINT, signal.SIGTERM):
            signal.signal(number, signal.SIG_IGN)
        for stream in getattr(stack, "streams", []):
            stream.response.close()
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
