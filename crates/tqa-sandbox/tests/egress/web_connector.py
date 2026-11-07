"""WEB-E2E-010 (connector half): in the browser, a person adds their connector credential and the task's connector
call runs with it (ADR-0035: nothing asks first); the credential appears nowhere but at the remote.

Reuses the RT-I-016 fixture (its CA, the server certificate carrying the upstream's IP, the egress network) with the
remote MCP server in place of the Git origin (mcp_fixture.py). The connector is seeded in gateway.toml: the URL names the
upstream's IP literal (the proxy refuses a private address reached by name, SEC-024), and its ca_file is the fixture CA in
the storage root the gateway reads.
Run from the repository root: PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 python3 -m egress.web_connector
"""
import base64
import json
import os
import re
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from . import page_diff
from .git_push import PushStack
from .stack import ROOT, compose

EMAIL = "connector@example.invalid"
CONNECTOR = "tracker"
# The tool as the model calls it; the journal and the activity line hold mcp.tracker.create_issue.
MODEL_TOOL = "mcp__tracker__create_issue"
ARGUMENTS = {"title": "Crash on save", "labels": ["bug", "p1"], "body": "Steps: open a file, save it twice."}
# The whole fixture's deadline: ten minutes, as the push fixture's suites set.
DEADLINE_SECONDS = 600
BROWSER_TIMEOUT_SECONDS = 420
OUTPUT_TAIL_CHARACTERS = 12000
WEB_WAIT_SECONDS = 60
# The fixture's record port answers within seconds of the container starting.
FIXTURE_WAIT_SECONDS = 60


def forms(value):
    """Every form the absence search looks for: raw, URL-encoded, base64 whole, and base64 inside anything (an HTTP
    Basic header's user:token among them) at each byte alignment, in both alphabets (deploy/firecracker/guest_search.py,
    shared with the microVM halves)."""
    sys.path.insert(0, str(ROOT / "deploy/firecracker"))
    import guest_search
    return guest_search.forms(value)


class ConnectorStack(PushStack):
    # The upstream is the MCP fixture, which has no Git witness: its record of each call (never the credential).
    upstream_record = "/record"

    def overlay(self):
        super().overlay()
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "upstream.py", Path(__file__).with_name("mcp_fixture.py").read_text())
        # Generated per run, so there is nothing to allowlist and no constant can pass the absence search.
        self.credential = "fake-connector-token-" + secrets.token_urlsafe(18)
        compose.write(self.directory / "connector-token", self.credential + "\n")
        compose.ownership(self.directory, self.env, "1000:1000")

    def scenario(self):
        deadline = time.monotonic() + FIXTURE_WAIT_SECONDS
        while True:
            try:
                self.upstream_get("/record")
                break
            except Exception:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.2)
        turns = [(MODEL_TOOL, ARGUMENTS), "Created issue #7 in the tracker."]
        responses = []
        for turn in turns:
            events = ([{"type": "text", "text": turn}] if isinstance(turn, str) else
                      [{"type": "tool_call", "name": turn[0], "arguments": turn[1], "split_into": 1}])
            responses.append({"events": events, "usage": {"prompt_tokens": 10, "completion_tokens": 10}})
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json", json.dumps({"model": "fixture", "responses": responses}))
        storage = Path(self.env["TQA_STORAGE_ROOT"])
        compose.write(storage / "fixture-ca.pem", (self.directory / "fixture-ca.pem").read_text())
        # A display name is database metadata set through the admin API; the file's [[connectors]] refuses it.
        seed = ("\n[[connectors]]\nname = {name}\nurl = {url}\n"
                "enabled_by_default = true\nca_file = {ca}\n[connectors.auth]\nscheme = \"header\"\n"
                "header = \"X-Api-Key\"\nrequired = true\n").format(
            name=json.dumps(CONNECTOR), url=json.dumps("https://{}:8443/mcp".format(self.upstream)),
            ca=json.dumps(str(storage / "fixture-ca.pem")))
        config = self.directory / "gateway.toml"
        compose.write(config, config.read_text() + seed)
        # The base prepare() appends [egress] next and hands the tree to the gateway user. Parse the file now, so an
        # order or syntax slip fails here, not as a gateway that won't start.
        settled(config.read_text())

    def start(self):
        """The page server, the administrator, and a task with the connector (enabled by default); no credential yet."""
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])
        password = self.env["TQA_STACK_PASSWORD"]
        account = {"email": EMAIL, "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        conversation = self.request("/api/v1/conversations", {"title": "Connector from the browser", "model_route": "local"})
        assert conversation.get("connectors") == [CONNECTOR], "The task must start with the connector: " + json.dumps(conversation)
        self.conversation = conversation["id"]
        self.path = "/api/v1/conversations/" + self.conversation

    def redact(self, text):
        text = compose.redact(text, self.env)
        credential = getattr(self, "credential", None)
        for form in forms(credential) if credential else []:
            text = text.replace(form, "[REDACTED]")
        return text

    def private_file(self, name, text):
        """A file in the temporary root readable by this user only; cleanup removes it."""
        path = self.directory.parent / name
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(descriptor, "w") as handle:
            handle.write(text)
        return path

    suite = "connector"

    def browser(self, result):
        # The profile half makes its task in the browser, so there is no conversation to hand over (ProfileStack).
        task = {"TQA_CONNECTOR_CONVERSATION": self.conversation} if self.conversation else {}
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE=self.suite, TQA_STACK_RESULT=str(result),
                   **task, TQA_CONNECTOR_NAME=CONNECTOR,
                   TQA_CONNECTOR_ARGUMENTS=json.dumps(ARGUMENTS),
                   TQA_CONNECTOR_FORMS_FILE=str(self.private_file("connector-forms", json.dumps(forms(self.credential)))),
                   TQA_CONNECTOR_TOKEN_FILE=str(self.private_file("connector-token", self.credential + "\n")))
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=BROWSER_TIMEOUT_SECONDS)
        print(self.redact(done.stdout)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        assert done.returncode == 0, "The browser part of WEB-E2E-010 (connectors) failed"
        return json.loads(result.read_text())

    def dump(self):
        """The gateway database as SQL text, taken inside the postgres container."""
        return self.command(self.configuration + ["exec", "-T", "postgres", "pg_dump", "-U", "tqa", "-d", "gateway"])

    def logs(self):
        text = self.command(self.configuration + ["logs", "--no-color", "gateway", "web", "fake-model", "upstream", "postgres"])
        return text + self.command(["docker", "logs", self.sandbox()])


# The UX review (UI-GUIDELINES §17): one task per state. Each task's first message names its script (the scripted
# model's prompt selection, TK-U-036), so the capture may drive them in any order.
UX_TASKS = ["credential", "offline", "slow"]
UX_PHRASES = {"credential": "File the crash in the tracker", "offline": "File it in the offline tracker",
              "slow": "Wait on the slow connector"}
# The credential task's first step is held (TK-U-035), long enough for the capture to remove the credential before the
# call is made, so the call itself finds none (a connector that cannot start is left out of the task instead).
UX_CREDENTIAL_WAIT_MS = 60000
UX_TURNS = {
    "credential": [("mcp__tracker__create_issue", {"title": "Crash on save", "labels": ["bug"]}),
                   "The tracker needs your credential first."],
    "offline": [("mcp__offline__create_issue", {"title": "Crash on save"}), "The offline tracker did not answer."],
    "slow": [("mcp__slow__wait", {}), "The slow connector timed out."],
}
UX_BROWSER_TIMEOUT_SECONDS = 1200
UX_DEADLINE_SECONDS = 1800
UX_SLOW_TIMEOUT_SECONDS = 3
# A pinned image no engine holds: the check refuses it (image_not_found), so the seeded default cannot be picked.
UX_MISSING_IMAGE = "sha256:" + "0" * 64


class UxStack(ConnectorStack):
    """The connector half's stack, arranged for screenshots of each state the night's web changes show (not a gate)."""

    def scenario(self):
        super().scenario()
        responses = []
        for name, turns in UX_TURNS.items():
            for index, turn in enumerate(turns):
                events = ([{"type": "text", "text": turn}] if isinstance(turn, str) else
                          [{"type": "tool_call", "name": turn[0], "arguments": turn[1], "split_into": 1}])
                reply = {"when": UX_PHRASES[name], "events": events,
                         "usage": {"prompt_tokens": 10, "completion_tokens": 10}}
                if name == "credential" and index == 0:
                    reply["wait_ms"] = UX_CREDENTIAL_WAIT_MS
                responses.append(reply)
        compose.ownership(self.directory, self.env)
        compose.write(self.directory / "scenario.json",
                      json.dumps({"model": "fixture", "responses": responses, "select": "prompt"}))
        storage = Path(self.env["TQA_STORAGE_ROOT"])
        ca = json.dumps(str(storage / "fixture-ca.pem"))
        seed = ""
        # Each starts its session normally and fails on the call itself (a connector that fails at start is left out of
        # the task): unreachable drops the connection mid-call; timeout answers after a 3-second call bound.
        for name, url, timeout in (("offline", "https://{}:8443/mcp-down".format(self.upstream), 60),
                                   ("slow", "https://{}:8443/mcp-slow".format(self.upstream), UX_SLOW_TIMEOUT_SECONDS)):
            seed += ("\n[[connectors]]\nname = {}\nurl = {}\nenabled_by_default = true\n"
                     "ca_file = {}\ncall_timeout_seconds = {}\n").format(json.dumps(name), json.dumps(url), ca, timeout)
        for name, label, image, default in (("workstation", "Workstation", self.runtime, False),
                                            ("minimal", "Minimal", self.runtime, False),
                                            ("legacy", "Legacy tools", UX_MISSING_IMAGE, True)):
            seed += ("\n[[templates]]\nname = {}\ndisplay_name = {}\nimage = {}\ncapabilities = [\"shell\"]\n"
                     "default = {}\n").format(json.dumps(name), json.dumps(label), json.dumps(image),
                                             "true" if default else "false")
        config = self.directory / "gateway.toml"
        compose.write(config, config.read_text() + seed)
        settled(config.read_text())

    def start(self):
        """The page server, the administrator, and one task per state, each in Workstation (the default is refused)."""
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])
        account = {"email": EMAIL, "password": self.env["TQA_STACK_PASSWORD"]}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        self.tasks = {}
        for name in UX_TASKS:
            task = self.request("/api/v1/conversations", {"title": "UX review: " + name, "model_route": "local",
                                                          "template": "workstation"})
            self.tasks[name] = task["id"]
        self.conversation = self.tasks["credential"]
        self.path = "/api/v1/conversations/" + self.conversation

    def capture(self, output):
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE="ux-review", TQA_UX_REVIEW_DIR=str(output),
                   TQA_UX_TASKS=json.dumps(self.tasks), TQA_STACK_RESULT=str(self.directory.parent / "ux-result.json"),
                   TQA_CONNECTOR_TOKEN_FILE=str(self.private_file("connector-token", self.credential + "\n")))
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=UX_BROWSER_TIMEOUT_SECONDS)
        print(self.redact(done.stdout)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        assert done.returncode == 0, "The UX review capture failed"
        # The credential case's effect, not only its line: the stored call was refused for the missing credential.
        path = "/api/v1/conversations/{}/events?after=0&limit=1000".format(self.tasks["credential"])
        results = [frame["event"] for frame in self.request(path)["items"]
                   if frame.get("event", {}).get("type") == "tool.result"]
        assert any(str(result.get("content", "")).startswith("connector_credential_missing") for result in results), \
            "The credential case's call was not refused for its missing credential: " + self.redact(json.dumps(
                [{key: result.get(key) for key in ("name", "status")} for result in results]))


# WEB-E2E-010's profile half: the person makes a profile with the connector in the browser and starts a task from it.
PROFILE = "tracker-work"


class ProfileStack(ConnectorStack):
    """The connector half's stack, with the connector off by default, so only a profile the person makes can bring it."""

    suite = "profile"

    def scenario(self):
        super().scenario()
        config = self.directory / "gateway.toml"
        text = config.read_text()
        assert text.count("enabled_by_default = true") == 1, "The seed must hold one connector enabled by default"
        compose.ownership(self.directory, self.env)
        compose.write(config, text.replace("enabled_by_default = true", "enabled_by_default = false"))
        settled(config.read_text())

    def start(self):
        """The page server and the person; a control task without a profile has no connector."""
        self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                           str(WEB_WAIT_SECONDS), "web"])
        account = {"email": EMAIL, "password": self.env["TQA_STACK_PASSWORD"]}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        control = self.request("/api/v1/conversations", {"title": "Without a profile", "model_route": "local"})
        assert control.get("connectors") == [] and not control.get("profile"), \
            "A task without a profile must start without the connector: " + json.dumps(control)
        self.conversation = None


# Every page renders per request with the CSP nonce (the CSP design; server-boundary.mjs names the proxy and layout).
# So the scan also reads each page live, signed in as the person whose credential it looks for: as HTML and as its
# server-component payload, the kind the browser half leaves unsearched. Ids are the task's own or synthetic: a page
# is the same shell for any id.
LIVE_PAGES = ["/", "/login", "/workspaces", "/channels", "/settings", "/settings/account", "/settings/tokens",
              "/settings/keys", "/settings/keys/shared", "/settings/git", "/settings/connector-credentials",
              "/settings/connectors", "/settings/models", "/settings/people", "/settings/profiles",
              "/settings/agent-profiles", "/settings/environments", "/settings/decision-engines",
              "/settings/internet-access", "/settings/limits", "/settings/usage", "/settings/status",
              "/channels/0190b2a4-7e1c-7d2a-9a55-3f1c2b8e4d01", "/join/synthetic-invitation-token",
              "/settings/people/0190b2a4-7e1c-7d2a-9a55-3f1c2b8e4d02"]
LIVE_PAGE_BYTES = 8 << 20
NONCE = re.compile(r"'nonce-([A-Za-z0-9+/=]+)'")


def live_page(stack, path, payload):
    """One signed-in fetch of a page: its body with this load's nonce replaced by a marker, the raw body, and the URL
    it ended at. urllib follows redirects, so a page that sends a signed-in person elsewhere (/login to /) is scanned
    as where it lands, and the messages name that URL. Each fetch must answer 200 (the id pages are client shells,
    whatever the id), be whole (under LIVE_PAGE_BYTES) and be what it claims: HTML holding the build's scripts, or a
    server-component payload."""
    headers = {"Accept": "text/html", "RSC": "1"} if payload else {"Accept": "text/html"}
    request = urllib.request.Request(stack.origin + path, headers=headers)
    try:
        response = stack.client.open(request, timeout=30)
    except urllib.error.HTTPError as error:
        raise AssertionError("{} answered {}, not 200".format(path, error.code)) from None
    with response:
        status, url = response.status, response.url
        policy = response.headers.get("Content-Security-Policy", "")
        kind = response.headers.get("Content-Type", "")
        data = response.read(LIVE_PAGE_BYTES + 1)
    where = "{} (at {})".format(path, url)
    assert status == 200, "{} answered {}, not 200".format(where, status)
    assert len(data) <= LIVE_PAGE_BYTES, "{} is longer than the scan reads".format(where)
    body = data.decode("utf-8", errors="replace")
    if payload:
        assert kind.startswith("text/x-component") and re.search(r"^[0-9a-f]+:", body, re.M), \
            "{} is not a server-component payload ({})".format(where, kind)
    else:
        assert kind.startswith("text/html") and "/_next/static/" in body, \
            "{} holds no build output, so it proves nothing ({})".format(where, kind)
    nonce = NONCE.search(policy)
    assert nonce, "{} came without the app's nonce policy: {!r}".format(where, policy[:200])
    # The first request's headers, the cookie processor's Cookie among them. urllib follows a redirect (Next's 307 to
    # /?_rsc) with a new request: its cookies are the jar's, which request_data reads, and its RSC header is short.
    sent = [value for _, value in request.header_items()] + list(request.unredirected_hdrs.values())
    return body.replace(nonce.group(1), "{nonce}"), body, url, sent


def request_data(stack, sent):
    """What a page may never hold that differs between loads (page_diff): the requests' header values, the cookie
    jar's values, the credential's forms, and the task's and its runs' ids."""
    jars = [handler.cookiejar for handler in stack.client.handlers
            if isinstance(handler, urllib.request.HTTPCookieProcessor)]
    cookies = [cookie.value for jar in jars for cookie in jar if cookie.value]
    runs = {frame.get("run_id") or frame.get("event", {}).get("run_id") for frame in stack.stored()}
    ids = [stack.conversation] + [run for run in runs if run]
    return page_diff.request_values(sent, cookies, forms(stack.credential) + ids)


def live_pages(stack):
    """Every page, twice, as HTML and as payload: beyond their nonces the two may differ only by a few fresh ids the
    server made (Next's render id among them), and never by request data (page_diff, agreed with the WEB-E2E-010
    owner). Returns every body, for the credential search."""
    bodies = []
    for path in LIVE_PAGES + ["/c/" + stack.conversation]:
        for payload in (False, True):
            first, raw, url, sent = live_page(stack, path, payload)
            second, _, _, again = live_page(stack, path, payload)
            # Beyond the nonce, only a few fresh server-made ids may differ, none of them request data (page_diff).
            reasons = page_diff.problems(first, second, request_data(stack, sent + again))
            assert not reasons, "{} ({}, at {}) differs between two loads: {}".format(
                path, "payload" if payload else "HTML", url, "; ".join(reasons))
            bodies.append(raw)
    return "\n".join(bodies)


def settled(text):
    """The seeded gateway.toml holds the connector and its header credential, and nothing ADR-0035 removed: no
    [permissions] and no connector class, which the gateway refuses at start."""
    config = tomllib.loads(text)
    connectors = [item for item in config.get("connectors", []) if item.get("name") == CONNECTOR]
    assert len(connectors) == 1, "The seed must hold one tracker connector"
    assert connectors[0].get("auth", {}).get("header") == "X-Api-Key", "The seed must name the X-Api-Key header"
    assert "permissions" not in config, "The seed must not hold [permissions] (ADR-0035)"
    assert not any("class" in item for item in config.get("connectors", [])), "No connector takes a class (ADR-0035)"


def found(forms_list, text):
    return [form for form in forms_list if form in text]


def check(stack, result):
    browser = stack.browser(result)
    if stack.conversation is None:
        # The profile half: the browser made the task from the profile it created.
        stack.conversation = browser["conversationId"]
        stack.path = "/api/v1/conversations/" + stack.conversation
    assert browser["conversationId"] == stack.conversation, "The browser drove another task"
    assert browser["responses"] > 0 and browser["found"] == [], "The credential reached the page or a response: " + \
        json.dumps(browser["found"])
    unsearched = browser["unsearched"]
    print("Response bodies not searched in the browser (path and type): " + json.dumps(unsearched), flush=True)
    # Two kinds may stay open or be dropped on navigation, and neither can hold a value typed during the run (run 9):
    # the event stream, whose frames are the events searched below; and Next.js server-component payloads
    # (text/x-component, mostly link prefetches). Only the listed dynamic routes render per request, each a client
    # component, and no server code reads request data: web/scripts/server-boundary.mjs checks this in `pnpm check`.
    allowed = ("text/event-stream", "text/x-component")
    assert all(any(kind in item for kind in allowed) for item in unsearched), \
        "Only event streams and server-component payloads may stay unsearched in the browser: " + json.dumps(unsearched)
    record = stack.upstream_get("/record")
    methods = [item["method"] for item in record["requests"]]
    assert "initialize" in methods and "tools/list" in methods, "The tool list did not arrive through the proxy: " + str(methods)
    assert not any(item["tqa_headers"] for item in record["requests"]), "A Tqa-* header reached the remote"
    calls = record["calls"]
    assert len(calls) == 1 and calls[0]["name"] == "create_issue", "Exactly one call must reach the remote"
    assert calls[0]["auth_valid"], "The remote must receive the person's credential"
    assert calls[0]["arguments"] == ARGUMENTS, "The remote got other arguments than the model's call"
    assert calls[0]["peer"] == stack.gateway_address(), "The call reached the remote other than through the gateway"
    if isinstance(stack, ProfileStack):
        task = stack.request(stack.path)
        profile = task.get("profile") or {}
        assert profile.get("name") == PROFILE, "The task was not made from the profile: " + json.dumps(profile)
        assert "connectors" in profile.get("sets", []) and "model_route" in profile.get("sets", []), \
            "The profile's copy does not set the route and connectors: " + json.dumps(profile)
        assert task.get("connectors") == [CONNECTOR], "Only the profile can bring the connector: " + json.dumps(task)
    values = forms(stack.credential)
    # Positive controls: each search finds a planted form, so an empty result means something.
    assert found(values, "x" + values[0] + "x") == [values[0]], "The form search misses a planted copy"
    dump = stack.dump()
    assert "mcp.tracker" in dump or CONNECTOR in dump, "The dump does not hold the connector, so it proves nothing"
    stored = json.dumps(stack.stored())
    assert "mcp.tracker.create_issue" in stored, "The events do not hold the call, so they prove nothing"
    pages = live_pages(stack)
    for place, text in (("the database", dump), ("the events", stored), ("the logs", stack.logs()),
                        ("a live page", pages)):
        assert found(values, text) == [], "The credential reached " + place
    if isinstance(stack, ProfileStack):
        print("WEB-E2E-010 (profiles): a profile made in the browser brought the connector no task gets otherwise; the task "
              "made from it on New task kept it (profile, sets, connectors); its call carried the person's credential "
              "once, through the gateway; the credential and its encoded forms are absent from the page, {} responses, "
              "the events, the database dump, every container log and every page fetched live".format(browser["responses"]),
              flush=True)
        return
    print("WEB-E2E-010 (connectors): the person's credential, added in the browser, reached the remote once in "
          "X-Api-Key through the gateway; the step's Details held the call's arguments; the credential and its encoded forms are "
          "absent from the page, {} responses, the events, the database dump, every container log and every page fetched "
          "live, each differing between loads only by its nonce".format(
              browser["responses"]), flush=True)


def interrupted(_number, _frame):
    raise InterruptedError("Web connector fixture interrupted or exceeded its deadline")


def main():
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-web-connector-")
    directory = compose.installation(temporary)
    env["TQA_STACK_PASSWORD"] = "synthetic-web-connector-" + os.urandom(16).hex()
    # `--ux <directory>`: the UX review's screenshots instead of WEB-E2E-010's checks (UI-GUIDELINES §17).
    ux = Path(sys.argv[sys.argv.index("--ux") + 1]).resolve() if "--ux" in sys.argv else None
    # `--profile`: WEB-E2E-010's profile half (the connector only through a profile made in the browser).
    kind = UxStack if ux else ProfileStack if "--profile" in sys.argv else ConnectorStack
    stack, failure = kind(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(UX_DEADLINE_SECONDS if ux else DEADLINE_SECONDS)
    try:
        stack.prepare("push")
        stack.start()
        if ux:
            stack.capture(ux)
        else:
            check(stack, directory.parent / "browser-result.json")
    except BaseException as error:
        failure = error
        print("Web connector contract failed: " + stack.redact(str(error)), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
