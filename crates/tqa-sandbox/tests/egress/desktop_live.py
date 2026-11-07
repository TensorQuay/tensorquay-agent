"""Computer use on a real stack (ADR-0039; the c15 review's P0-4): the `computer` tool in a Desktop sandbox the gateway
started, driven by runs of the scripted model, with the release's images and no test hook. Four cases, each on its own
owned stack:

- `tool`: RT-I-046 and RT-I-047 live. The browser by DOM ref (type, click, a mark), XTEST input landing, the settle
  rule's evidence (a before and an after capture around each acting action, each journaled as `screen.capture` ahead of
  its call's start or result, and served by the gateway byte for byte), `stale_ref` for a ref not in the snapshot and
  for a mark of an earlier screenshot, `screenshot_too_large` on a screen of noise, the per-run caps
  (`screenshot_limit`, `action_limit`, from the administrator's limits), the clipboard (no `ServerCutText` from the
  desktop's VNC server while the selections change, with a control server on the same display that does send it, and
  a paste in that arrives), Mousepad over AT-SPI, and set-of-marks with a page that paints a decoy "7".
- `record`: slice 3's record on the page. The `tool` case's acting, refused and too-detailed steps, and a task whose
  screenshot store is full ("Not kept."), read in the browser (`web/e2e/screen-stack.spec.ts`, phase `record`).
- `sign-in`: WEB-E2E-037 on the stack and SEC-059's stack half. The Screen panel watching a real run, its pictures and
  replay; Take over to sign in to a site by hand in the desktop's browser, Hand back; the run that follows is signed
  in, the one after is not; with a sign-in in the profile, a share holder's, a fork's and another of the owner's
  tasks on the workspace all start signed out; Sign out everywhere from the panel, and the next run is signed out; the
  site's cookie is never anywhere in a sandbox but the browser's tmpfs.
- `reap`: SBX-C-022. A run's shell command and the screen read after it; past the reaps that follow, every part of
  the display stack still runs, none of them under the runtime (its reaper kills every direct child it didn't start).

Run from the repository root on images built from the commit under test (the `record` and `sign-in` cases need the web
image and `pnpm install` in web/):
    PYTHONPATH=crates/tqa-sandbox/tests TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE=sha256:<gateway image id> \\
    TQA_WORKSTATION_IMAGE=<workstation image id> TQA_DESKTOP_TEST_IMAGE=<desktop image id> \\
    [TQA_WEB_IMAGE=sha256:<web image id>] python3 -B -m egress.desktop_live tool|record|sign-in|reap
"""
import argparse
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import signal
import struct
import subprocess
import tempfile
import time
import urllib.request

from . import desktop
from .stack import ROOT, Stack, compose, keep_logs
from .upstream import SESSION, SIGNED_IN, SIGNED_OUT

CASES = ("tool", "record", "sign-in", "reap")
DEADLINE_SECONDS = {"tool": 900, "record": 900, "sign-in": 1500, "reap": 600}
RUN_SECONDS = 240
# The runtime's browser's view of its cookies, through its debugging port (the port in its profile's
# DevToolsActivePort, Node's own WebSocket in the Desktop image): the fixture site's cookies by name and domain, never
# a value. Chromium writes its cookie store to disk only in batches, about every 30 s, and the run that follows a
# sign-in uses this same live browser, so its memory is what the sign-in is checked by (ADR-0039 section 6).
BROWSER_COOKIES = r"""
const fs = require('fs');
let port, path;
try { [port, path] = fs.readFileSync('/run/tqa-browser/profile/DevToolsActivePort', 'utf8').split('\n'); }
catch { console.log(JSON.stringify({error: 'no browser'})); process.exit(0); }
const ws = new WebSocket(`ws://127.0.0.1:${port}${path}`);
const fail = error => { console.log(JSON.stringify({error})); process.exit(0); };
ws.onerror = event => fail('websocket: ' + (event.message || event.type));
setTimeout(() => fail('timeout'), 20000);
ws.onopen = () => ws.send(JSON.stringify({id: 1, method: 'Storage.getCookies'}));
ws.onmessage = event => {
  const message = JSON.parse(event.data);
  if (message.id !== 1) return;
  if (message.error) fail(message.error.message);
  const named = (message.result.cookies || []).filter(cookie => cookie.name === process.argv[1]);
  console.log(JSON.stringify({cookies: named.map(cookie => ({name: cookie.name, domain: cookie.domain}))}));
  process.exit(0);
};
"""
# A run the gateway stops (the state quota) ends at once; this bound says so instead of the full one.
STOP_SECONDS = 90
BROWSER_TIMEOUT_SECONDS = 600
OUTPUT_TAIL_CHARACTERS = 6000
WEB_WAIT_SECONDS = 120
EMAIL = "desktop@example.invalid"
TEAMMATE_EMAIL = "desktop-teammate@example.invalid"

FORM = "file:///workspace/form.html"
MARKS = "file:///workspace/marks.html"
NOISE = "file:///workspace/noise.html"
DIALOG = "file:///workspace/dialog.html"
UPLOAD = "file:///workspace/upload.html"
# TOOL-U-062 live: the workspace file a page's file inputs are given, and its size the page reports.
REPORT = "Q3\n"
# What a paste in carries, from the raw VNC client to the page's text box.
PASTED = "pasted-in-fixture"
# The store's quota for the `record` case (the least `sandbox.state_bytes` takes), and how much of it the full task's
# own state fills: any screenshot is then too much, and none is kept.
STATE_BYTES = 16 << 20
FILLER_BYTES = STATE_BYTES - (16 << 10)
# Mousepad, started by the agent's shell on the desktop's display and session bus (desktop-init's own).
MOUSEPAD = ("cd /tmp && DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/tqa-desktop "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/tqa-desktop/bus "
            "setsid mousepad /workspace/notes.txt </dev/null >/dev/null 2>&1 & sleep 1; echo started")
LISTED = re.compile(r"^\s*e(\d+) (\S+) \"([^\"]*)\"", re.MULTILINE)
BUTTONS = "ABCDEFGHI"

computer = desktop.computer


def navigate(url):
    """The address bar, the page, and a wait for it: four actions."""
    return [computer("key", keys="ctrl+l"), computer("type", text=url), computer("key", keys="Return"),
            computer("wait", ms=1500)]


# Each run's calls, in order; a run ends with its own text, so each takes len(calls) + 1 of the model's turns.
def runs(upstream):
    whoami = "http://{}:8081/whoami".format(upstream)
    return {
        # RT-I-046 by DOM ref, the evidence, and both stale refusals.
        "act": navigate(FORM) + [computer("screenshot"), computer("type", ref="e1", text="Ada"),
                                 computer("click", ref="2"), computer("observe"), computer("click", ref="e9"),
                                 computer("click", ref="1")],
        # RT-I-047: mark 7 against a page painting its own "7" beside another button.
        "decoy": navigate(MARKS) + [computer("screenshot"), computer("click", ref="7"), computer("observe")],
        "detail": navigate(NOISE) + [computer("screenshot")],
        # RT-I-046's clipboard: the paste in, then a copy the desktop must never send out.
        "clipboard": navigate(FORM) + [computer("observe"), computer("click", ref="e1"),
                                       computer("key", keys="ctrl+v"), computer("observe"),
                                       computer("key", keys="ctrl+a"), computer("key", keys="ctrl+c"),
                                       computer("wait", ms=1500)],
        # TOOL-U-062 live: the styled button's chooser answered by a type with no ref, the plain input by ref, then a
        # runtime file and a '..' path refused before the browser sees them.
        "upload": navigate(UPLOAD) + [computer("observe"), computer("click", ref="e2"),
                                      computer("type", text="report.txt"), computer("observe"),
                                      computer("type", ref="e1", text="report.txt"), computer("observe"),
                                      computer("type", ref="e1", text="/run/tqa-browser/profile/Local State"),
                                      computer("type", ref="e1", text="../etc/passwd")],
        # RT-I-046's AT-SPI cases and RT-I-047's marks on an application.
        "atspi": [("shell", {"command": MOUSEPAD}), computer("wait", ms=2000), computer("observe"),
                  computer("screenshot"), computer("key", keys="alt+f"), computer("observe"),
                  computer("key", keys="Escape")],
        # With the caps at one screenshot and four actions: the navigation spends the four.
        "caps": navigate(FORM) + [computer("screenshot"), computer("screenshot"), computer("key", keys="Escape")],
        # TOOL-U-057 live: a click that opens a JavaScript dialog, then Return answers it as a person's key, through
        # XTEST to Chromium's own dialog.
        "dialog": navigate(DIALOG) + [computer("observe"), computer("click", ref="e1"), computer("key", keys="Return"),
                                      computer("observe")],
        # A task whose state is full: the gateway stops the run before anything is captured.
        "state-full": navigate(FORM) + [computer("screenshot"), computer("click", ref="2")],
        # WEB-E2E-037's watched run: a picture, an acting step with its evidence, and a stale ref on the page.
        "watched": navigate(FORM) + [computer("screenshot"), computer("click", ref="2"), computer("observe"),
                                     computer("click", ref="e9")],
        # SEC-059: who the site thinks the browser is.
        "whoami": navigate(whoami) + [computer("observe")],
        # SBX-C-022: a command, after which the runtime reaps what it leaves behind, then the screen read.
        # SBX-C-022: an application opened from the shell and a plain background process, the reap after that call,
        # then a later call that says which survived; the screen read last.
        "reap": [("shell", {"command": REAP_OPEN}), ("shell", {"command": REAP_CHECK}), computer("observe")],
    }


# The runs of each case, in the order the driver sends them (CONVERSATIONS says which task's script answers each).
ORDER = {
    "tool": ["act", "decoy", "detail", "clipboard", "upload", "atspi", "caps"],
    "record": ["act", "detail", "dialog", "caps", "state-full"],
    # The watched run and the two after the Take over's sign-in; a fork's task; the run after Sign out everywhere; a
    # share holder's and another of the owner's tasks.
    "sign-in": ["watched", "whoami", "whoami", "whoami", "whoami", "whoami", "whoami"],
    "reap": ["reap"],
}

# Each case's conversations, by the phrase of their first message and the runs each holds in order. The scripted model
# serves a conversation only the replies naming its phrase, by its own turn ("select": "prompt"): by turn alone a new
# task started over at the first run's calls (c16's record rerun, 12 Oct 2026: the capped task got the act run's).
# A phrase several conversations share is one script each of them follows from its start.
CONVERSATIONS = {
    "tool": [("Do the act steps", ["act", "decoy", "detail", "clipboard", "upload", "atspi"]),
             ("Do the caps steps", ["caps"])],
    "record": [("Do the act steps", ["act", "detail", "dialog"]), ("Do the caps steps", ["caps"]),
               ("Do the state-full steps", ["state-full"])],
    # The browser sends the sign-in task's messages (web/e2e/screen-stack.spec.ts); the run after Sign out everywhere
    # is that task's fourth. A fork's, a share holder's and another task's desktops each run whoami once.
    "sign-in": [("Open the form and save it", ["watched", "whoami", "whoami", "whoami"]),
                ("Do the whoami steps", ["whoami"])],
    "reap": [("Do the reap steps", ["reap"])],
}


def scenario(case, upstream):
    """The scripted model's turns for `case`, conversation by conversation: each run's calls, one per turn, then its
    closing text, all naming the conversation's phrase."""
    table, responses = runs(upstream), []
    for phrase, names in CONVERSATIONS[case]:
        for index, name in enumerate(names):
            for tool, arguments in table[name]:
                responses.append({"when": phrase, "events": [{"type": "tool_call", "name": tool,
                                                              "arguments": arguments, "split_into": 1}]})
            responses.append({"when": phrase, "events": [
                {"type": "text", "text": "run {} ({}) done".format(index + 1, name)}]})
    for response in responses:
        response["usage"] = {"prompt_tokens": 10, "completion_tokens": 10}
    return {"model": "fixture", "select": "prompt", "responses": responses}


def form_page():
    return ("<!doctype html><title>Form</title><body style='margin:40px;font:20px sans-serif'>"
            "<input aria-label='Name' style='font-size:20px'> "
            "<button onclick=\"document.title='saved '+document.querySelector('input').value\">Save</button>"
            "</body>\n")


def dialog_page():
    """One button whose confirm() names its answer in the title."""
    return ("<!doctype html><title>Dialog</title><body style='margin:40px;font:20px sans-serif'>"
            "<button onclick=\"document.title=confirm('Sure?')?'confirmed':'declined'\">Ask</button></body>\n")


def upload_page():
    """A plain file input, then a styled Upload button that opens a hidden one's chooser; each names what it got in the
    title."""
    named = "document.title='{} '+this.files[0].name+' '+this.files[0].size"
    return ("<!doctype html><title>Upload</title><body style='margin:40px;font:20px sans-serif'>"
            "<input type=file aria-label='Report file' onchange=\"" + named.format("chose") + "\"> "
            "<button onclick=\"document.getElementById('hidden').click()\">Upload</button>"
            "<input type=file id=hidden style='display:none' onchange=\"" + named.format("styled") + "\">"
            "</body>\n")


def marks_page():
    """Nine buttons in a column; beside the third, the page paints its own red "7", a decoy for mark 7."""
    buttons = "".join("<p><button onclick=\"document.title='pressed Button {0}'\">Button {0}</button></p>".format(letter)
                      for letter in BUTTONS)
    decoy = ("<span style='position:absolute;left:150px;top:{}px;background:#c00;color:#fff;font:bold 14px sans-serif;"
             "padding:1px 4px'>7</span>").format(40 + 2 * 46)
    return ("<!doctype html><title>Marks</title><body style='margin:40px;font:16px sans-serif;position:relative'>"
            + buttons + decoy + "</body>\n")


def noise_page():
    """A screen of random pixels: too much detail for any size the store takes."""
    return ("<!doctype html><title>Noise</title><body style='margin:0;overflow:hidden'>"
            "<canvas id=c width=1920 height=1200 style='width:100vw;height:100vh'></canvas><script>"
            "const g=document.getElementById('c').getContext('2d'),d=g.createImageData(1920,1200);"
            "for(let i=0;i<d.data.length;i+=65536)crypto.getRandomValues(d.data.subarray(i,i+65536));"
            "for(let i=3;i<d.data.length;i+=4)d.data[i]=255;g.putImageData(d,0,0);</script></body>\n")


# RT-I-046's clipboard probe, run in the sandbox as its user: a raw RFB 3.8 client on the desktop's VNC server and one
# on a control server (the same display, x11vnc's clipboard left at its defaults). It sends the paste in on the
# desktop's, says it is ready, and for `seconds` records every ServerCutText each server sends. It never asks for a
# framebuffer update, so nothing else is sent but a bell.
RFB_PROBE = r'''import json, socket, struct, sys, threading, time

def exact(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise EOFError("closed")
        data += chunk
    return data

def connect(port):
    sock = socket.create_connection(("127.0.0.1", port), timeout=5)
    exact(sock, 12)
    sock.sendall(b"RFB 003.008\n")
    types = exact(sock, exact(sock, 1)[0])
    assert 1 in types, "security types: " + repr(types)
    sock.sendall(b"\x01")
    assert exact(sock, 4) == b"\0\0\0\0", "security refused"
    sock.sendall(b"\x01")
    init = exact(sock, 24)
    exact(sock, struct.unpack(">I", init[20:24])[0])
    return sock

def watch(sock, found, until):
    while time.monotonic() < until:
        sock.settimeout(max(0.1, until - time.monotonic()))
        try:
            kind = exact(sock, 1)[0]
        except (socket.timeout, TimeoutError):
            return
        if kind == 3:
            exact(sock, 3)
            found.append(exact(sock, struct.unpack(">I", exact(sock, 4))[0]).decode("latin-1"))
        elif kind != 2:
            found.append("unexpected message {}".format(kind))
            return

seconds, pasted, out = int(sys.argv[1]), sys.argv[2], sys.argv[3]
desktop, control = connect(5900), connect(5901)
text = pasted.encode("latin-1")
desktop.sendall(b"\x06\0\0\0" + struct.pack(">I", len(text)) + text)
open(out + ".ready", "w").close()
seen = {"desktop": [], "control": []}
until = time.monotonic() + seconds
threads = [threading.Thread(target=watch, args=(sock, seen[name], until))
           for name, sock in (("desktop", desktop), ("control", control))]
for thread in threads:
    thread.start()
for thread in threads:
    thread.join()
with open(out, "w") as result:
    json.dump(seen, result)
'''
PROBE_OUT = "/tmp/rt-i-046-cut.json"


def listing(content):
    """A snapshot's elements by ref: {"e7": ("button", "Button G")}."""
    return {"e" + number: (role, name) for number, role, name in LISTED.findall(content)}


def png_size(data):
    """A PNG's width and height from its header."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR", "not a PNG"
    return struct.unpack(">II", data[16:24])


class LiveStack(Stack):
    """The egress stack with a Desktop pinned, the route offering images, room for four desktops, and its own runs."""

    def scenario(self):
        flavour = self.case.removeprefix("live-")
        compose.write(self.directory / "scenario.json", json.dumps(scenario(flavour, self.upstream)))
        models = self.directory / "models.toml"
        compose.write(models, models.read_text() + "vision=true\n")
        config = self.directory / "gateway.toml"
        sandbox = "[sandbox]\n" + ("" if "max_live=" in config.read_text() else "max_live=8\n")
        if flavour == "record":
            sandbox += "state_bytes={}\n".format(STATE_BYTES)
        compose.write(config, config.read_text().replace("[sandbox]\n", sandbox, 1))

    def start(self):
        if self.case not in ("live-tool", "live-reap"):
            self.command(self.configuration + ["up", "-d", "--no-build", "--no-deps", "--wait", "--wait-timeout",
                                               str(WEB_WAIT_SECONDS), "web"])
        password = "synthetic-desktop-" + secrets.token_hex(16)
        self.env["TQA_STACK_PASSWORD"] = password
        account = {"email": EMAIL, "password": password}
        self.request("/api/v1/auth/bootstrap", dict(account, token=self.env["TQA_BOOTSTRAP_TOKEN"]))
        self.request("/api/v1/auth/login", account)
        self.offered("desktop")
        self.order = list(ORDER[self.case.removeprefix("live-")])

    def redact(self, text):
        return compose.redact(text, self.env)

    # --- tasks and runs -----------------------------------------------------------------------------------------------

    def task(self, title, workspace=None, files=True):
        """A new Desktop task (on `workspace` when given), with the fixture pages in its workspace."""
        body = {"title": title, "model_route": "local", "template": "desktop"}
        if workspace:
            body["workspace_id"] = workspace
        conversation = self.request("/api/v1/conversations", body)
        if files:
            self.files(conversation["id"])
        return conversation

    def files(self, conversation, extra=None):
        compose.ownership(self.directory, self.env)
        workspace = Path(self.env["TQA_STORAGE_ROOT"]) / "workspaces" / conversation
        workspace.mkdir(parents=True, exist_ok=True)
        pages = {"form.html": form_page(), "marks.html": marks_page(), "noise.html": noise_page(),
                 "dialog.html": dialog_page(), "upload.html": upload_page(), "report.txt": REPORT,
                 "notes.txt": "Notes for the AT-SPI case.\n", "rfb_probe.py": RFB_PROBE}
        for name, text in dict(pages, **(extra or {})).items():
            compose.write(workspace / name, text)
            (workspace / name).chmod(0o644)
        compose.ownership(self.directory, self.env, "1000:1000")

    def events(self, conversation):
        items, after = [], 0
        while True:
            page = self.request("/api/v1/conversations/{}/events?after={}&limit=1000".format(conversation, after))
            items += page["items"]
            if len(page["items"]) < 1000:
                return items
            after = items[-1]["seq"]

    def ended(self, conversation):
        return [item for item in self.events(conversation)
                if item["event"]["type"] in ("run.completed", "run.failed")]

    def run(self, conversation, expected, text=None, end="run.completed", seconds=RUN_SECONDS):
        """Sends a message and waits for its run to end as `end`, within `seconds`; returns that run's events.
        `expected` names the scripted run it must be, which keeps the driver and the scenario's order in step."""
        assert self.order and self.order[0] == expected, "scenario order: next is {}, not {}".format(
            self.order[:1], expected)
        before = len(self.ended(conversation))
        self.request("/api/v1/conversations/{}/messages".format(conversation),
                     {"text": text or "Do the {} steps.".format(expected), "attachments": []})
        self.order.pop(0)
        return self.awaited(conversation, before, end, seconds)

    def awaited(self, conversation, before, end="run.completed", seconds=RUN_SECONDS):
        """The events of the run that ends after `before` runs had, ended as `end`; any other end is said."""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            items = self.events(conversation)
            ends = [index for index, item in enumerate(items)
                    if item["event"]["type"] in ("run.completed", "run.failed", "run.cancelled")]
            if len(ends) > before:
                last = ends[before]
                start = max((index for index, item in enumerate(items[:last])
                             if item["event"]["type"] == "run.started"), default=0)
                assert items[last]["event"]["type"] == end, "the run ended otherwise than {}: {}".format(
                    end, self.redact(json.dumps(items[last]["event"])[:800]))
                return [item for item in items[start:last + 1]]
            time.sleep(1)
        raise AssertionError("the run on {} did not end within {} s".format(conversation, seconds))

    def picture(self, conversation, screenshot):
        request = urllib.request.Request(
            "{}/api/v1/conversations/{}/screenshots/{}".format(self.origin, conversation, screenshot),
            headers={"Origin": self.origin})
        with self.client.open(request, timeout=15) as response:
            assert response.headers.get("Content-Type", "").startswith("image/png"), response.headers
            return response.read(600_000)

    def limit(self, key, value):
        """Sets an administrator's limit, at its current version."""
        items = self.request("/api/v1/admin/limits")["items"]
        current = next(item for item in items if item["key"] == key)
        if current["value"] != value:
            self.request("/api/v1/admin/limits/" + key, {"value": value, "version": current["version"]}, method="PUT")

    def sandbox_of(self, conversation):
        found = self.command(["docker", "ps", "-q", "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                              "--filter", "label=tqa.conversation=" + conversation]).split()
        assert len(found) == 1, "expected one sandbox for {}, found {}".format(conversation, len(found))
        return found[0]

    def in_sandbox(self, conversation, arguments, detach=False):
        return self.command(["docker", "exec"] + (["-d"] if detach else []) + [self.sandbox_of(conversation)]
                            + arguments)

    def cookies(self, conversation):
        """Where in the sandbox the fixture site's cookie is: any file holding its name (Chromium's cookie store keeps
        names in the clear)."""
        found = self.in_sandbox(conversation, ["sh", "-c", "grep -rlaF {} /home /workspace /tmp /run/tqa-browser "
                                               "2>/dev/null; true".format(SESSION)])
        return sorted(line for line in found.splitlines() if line.strip())

    def logins(self):
        """How many times the fixture site has served its login page (upstream.py's /logins), read inside the upstream
        container: whether a sign-in by hand reached the site at all."""
        page = self.command(self.configuration + [
            "exec", "-T", "upstream", "python3", "-c",
            "import urllib.request; print(urllib.request.urlopen('http://127.0.0.1:8081/logins', timeout=5).read()"
            ".decode())"])
        return int(re.search(r"<title>(\d+)</title>", page).group(1))

    def browser_up(self, conversation):
        """Whether the runtime's browser is running: its debugging port file is there and answers."""
        found = json.loads(self.in_sandbox(conversation, ["node", "-e", BROWSER_COOKIES, SESSION]) or "{}")
        return found.get("error") != "no browser"

    def browser_cookies(self, conversation):
        """The fixture site's cookies in the runtime's browser's memory (name and domain, never a value); none when
        the browser holds none, or there is no browser."""
        found = json.loads(self.in_sandbox(conversation, ["node", "-e", BROWSER_COOKIES, SESSION]) or "{}")
        assert found.get("error") in (None, "no browser"), "the browser's cookies weren't read: {}".format(found)
        return sorted((cookie["name"], cookie["domain"]) for cookie in found.get("cookies", []))

    # --- people ---------------------------------------------------------------------------------------------------------

    def teammate(self):
        """A member the administrator invited; their own session's client."""
        invitation = self.request("/api/v1/invitations", {"email": TEAMMATE_EMAIL, "role": "member"})
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        own, self.client = self.client, client
        try:
            self.request("/api/v1/invitations/accept",
                         {"token": invitation["token"], "password": self.env["TQA_STACK_PASSWORD"]})
            self.request("/api/v1/auth/login", {"email": TEAMMATE_EMAIL, "password": self.env["TQA_STACK_PASSWORD"]})
        finally:
            self.client = own
        return client

    def as_person(self, client, work):
        own, self.client = self.client, client
        try:
            return work()
        finally:
            self.client = own

    def browser(self, phase, result, **extra):
        """Runs `web/e2e/screen-stack.spec.ts`'s `phase`; returns what it wrote to `result`."""
        env = dict(self.env, TQA_STACK_URL=self.origin, TQA_STACK_SUITE="screen", TQA_SCREEN_PHASE=phase,
                   TQA_STACK_RESULT=str(result), TQA_SCREEN_EMAIL=EMAIL, **extra)
        args = ["corepack", "pnpm", "exec", "playwright", "test", "--config", "playwright.stack.config.ts"]
        done = subprocess.run(args, env=env, cwd=ROOT / "web", stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              text=True, timeout=BROWSER_TIMEOUT_SECONDS)
        print(self.redact(done.stdout)[-OUTPUT_TAIL_CHARACTERS:], flush=True)
        assert done.returncode == 0, "the browser's {} phase failed".format(phase)
        return json.loads(result.read_text())


# --- what a run's events say ------------------------------------------------------------------------------------------

def calls(events):
    """Each tool call of a run, in order: its start's and result's seq, the result, and its captures by phase."""
    found, by_id = [], {}
    for item in events:
        event = item["event"]
        if event["type"] == "tool.start":
            # A before capture is journaled ahead of its call's start, so the call may be known already.
            step = by_id.setdefault(event["call_id"], {"captures": {}})
            step["start"] = item["seq"]
            found.append(step)
        elif event["type"] == "tool.result":
            by_id[event["call_id"]].update(result=event, end=item["seq"])
        elif event["type"] == "screen.capture":
            by_id.setdefault(event["call_id"], {"captures": {}})
            by_id[event["call_id"]]["captures"][event["phase"]] = dict(event, seq=item["seq"])
    return found


def captures(events):
    return [item["event"] for item in events if item["event"]["type"] == "screen.capture"]


def served(stack, conversation, capture):
    """The gateway serves a capture's picture exactly as journaled: its size, its SHA-256 and its dimensions."""
    data = stack.picture(conversation, capture["id"])
    assert len(data) == capture["bytes"], "the served picture's size differs from its capture"
    assert hashlib.sha256(data).hexdigest() == capture["sha256"], "the served picture differs from its capture"
    assert png_size(data) == (capture["width"], capture["height"]), "the picture's dimensions differ"


def refused(step, code, words):
    result = step["result"]
    assert result["status"] == "error" and result["content"].startswith(code + ": "), result
    assert words in result["content"], result["content"]
    assert "evidence" not in result and "target" not in result, "a refused action carries no evidence or target"


def acted(stack, conversation, step, role, name):
    """An acting action on an element: its target, both captures kept, unmarked, never sent, and in order."""
    result = step["result"]
    assert result["status"] == "ok", result
    assert result.get("target") == {"role": role, "name": name}, result.get("target")
    assert result.get("evidence") == {"before": True, "after": True}, result.get("evidence")
    before, after = step["captures"].get("before"), step["captures"].get("after")
    assert before and after, "an acting action has a before and an after capture: {}".format(step["captures"])
    assert before["seq"] < step["start"] < after["seq"] < step["end"], "captures out of order: {}".format(step)
    for capture in (before, after):
        assert not capture["marked"] and not capture["sent_to_model"], capture
        served(stack, conversation, capture)


def looked(stack, conversation, step):
    """The model's own screenshot: one capture, marked and sent, ahead of its result; evidence after only."""
    result = step["result"]
    assert result["status"] == "ok" and result["content"].startswith("Screenshot "), result["content"][:200]
    assert result.get("evidence") == {"after": True}, result.get("evidence")
    shot = step["captures"].get("screenshot")
    assert shot and set(step["captures"]) == {"screenshot"}, step["captures"]
    assert shot["marked"] and shot["sent_to_model"] and shot["seq"] < step["end"], shot
    served(stack, conversation, shot)
    return listing(result["content"])


def act_run(stack, conversation, events):
    """RT-I-046 by DOM ref: XTEST typing and a mark's click land; the evidence; both stale refusals."""
    steps = calls(events)
    assert len(steps) == 10, [step["result"]["status"] for step in steps]
    elements = looked(stack, conversation, steps[4])
    assert elements.get("e1", (None, None))[1] == "Name" and elements.get("e2") == ("button", "Save"), elements
    acted(stack, conversation, steps[5], elements["e1"][0], "Name")
    acted(stack, conversation, steps[6], "button", "Save")
    assert "saved Ada" in steps[7]["result"]["content"], "the typed text and the click didn't land"
    refused(steps[8], "stale_ref", "e9 isn't in snapshot")
    refused(steps[9], "stale_ref", "mark 1 belongs to an earlier screenshot")
    print("RT-I-046 live: typed and clicked by ref and mark in the browser; before and after captured, journaled in "
          "order and served exactly; a missing ref and an old mark refused as stale_ref", flush=True)


def decoy_run(stack, conversation, events):
    """RT-I-047: mark 7 presses the snapshot's e7, whatever the page paints beside another button."""
    steps = calls(events)
    elements = looked(stack, conversation, steps[4])
    assert [elements.get("e{}".format(index + 1)) for index in range(len(BUTTONS))] == [
        ("button", "Button " + letter) for letter in BUTTONS], elements
    acted(stack, conversation, steps[5], "button", "Button G")
    after = steps[6]["result"]["content"]
    assert "pressed Button G" in after and "pressed Button C" not in after, after[:400]
    print("RT-I-047 live: mark 7 pressed e7 (Button G), not the button the page painted its own 7 beside", flush=True)


def detail_run(events):
    steps = calls(events)
    refused(steps[4], "screenshot_too_large", "too much detail")
    assert not steps[4]["captures"], "a refused screenshot is never stored"
    print("RT-I-046 live: a screen of noise refused as screenshot_too_large, nothing stored", flush=True)


def upload_run(events):
    """TOOL-U-062 live: a page's file inputs get a workspace file with no native dialog; a runtime file and '..' never."""
    steps = calls(events)
    assert len(steps) == 12, [step["result"]["status"] for step in steps]
    asked = steps[5]["result"]["content"]
    assert "The page asks for a file" in asked, asked[:400]
    size = len(REPORT.encode())
    assert steps[6]["result"]["content"].startswith("Chose report.txt"), steps[6]["result"]
    assert "styled report.txt {}".format(size) in steps[7]["result"]["content"], "the styled button's input got nothing"
    assert steps[8]["result"]["content"].startswith("Chose report.txt"), steps[8]["result"]
    assert "chose report.txt {}".format(size) in steps[9]["result"]["content"], "the plain input got nothing"
    refused(steps[10], "invalid_args", "only a file in /workspace can be chosen")
    refused(steps[11], "invalid_args", "named without '..'")
    print("TOOL-U-062 live: both file inputs got /workspace/report.txt with no native dialog; a runtime file and a "
          "'..' path refused before the browser saw them", flush=True)


def dialog_run(events):
    """TOOL-U-057 live: the dialog a click opened is answered by Return through XTEST, as a person's key."""
    steps = calls(events)
    assert all(step["result"]["status"] == "ok" for step in steps), [step["result"] for step in steps]
    assert 'A dialog opened: "Sure?"' in steps[5]["result"]["content"], steps[5]["result"]["content"][:400]
    after = steps[7]["result"]["content"]
    assert "Window: confirmed" in after, "Return through XTEST didn't answer the dialog: " + after[:400]
    print("TOOL-U-057 live: a JavaScript dialog opened by a click was answered by Return through XTEST", flush=True)


def caps_run(stack, conversation, events):
    steps = calls(events)
    looked(stack, conversation, steps[4])
    refused(steps[5], "screenshot_limit", "as many screenshots as it may")
    refused(steps[6], "action_limit", "as many computer actions as it may")
    print("RT-I-046 live: the administrator's caps (1 screenshot, 4 actions) refused the next of each", flush=True)


def atspi_run(stack, conversation, events):
    steps = calls(events)
    # The calls' result texts, kept when asked, so two images' AT-SPI snapshots can be compared byte for byte (the
    # accessibility reader's move off the atspi crate, ADR-0039 slice 2, amended 18 Oct 2026).
    if os.environ.get("TQA_DESKTOP_LIVE_RECORD"):
        kept = Path(os.environ["TQA_DESKTOP_LIVE_RECORD"])
        kept.mkdir(parents=True, exist_ok=True)
        (kept / "atspi.json").write_text(json.dumps(
            [{"call": index, "status": step.get("result", {}).get("status"),
              "content": step.get("result", {}).get("content")} for index, step in enumerate(steps)], indent=1))
    assert steps[0]["result"]["status"] == "ok" and "started" in steps[0]["result"]["content"], steps[0]["result"]
    tree = steps[2]["result"]["content"]
    assert "the window's accessibility tree" in tree and 'menu "File"' in tree, tree[:600]
    looked(stack, conversation, steps[3])
    opened = steps[5]["result"]["content"]
    assert 'menuitem "Save' in opened, "alt+f opened Mousepad's File menu: " + opened[:600]
    print("RT-I-046/047 live: Mousepad read over AT-SPI, marked, and its File menu opened by XTEST keys", flush=True)


def clipboard_run(steps, seen):
    pasted = steps[7]["result"]["content"]
    assert PASTED in pasted, "the paste in didn't arrive: " + pasted[:400]
    assert seen["desktop"] == [], "the desktop's VNC server sent cut text: {}".format(seen["desktop"])
    assert any(PASTED in text for text in seen["control"]), \
        "the control server never sent the copy, so the check proves nothing: {}".format(seen["control"])
    print("RT-I-046 live: a paste in arrived; the copy reached the control server's client and never the desktop's",
          flush=True)


def clipboard(stack, conversation):
    """Starts the control VNC server and the probe, runs the clipboard steps, and checks what each client got."""
    stack.in_sandbox(conversation, ["x11vnc", "-display", ":0", "-localhost", "-rfbport", "5901", "-nopw", "-forever",
                                    "-shared", "-quiet"], detach=True)
    time.sleep(2)
    stack.in_sandbox(conversation, ["python3", "/workspace/rfb_probe.py", "45", PASTED, PROBE_OUT], detach=True)
    deadline = time.monotonic() + 20
    while stack.in_sandbox(conversation, ["sh", "-c", "test -e {}.ready && echo ready; true".format(PROBE_OUT)]) != "ready":
        assert time.monotonic() < deadline, "the RFB probe didn't connect"
        time.sleep(0.5)
    events = stack.run(conversation, "clipboard")
    deadline = time.monotonic() + 60
    while True:
        out = stack.in_sandbox(conversation, ["sh", "-c", "cat {} 2>/dev/null; true".format(PROBE_OUT)])
        if out:
            break
        assert time.monotonic() < deadline, "the RFB probe didn't finish"
        time.sleep(1)
    # [r]: the pattern mustn't match this shell's own command line, or pkill ends it (exit 143, the 16d integration).
    stack.in_sandbox(conversation, ["sh", "-c", "pkill -f '[r]fbport 5901'; true"])
    clipboard_run(calls(events), json.loads(out))


def whoami(events):
    """What the site said to the run's browser: signed in or out."""
    content = calls(events)[4]["result"]["content"]
    if SIGNED_IN in content:
        return "in"
    assert SIGNED_OUT in content, "the site's page wasn't read: " + content[:400]
    return "out"


# --- the cases ----------------------------------------------------------------------------------------------------------

def tool(stack):
    first = stack.task("Computer use, live")["id"]
    act_run(stack, first, stack.run(first, "act"))
    decoy_run(stack, first, stack.run(first, "decoy"))
    detail_run(stack.run(first, "detail"))
    clipboard(stack, first)
    upload_run(stack.run(first, "upload"))
    atspi_run(stack, first, stack.run(first, "atspi"))
    # The caps apply from a session's start: a new task gets them.
    stack.limit("computer.max_screenshots_per_run", 1)
    stack.limit("computer.max_actions_per_run", 4)
    capped = stack.task("Computer use, capped")["id"]
    caps_run(stack, capped, stack.run(capped, "caps"))
    stack.limit("computer.max_screenshots_per_run", 200)
    stack.limit("computer.max_actions_per_run", 1000)


def record(stack, directory):
    # Three desktops stay up (the record's, the capped and the full one): past the cap a desktop waits for one to end,
    # and an idle one isn't retired for it (the default of 2 held the full task's run in the queue).
    slots = next(item["value"] for item in stack.request("/api/v1/admin/limits")["items"]
                 if item["key"] == "admission.slots")
    stack.limit("admission.slots", max(3, slots))
    stack.limit("desktop.max_sessions", 3)
    first = stack.task("Computer use, record")["id"]
    act_run(stack, first, stack.run(first, "act"))
    detail_run(stack.run(first, "detail"))
    dialog_run(stack.run(first, "dialog"))
    stack.limit("computer.max_screenshots_per_run", 1)
    stack.limit("computer.max_actions_per_run", 4)
    capped = stack.task("Computer use, capped")["id"]
    caps_run(stack, capped, stack.run(capped, "caps"))
    stack.limit("computer.max_screenshots_per_run", 200)
    stack.limit("computer.max_actions_per_run", 1000)
    # The full task: its state left with too little room for the run's own, so the gateway stops the run (GW-I-091)
    # before any capture: with one quota for the state and the kept screenshots (GW-I-221), a store too full for a
    # picture is soon a state past its bound, and no margin keeps a run going with its pictures refused.
    full = stack.task("Computer use, store full")["id"]
    compose.ownership(stack.directory, stack.env)
    state = Path(stack.env["TQA_STORAGE_ROOT"]) / "runtime-state" / full
    # Private, as the gateway makes its own (storage.rs's create_private refuses a group- or world-readable one).
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(state, 0o700)
    with open(state / "filler", "wb") as filler:
        filler.truncate(0)
        for _ in range(FILLER_BYTES // (1 << 20)):
            filler.write(secrets.token_bytes(1 << 20))
        filler.write(secrets.token_bytes(FILLER_BYTES % (1 << 20)))
    compose.ownership(stack.directory, stack.env, "1000:1000")
    events = stack.run(full, "state-full", end="run.cancelled", seconds=STOP_SECONDS)
    assert not captures(events), "a task past its state bound kept a screenshot"
    print("GW-I-091/221 live: a task whose state was full had its run stopped, and nothing captured", flush=True)
    found = stack.browser("record", directory / "record.json", TQA_SCREEN_TASK=first, TQA_SCREEN_CAPPED=capped,
                          TQA_SCREEN_FULL=full)
    assert found.get("checked") == ["acted", "stale", "too-detailed", "caps", "state-full"], found
    print("Slice 3 live, on the page: the click's Before and After from the gateway, the stale step, the too-detailed "
          "screenshot and both caps each said in words; the full task's run reads Stopped, with no picture", flush=True)


def sign_in(stack, directory):
    login = "http://{}:8081/login".format(stack.upstream)
    stack.limit("admission.slots", max(4, next(item["value"] for item in stack.request("/api/v1/admin/limits")["items"]
                                                if item["key"] == "admission.slots")))
    stack.limit("desktop.max_sessions", 4)
    own = stack.task("Desktop sign-in", files=True)
    task, workspace = own["id"], own["workspace_id"]

    # WEB-E2E-037: the watched run in the browser, then a Take over to sign in, Hand back, and two runs it sends.
    # The browser sends these three runs' messages.
    assert stack.order[:3] == ["watched", "whoami", "whoami"], stack.order
    del stack.order[:3]
    found = stack.browser("watch", directory / "watch.json", TQA_SCREEN_TASK=task, TQA_SCREEN_LOGIN=login)
    assert found.get("checked") == ["watching", "pictures", "replay", "stale", "take-over", "handed-back"], found
    events = stack.events(task)
    ends = [index for index, item in enumerate(events) if item["event"]["type"] in ("run.completed", "run.failed")]
    assert len(ends) == 3 and all(events[end]["event"]["type"] == "run.completed" for end in ends), \
        "the browser's three runs completed: {}".format([events[end]["event"]["type"] for end in ends])
    runs_of = []
    for end in ends:
        start = max(position for position, item in enumerate(events[:end]) if item["event"]["type"] == "run.started")
        runs_of.append(events[start:end + 1])
    watched = calls(runs_of[0])
    looked(stack, task, watched[4])
    acted(stack, task, watched[5], "button", "Save")
    refused(watched[7], "stale_ref", "e9 isn't in snapshot")
    assert whoami(runs_of[1]) == "in", "the run after the Take over's sign-in wasn't signed in"
    assert whoami(runs_of[2]) == "out", "the run after that was still signed in"
    assert stack.cookies(task) == [], "the site's cookie outlived the run it served: {}".format(stack.cookies(task))
    assert stack.browser_cookies(task) == [], "the browser still holds the site's cookie after the run it served"
    print("SEC-059 live: the Take over's sign-in served the next run and was gone after it; WEB-E2E-037 watched, "
          "replayed and took over a real desktop", flush=True)

    # A sign-in in the profile, and no run after it: a fork's desktop starts signed out, and leaves it as it is.
    held = hand_sign_in(stack, directory / "sign-in.json", task, login)
    fork = stack.request("/api/v1/workspaces/{}/fork".format(workspace), {"name": "Desktop fork"})["id"]
    deadline = time.monotonic() + 120
    while stack.request("/api/v1/workspaces/" + fork).get("state") != "ready":
        assert time.monotonic() < deadline, "the fork didn't become ready"
        time.sleep(1)
    forked = stack.task("A fork's desktop", workspace=fork, files=False)["id"]
    assert whoami(stack.run(forked, "whoami")) == "out", "a fork's desktop was signed in"
    assert stack.cookies(forked) == [], "the site's cookie is in the fork's desktop"
    assert stack.browser_cookies(task) == held, "the fork's run touched the first desktop's browser"
    print("SEC-059 live: with a sign-in held, a fork's desktop started signed out and left it as it was", flush=True)

    # Sign out everywhere from the panel; the run that follows the sign-in is then signed out.
    found = stack.browser("sign-out", directory / "sign-out.json", TQA_SCREEN_TASK=task)
    assert found.get("checked") == ["signed-out"], found
    assert stack.cookies(task) == [], "Sign out everywhere left the cookie: {}".format(stack.cookies(task))
    assert stack.browser_cookies(task) == [], "Sign out everywhere left the cookie in the browser"
    assert whoami(stack.run(task, "whoami")) == "out", "the run after Sign out everywhere was signed in"
    print("SEC-059 live: Sign out everywhere emptied the profile, said so on the panel, and the next run was signed "
          "out", flush=True)

    # Signed in again: a share holder's and another task's desktops on the same workspace start signed out. Each takes
    # the workspace from the idle desktop holding the sign-in (the workspace lease hands it over), whose profile goes
    # with its sandbox.
    hand_sign_in(stack, directory / "sign-in-again.json", task, login)
    client = stack.teammate()
    stack.request("/api/v1/workspaces/{}/shares".format(workspace), {"email": TEAMMATE_EMAIL, "access": "write"},
                  method="PUT")
    shared = stack.as_person(client, lambda: stack.task("A share holder's desktop", workspace=workspace,
                                                        files=False)["id"])
    assert stack.as_person(client, lambda: whoami(stack.run(shared, "whoami"))) == "out", "a share holder was signed in"
    assert stack.cookies(shared) == [], "the site's cookie is in the share holder's desktop"
    other = stack.task("Another task on the workspace", workspace=workspace, files=False)["id"]
    assert whoami(stack.run(other, "whoami")) == "out", "another task on the workspace was signed in"
    assert stack.cookies(other) == [], "the site's cookie is in another task's desktop"
    print("SEC-059 live: with a sign-in held, a share holder's and another task's desktops on the workspace started "
          "signed out", flush=True)


def hand_sign_in(stack, result, task, login):
    """The browser's sign-in by hand in a Take over, with no run after it: at the hand-back the site's cookie is in the
    runtime's browser, and no file outside its profile's tmpfs holds it. The disk isn't waited for: the next run uses
    this live browser, and Chromium writes its cookies only in batches (16e's SEC-059 failure: none on disk in 60 s
    under load, with the cookie in memory)."""
    # 16r's sec_059_a: the cookie was missing, and nothing said whether the browser was there, the address reached the
    # site, or the cookie went: each is told apart now.
    before = stack.logins()
    stack.browser("sign-in", result, TQA_SCREEN_TASK=task, TQA_SCREEN_LOGIN=login)
    assert stack.browser_up(task), "no browser on the desktop after the Take over's hand-back"
    assert stack.logins() > before, ("the site's login page was never requested: what was typed under control went "
                                     "nowhere")
    held = stack.browser_cookies(task)
    assert held, "the sign-in's cookie isn't in the runtime's browser"
    written = stack.cookies(task)
    assert all(path.startswith("/run/tqa-browser/") for path in written), \
        "the sign-in is in the runtime's profile and nowhere else: {}".format(written)
    return held


# --- SBX-C-022: the display stack outlives the runtime's reaps -------------------------------------------------------

# Each part of the display stack desktop-init keeps up, by the program its command line starts with (SBX-C-020's).
PARTS = ("Xvfb", "dbus-daemon", "/usr/libexec/at-spi-bus-launcher", "openbox", "x11vnc")
RUNTIME = "/usr/local/bin/tqa-runtime"
# Past the reap after the run's shell command and the one after the run itself.
REAP_SECONDS = 5
# An application opened as the agent would, out of the shell's session and slow to start (it reaches the display only
# 3 s in, after its call has ended and been reaped), a plain background `sleep`, and the runtime's browser, each
# recorded. The call returns at once, as a model's would: no waiting for the window.
REAP_OPEN = ("cd /tmp && DISPLAY=:0 XDG_RUNTIME_DIR=/tmp/tqa-desktop "
             "DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/tqa-desktop/bus "
             "setsid sh -c 'sleep 3; exec mousepad' </dev/null >/dev/null 2>&1 & echo $! > /tmp/reap-mousepad; "
             "sleep 300 </dev/null >/dev/null 2>&1 & echo $! > /tmp/reap-sleep; "
             "pgrep -o -f '[/]usr/lib/chromium/chromium .*--user-data-dir' > /tmp/reap-chromium; echo opened")
# Past the reaper's grace for young orphans (10 s, ADR-0039 section 10), so its later decision falls while this call
# runs: which of the three still runs.
REAP_CHECK = ("sleep 12; for name in mousepad sleep chromium; do pid=$(cat /tmp/reap-$name); "
              "if [ -n \"$pid\" ] && [ -d /proc/$pid ]; then echo \"$name alive\"; else echo \"$name gone\"; fi; done")
# Every process in the sandbox as `<pid> <parent pid> <command line>`, from /proc.
PROCESSES = r"""for d in /proc/[0-9]*; do
  p=${d#/proc/}; pp=$(awk '/^PPid:/ {print $2}' "$d/status" 2>/dev/null)
  c=$(tr '\0' ' ' < "$d/cmdline" 2>/dev/null); [ -n "$c" ] && [ -n "$pp" ] && echo "$p $pp $c"
done; true"""


def process_table(text):
    """`{pid: (parent, command line)}` from PROCESSES's output."""
    table = {}
    for line in text.splitlines():
        pid, parent, command = (line.split(" ", 2) + ["", ""])[:3]
        if pid.isdigit() and parent.isdigit():
            table[int(pid)] = (int(parent), command.strip())
    return table


def outlived(table):
    """What's wrong with the display stack in `table`: a part not running, or one the runtime is an ancestor of (its
    reaper kills every direct child it didn't start, and what they leave behind after them). Empty when all is well."""
    runtimes = {pid for pid, (_, command) in table.items() if command.startswith(RUNTIME)}
    problems = [] if runtimes else ["the runtime isn't running"]
    for part in PARTS:
        found = [pid for pid, (_, command) in table.items() if command.startswith(part)]
        if not found:
            problems.append("{} isn't running".format(part))
        for pid in found:
            seen, parent = set(), table[pid][0]
            while parent in table and parent not in seen:
                if parent in runtimes:
                    problems.append("{} ({}) descends from the runtime ({})".format(part, pid, parent))
                    break
                seen.add(parent)
                parent = table[parent][0]
    return problems


def reap(stack):
    """SBX-C-022: a run's shell command, then past the reaps that follow it and the run: every part of the display
    stack is still running and none of them is the runtime's, and the screen still reads."""
    conversation = stack.task("Desktop reap")["id"]
    steps = calls(stack.run(conversation, "reap"))
    time.sleep(REAP_SECONDS)
    table = process_table(stack.in_sandbox(conversation, ["sh", "-c", PROCESSES]))
    assert not outlived(table), "after a run, the display stack didn't outlive the runtime's reaps: {}; {}".format(
        outlived(table), sorted(command for _, command in table.values()))
    assert len(steps) == 3 and all(step["result"]["status"] == "ok" for step in steps), \
        [step["result"]["status"] for step in steps]
    survived = survivors(steps[1]["result"]["content"])
    assert survived == {"mousepad": True, "sleep": False, "chromium": True}, \
        "after the opening call's reap: {} ({})".format(survived, steps[1]["result"]["content"][:300])
    print("SBX-C-022 live: after a command and its run, Xvfb, both buses, openbox and x11vnc still run, none of them "
          "the runtime's, and the screen still reads", flush=True)
    print("SBX-C-022 live: the reap after a shell call left the application the agent opened and the runtime's browser, "
          "and took the plain background process", flush=True)


def survivors(text):
    """`<name> alive|gone` lines, as REAP_CHECK prints them."""
    found = {}
    for line in text.splitlines():
        words = line.split()
        if len(words) == 2 and words[1] in ("alive", "gone"):
            found[words[0]] = words[1] == "alive"
    return found


def interrupted(_number, _frame):
    raise InterruptedError("The desktop stack case was interrupted or passed its deadline")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("case", choices=CASES)
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    if not os.environ.get("TQA_DESKTOP_TEST_IMAGE"):
        raise SystemExit("These cases need TQA_DESKTOP_TEST_IMAGE, the Desktop image under test")
    if args.case not in ("tool", "reap") and not os.environ.get("TQA_WEB_IMAGE"):
        raise SystemExit("The {} case needs TQA_WEB_IMAGE, the web image under test".format(args.case))
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-desktop-live-")
    directory = compose.installation(temporary)
    results = Path(tempfile.mkdtemp(prefix="tqa-desktop-live-results-"))
    stack, failure = LiveStack(directory, env), None
    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(number, interrupted)
    signal.alarm(DEADLINE_SECONDS[args.case])
    try:
        stack.prepare("live-" + args.case)
        stack.start()
        if args.case == "tool":
            tool(stack)
        elif args.case == "record":
            record(stack, results)
        elif args.case == "reap":
            reap(stack)
        else:
            sign_in(stack, results)
        assert stack.order == [], "scripted runs left over: {}".format(stack.order)
    except BaseException as error:
        failure = error
        print("Desktop stack case {} failed: {}".format(args.case, stack.redact(str(error))), flush=True)
        stack.diagnostics()
    finally:
        signal.alarm(0)
        for stream in stack.streams:
            stream.response.close()
        keep_logs(env, failure, stack.configuration)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        try:
            compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
        finally:
            shutil.rmtree(results, ignore_errors=True)


if __name__ == "__main__":
    main()
