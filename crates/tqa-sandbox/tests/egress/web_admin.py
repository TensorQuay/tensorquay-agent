"""WEB-E2E-006: an administrator's changes in the browser take effect for the next new conversation, without a restart.

Reuses the egress fixture (an upstream on the sandbox network and the policy proxy) and replaces only its scenario and
setup. In the browser: the seeded allow rule is deleted, so a new conversation's fetch is blocked; a model route, an
allow rule and an invitation are added; the invited member's new conversation on the new route fetches through; the
rule is deleted and the next new conversation is blocked again.
Run from the repository root: PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 python3 -m egress.web_admin
"""
import json
import os
import signal
import subprocess
import tempfile

from .stack import ROOT, Stack, compose

EMAIL = "ux@example.invalid"
# The upstream's plain HTTP port (upstream.py), the one the rule allows.
UPSTREAM_PORT = 8081
# The whole fixture's deadline, as the other egress contracts use for the same stack.
DEADLINE_SECONDS = 600
# The browser part: its spec allows 300 s; the rest covers Playwright start-up.
BROWSER_TIMEOUT_SECONDS = 420
# Printed browser output, as the stack diagnostics bound theirs (stack.py prints the last 12000 characters).
OUTPUT_TAIL_CHARACTERS = 12000
# The page server's health wait, as the base fixture waits for postgres and the model server (stack.py).
WEB_WAIT_SECONDS = 60


class WebAdminStack(Stack):
    def scenario(self):
        """Three conversations in turn, each one fetch and one answer; the fake model serves them in order."""
        self.url = "http://{}:{}/allowed".format(self.upstream, UPSTREAM_PORT)
        responses = []
        # A neutral answer: whether each fetch went through is asserted on its own result, never on this text.
        for _ in range(3):
            responses.append({"events": [{"type": "tool_call", "name": "fetch_url", "arguments": {"url": self.url},
                                          "split_into": 1}]})
            responses.append({"events": [{"type": "text", "text": "Done."}]})
        for response in responses:
            response["usage"] = {"prompt_tokens": 10, "completion_tokens": 10}
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": responses}))

    def start(self):
        """Only the page server: the browser bootstraps the administrator and makes every change."""
        # The API-only egress contracts never start the page server; the gateway serves pages from it (ADR-0022).
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])

    def browser(self, result):
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE="admin", TQA_STACK_RESULT=str(result),
                   TQA_ADMIN_UPSTREAM=self.upstream, TQA_ADMIN_PORT=str(UPSTREAM_PORT), TQA_ADMIN_URL=self.url)
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=BROWSER_TIMEOUT_SECONDS)
        print(compose.redact(done.stdout, self.env)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        assert done.returncode == 0, "The browser part of WEB-E2E-006 failed"
        return json.loads(result.read_text())

    def fetches(self, conversation):
        """The fetch results of one of the administrator's conversations, as stored."""
        stored = self.request("/api/v1/conversations/" + conversation + "/events?after=0&limit=1000")["items"]
        calls = {frame["event"]["call_id"] for frame in stored
                 if frame.get("event", {}).get("type") == "tool.call" and frame["event"]["name"] == "fetch_url"}
        return [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"
                and frame["event"]["call_id"] in calls]


def check(stack, result):
    browser = stack.browser(result)
    stack.request("/api/v1/auth/login", {"email": EMAIL, "password": stack.env["TQA_STACK_PASSWORD"]})
    before, after = stack.fetches(browser["before"]), stack.fetches(browser["after"])
    assert [item["status"] for item in before] == ["error"], json.dumps(before)[:1000]
    assert [item["status"] for item in after] == ["error"], json.dumps(after)[:1000]
    rules = stack.request("/api/v1/admin/egress")
    assert all(rule["host"] != stack.upstream for rule in rules["items"]), "The deleted rule is still listed"
    routes = {route["name"]: route for route in stack.request("/api/v1/admin/routes")["items"]}
    assert routes.get("second", {}).get("model") == "fixture", "The route added in the browser is not stored"
    print("WEB-E2E-006: with the seeded rule deleted a new conversation's fetch was blocked; the route, the allow rule "
          "and the invitation added in the browser served the member's new conversation on the new route without a "
          "restart; after the rule was deleted the next new conversation was blocked again", flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("Web admin fixture interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-web-admin-")
    directory = compose.installation(temporary)
    env["TQA_STACK_PASSWORD"] = "synthetic-web-admin-" + os.urandom(16).hex()
    stack, failure = WebAdminStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS)
    try:
        stack.prepare("admin")
        stack.start()
        # The installation belongs to the gateway's user; the browser writes its result in the temporary root.
        check(stack, directory.parent / "browser-result.json")
    except BaseException as error:
        failure = error
        print("Web admin contract failed: " + compose.redact(str(error), env), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
