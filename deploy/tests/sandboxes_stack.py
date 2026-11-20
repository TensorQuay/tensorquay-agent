"""GW-I-326, GW-I-327, GW-I-328 and E2E-OPS-001: the administrator's Sandboxes page on a real Docker engine with the
testkit's fake model (the Sandboxes page design note; GW-U-216 to GW-U-218 on the fixture's engine are the same rules
without one).

Each case starts its own stack (robustness.Stack: its own name, pool, port and installation), with a fake-model
scenario that answers each task by a phrase of its first message (prompt selection), so tasks of two people run their
own scripts side by side. The cases read only the public API, the engine and the stack's own database.

Usage: TQA_DOCKER_TESTS=1 python3 deploy/tests/sandboxes_stack.py listing|stop|verdict|page
"""
import argparse
import http.cookiejar
import json
import os
import secrets
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

import compose
import demo
import robustness


# The working task's call: a CPU loop of 25 s (under the shell tool's 30 s timeout), so the sampler's passes during it
# measure a busy sandbox beside two idle ones.
BUSY_SECONDS = 25
# The sampler takes every running sandbox every 10 s (manager/sampler.rs SAMPLE_EVERY); a pass is seen as a new
# `totals.usage.measured_at`. Three passes and the start of a sandbox fit well inside this.
PASSES_WITHIN = 90
# GW-I-327: the container goes within 15 s of the stop.
GONE_WITHIN = 15
MEMBER = "sandboxes-member@example.invalid"


def reply(text):
    return {"events": [{"type": "text", "text": text}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}}


def shell(command):
    return {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}}


def scripted(phrase, replies):
    return [dict(item, when=phrase) for item in replies]


SCENARIO = {"model": "fixture", "select": "prompt", "responses": (
    scripted("Busy for a while", [
        shell("timeout {} sh -c 'while :; do :; done'; echo busy-done".format(BUSY_SECONDS)),
        reply("The busy call ended.")])
    # E2E-OPS-001: two calls in a row, so the page is read while it works and every row is measured.
    + scripted("Busy twice", [
        shell("timeout {} sh -c 'while :; do :; done'; echo busy-done".format(BUSY_SECONDS)),
        shell("timeout {} sh -c 'while :; do :; done'; echo busy-done".format(BUSY_SECONDS)),
        reply("Both busy calls ended.")])
    + scripted("Quiet task", [reply("Nothing to do.")])
    + scripted("Write the note", [
        shell("printf kept > note.txt && echo written"),
        reply("The note is written."),
        shell("cat note.txt"),
        reply("The note reads kept.")]))}


class Person:
    """One signed-in person's own cookies against the stack."""

    def __init__(self, stack):
        self.stack = stack
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, path, body=None, method=None):
        headers = {"Origin": self.stack.origin, "Content-Type": "application/json"}
        if body is not None:
            headers["Idempotency-Key"] = secrets.token_hex(16)
        request = urllib.request.Request(self.stack.origin + path, headers=headers, method=method,
                                         data=None if body is None else json.dumps(body).encode())
        try:
            with self.client.open(request, timeout=15) as response:
                data = response.read(1048577)
                status = response.status
        except urllib.error.HTTPError as error:
            data, status = error.read(1048577), error.code
        assert len(data) <= 1048576, "Response exceeded fixture cap"
        return status, (json.loads(data) if data else None)

    def ok(self, path, body=None, method=None, expect=(200, 201, 202)):
        status, answer = self.request(path, body, method)
        assert status in expect, "{} {} answered {}: {}".format(method or ("POST" if body is not None else "GET"),
                                                                 path, status, answer)
        return answer

    def task(self, title, text):
        conversation = self.ok("/api/v1/conversations", {"title": title, "model_route": "local"})["id"]
        self.ok("/api/v1/conversations/{}/messages".format(conversation), {"text": text, "attachments": []})
        return conversation

    def events(self, conversation):
        return self.ok("/api/v1/conversations/{}/events?after=0&limit=1000".format(conversation))["items"]


def administrator(stack):
    stack.sign_in()
    admin = Person(stack)
    admin.client = stack.client
    return admin


def member(stack, admin):
    """A second person, invited and joined through the public API."""
    token = admin.ok("/api/v1/invitations", {"email": MEMBER, "role": "member"})["token"]
    person = Person(stack)
    person.ok("/api/v1/invitations/accept", {"token": token, "password": "synthetic-member-" + secrets.token_hex(12)})
    return person


def wait(condition, seconds, what, every=0.5):
    deadline = time.monotonic() + seconds
    while True:
        value = condition()
        if value:
            return value
        assert time.monotonic() < deadline, "{} within {} s".format(what, seconds)
        time.sleep(every)


def ended(person, conversation, count=1):
    """The task's `count`th run has completed."""
    kinds = [frame["event"]["type"] for frame in person.events(conversation)]
    assert "run.failed" not in kinds, "a run failed: {}".format(kinds)
    return kinds.count("run.completed") >= count


def listing(admin):
    return admin.ok("/api/v1/admin/sandboxes")


def row(answer, conversation):
    rows = [item for item in answer["items"] if item["conversation_id"] == conversation]
    return rows[0] if rows else None


def containers(stack, conversation):
    return stack.command(["docker", "ps", "-aq", "--filter", "label=tqa.conversation=" + conversation]).split()


def listing_case(stack):
    """GW-I-326: three sandboxes of two people, one working in a 25 s call and two idle; the sampler's passes."""
    admin = administrator(stack)
    other = member(stack, admin)
    quiet = [admin.task("Quiet one", "Quiet task, the first"), other.task("Quiet two", "Quiet task, the second")]
    for person, conversation in zip((admin, other), quiet):
        wait(lambda: ended(person, conversation), 180, "the quiet task's run ended")
    # The idle ones measured first (two samples each), so every later pass shows as a new `measured_at`.
    wait(lambda: all((row(listing(admin), conversation) or {}).get("usage") for conversation in quiet), PASSES_WITHIN,
         "both idle sandboxes measured")
    sent, first_sight = time.monotonic(), None
    busy = admin.task("Busy one", "Busy for a while")
    # Every sampler pass from the moment the busy task's row appears, in any state (a pass is a new
    # `totals.usage.measured_at`; the idle ones have usage by then, so there is one). The first holds no usage for the
    # busy sandbox; the rows are judged at a pass while its call works; the third pass gives its CPU rate.
    seen, working, deadline = [], None, time.monotonic() + PASSES_WITHIN
    while len(seen) < 3:
        answer = listing(admin)
        at = (answer["totals"].get("usage") or {}).get("measured_at")
        busy_row = row(answer, busy)
        if busy_row and not seen and first_sight is None:
            first_sight = time.monotonic()
            print("busy row first listed {:.1f} s after its message: {}, measured_at {}, usage {}".format(
                first_sight - sent, busy_row["state"], at, busy_row["usage"]), flush=True)
        if at and busy_row and (not seen or seen[-1][0] != at):
            seen.append((at, answer))
            if busy_row["state"] == "working":
                working = answer
            print("pass {} at {}: busy {} usage {}".format(len(seen), at, busy_row["state"], busy_row["usage"]),
                  flush=True)
        assert time.monotonic() < deadline, "three sampler passes within {} s".format(PASSES_WITHIN)
        time.sleep(0.5)
    first, third = seen[0][1], seen[2][1]
    assert row(first, busy)["usage"] is None, "the busy sandbox has usage on its first pass: {}".format(row(first, busy))
    assert working is not None, "no pass saw the busy sandbox working"
    assert third["backend"] == "docker" and working["backend"] == "docker", third["backend"]
    owners = {busy: "robustness@example.invalid", quiet[0]: "robustness@example.invalid", quiet[1]: MEMBER}
    assert [item["conversation_id"] for item in working["items"]][0] == busy, "the working one first"
    for conversation in owners:
        item = row(working, conversation)
        assert item, "every sandbox listed: {}".format(working["items"])
        assert item["owner"]["email"] == owners[conversation], item["owner"]
        busy_one = conversation == busy
        assert item["state"] == ("working" if busy_one else "idle"), item
        assert item["can_stop"] is (not busy_one) and (item["stop_refusal"] is None) is (not busy_one), item
        assert (item["stops_at"] is None) is busy_one, item
    rows = {conversation: row(third, conversation) for conversation in owners}
    for item in rows.values():
        assert item and item["usage"] and item["usage"]["memory_bytes"] > 0, "measured on the third pass: {}".format(item)
    busy_cpu = rows[busy]["usage"]["cpu_millicores"]
    idle_cpu = max(rows[conversation]["usage"]["cpu_millicores"] for conversation in quiet)
    assert busy_cpu > idle_cpu, "the working one's CPU ({}) above the idle ones' ({})".format(busy_cpu, idle_cpu)
    reserved = third["totals"]["reserved"]
    for key in ("cpu_millicores", "memory_bytes"):
        total = sum(item["size"][key] or 0 for item in rows.values())
        assert reserved[key] == total, "reserved {} {} against the sum {}".format(key, reserved[key], total)
    print("GW-I-326: three listed by owner and state; usage null then measured, working {} m against idle {} m"
          .format(busy_cpu, idle_cpu), flush=True)


# The gateway's metrics listener, inside its container; the case publishes it on the host's loopback only.
METRICS_PORT = 9464
METRICS = {"top": "metrics_listen='0.0.0.0:{}'\n".format(METRICS_PORT),
           "services": {"gateway": {"ports": ["127.0.0.1::{}".format(METRICS_PORT)]}}}


def metrics(stack):
    published = stack.command(["docker", "port", stack.env["TQA_GATEWAY_CONTAINER"], "{}/tcp".format(METRICS_PORT)])
    port = published.split()[0].rsplit(":", 1)[1]
    with urllib.request.urlopen("http://127.0.0.1:{}/metrics".format(port), timeout=10) as response:
        return response.read(4194304).decode()


def retired(stack):
    """`tqa_sandbox_retired_total{reason="administrator"}`, 0 before its first count."""
    for line in metrics(stack).splitlines():
        if line.startswith("tqa_sandbox_retired_total{") and 'reason="administrator"' in line:
            return float(line.rsplit(" ", 1)[1])
    return 0.0


def audit_rows(stack, conversation):
    query = ("SELECT area, action, item, actor_kind FROM admin_audit WHERE area = 'sandboxes' "
             "AND item = '{}'".format(conversation))
    return stack.command(stack.configuration + ["exec", "-T", "postgres", "psql", "-U", "tqa", "-d", "gateway", "-At",
                                                "-c", query]).splitlines()


def stop_case(stack):
    """GW-I-327: an administrator's stop; the task resumes on its files; a stop during the next start is refused."""
    admin = administrator(stack)
    other = member(stack, admin)
    task = other.task("Note", "Write the note")
    wait(lambda: ended(other, task), 180, "the first run ended")
    wait(lambda: (row(listing(admin), task) or {}).get("state") == "idle", 60, "the sandbox listed idle")
    before = containers(stack, task)
    assert len(before) == 1, before
    assert other.ok("/api/v1/conversations/{}".format(task))["screen_notices"] is False
    retired_before = retired(stack)
    stopped = time.monotonic()
    admin.ok("/api/v1/admin/sandboxes/{}/stop".format(task), {}, expect=(202,))
    wait(lambda: not containers(stack, task), GONE_WITHIN, "the container gone")
    print("GW-I-327: the container went {:.1f} s after the stop".format(time.monotonic() - stopped), flush=True)
    assert other.ok("/api/v1/conversations/{}".format(task))["screen_notices"] is True
    notices = other.ok("/api/v1/conversations/{}/screen/notices".format(task))["items"]
    assert [notice["kind"] for notice in notices] == ["sandbox.stopped"], notices
    assert "by" not in notices[0] and "robustness@example.invalid" not in json.dumps(notices), notices
    audited = audit_rows(stack, task)
    assert audited == ["sandboxes|stop|{}|admin".format(task)], "one audit row of the stop, by an administrator: {}".format(
        audited)
    assert retired(stack) == retired_before + 1, "the administrator's retired counter went up by one"
    # The next message starts another sandbox; a stop while it starts is refused and harms nothing.
    other.ok("/api/v1/conversations/{}/messages".format(task), {"text": "Read the note", "attachments": []})
    status, answer = admin.request("/api/v1/admin/sandboxes/{}/stop".format(task), {})
    assert (status, (answer or {}).get("error", {}).get("code")) == (409, "working"), (status, answer)
    wait(lambda: ended(other, task, 2), 180, "the second run ended")
    after = containers(stack, task)
    assert len(after) == 1 and after != before, (before, after)
    results = [frame["event"] for frame in other.events(task) if frame["event"]["type"] == "tool.result"]
    assert "kept" in json.dumps(results[-1]), results[-1]
    assert len(audit_rows(stack, task)) == 1, "the refused stop wrote an audit row"
    print("GW-I-327: resumed in a new sandbox reading its file; the notice and one audit row; the stop at the start "
          "refused 409", flush=True)


# GW-I-328: the sweep every 10 s (`[sandbox] sweep_seconds`, GW-U-233) and the shortest create budget, so a planted
# container is a stray after a minute.
SWEEP = "sweep_seconds=10\ncreate_budget_seconds=60\n"
CREATE_BUDGET = 60


def detail(admin):
    return admin.ok("/readyz?detail=1")["sandboxes"]


def gauge(text, name, kind):
    line = next((line for line in text.splitlines() if line.startswith('{}{{kind="{}"}} '.format(name, kind))), None)
    return None if line is None else int(float(line.rsplit(" ", 1)[1]))


def agree(stack, sandboxes):
    """`/readyz?detail=1` and `/metrics` say the same counts."""
    text = metrics(stack)
    for kind in ("containers", "networks"):
        assert gauge(text, "tqa_sandbox_unowned", kind) == sandboxes["unowned"][kind], (kind, sandboxes, text[-2000:])
        assert gauge(text, "tqa_sandbox_left_behind", kind) == sandboxes["left_behind"][kind], (kind, sandboxes)


def plant(stack, name, image, prefix):
    labels = ["--label", "tqa.instance=" + prefix, "--label", "tqa.conversation=" + str(uuid.uuid4()),
              "--label", "tqa.created_at={}".format(int(time.time()))]
    return stack.command(["docker", "run", "-d", "--name", name, "--network", "none", *labels, "--entrypoint", "sleep",
                          image, "3600"])


def gone(stack, kind, name):
    listed = stack.command(["docker", kind, "ls", "-aq" if kind == "container" else "-q", "--filter",
                            "name=^{}$".format(name)])
    return not listed.strip()


def next_pass(admin, after, seconds=60):
    """The readiness detail of the first sweep that ends after `after` (its `swept_at`)."""
    def newer():
        sandboxes = detail(admin)
        return sandboxes if sandboxes["swept_at"] and sandboxes["swept_at"] > after else None
    return wait(newer, seconds, "a sweep after {}".format(after), every=1)


def verdict_case(stack):
    """GW-I-328: a planted stray container and network are swept and counted, green; a container carrying this
    installation's labels that the listing can't read (its conversation label is no task id) can never be checked
    against a record, so every pass counts it as one it couldn't remove: amber at its second pass, listed with its
    remove command; removed by hand with that command, the next pass is green. (A paused container is no such case:
    the sweep stops it first, which a paused container allows, and removes it.) The readiness detail and the metrics
    agree at each step."""
    admin = administrator(stack)
    prefix, image = stack.env["TQA_SANDBOX_NETWORK"], demo.runtime_image(stack.env)
    tag = secrets.token_hex(4)
    stray, unreadable, network = "tqa-stray-" + tag, "tqa-stray-unreadable-" + tag, "tqa-stray-net-" + tag
    plant(stack, stray, image, prefix)
    stack.command(["docker", "network", "create", "--label", "tqa.instance=" + prefix, "--label",
                   "tqa.subnet=192.0.2.0/29", network])
    try:
        # Each pass is read the moment it is recorded, until the one after which the stray container is gone.
        passes, at = [], ""
        while True:
            sandboxes = next_pass(admin, at)
            at = sandboxes["swept_at"]
            agree(stack, sandboxes)
            passes.append(sandboxes)
            print("pass: unowned {} left behind {}".format(sandboxes["unowned"], sandboxes["left_behind"]), flush=True)
            if gone(stack, "container", stray):
                break
            assert len(passes) < 20, "the stray container was never swept: {}".format(passes)
        assert gone(stack, "network", network), "the planted network is gone"
        assert any(entry["unowned"]["networks"] >= 1 for entry in passes), \
            "the network counted in the pass that removed it: {}".format(passes)
        removal = passes[-1]
        assert removal["unowned"]["containers"] == 1 and removal["left_behind"]["containers"] == 0, \
            "the pass that removed the stray container counts it, green: {}".format(removal)
        print("GW-I-328: stray container and network swept and counted; green", flush=True)
        # A container the listing can't read: counted from its first pass, amber from its second.
        unreadable_id = stack.command([
            "docker", "run", "-d", "--name", unreadable, "--network", "none", "--label", "tqa.instance=" + prefix,
            # Not a task id (a space and "!": an id is letters, digits, "-" and "_"), so the listing can't read it;
            # "not-a-task" would be a valid id, a young stray the sweep skips.
            "--label", "tqa.conversation=not a task!", "--label", "tqa.created_at=1", "--entrypoint", "sleep",
            image, "3600"])
        first = next_pass(admin, at)
        agree(stack, first)
        assert first["unowned"]["containers"] == 1 and first["left_behind"]["containers"] == 0, \
            "met once: counted, green: {}".format(first)
        second = next_pass(admin, first["swept_at"])
        agree(stack, second)
        assert second["left_behind"]["containers"] == 1 and second["left_behind"]["reason"] == "invalid_labels", second
        survivors = listing(admin)["left_behind"]
        assert len(survivors) == 1, survivors
        survivor = survivors[0]
        assert survivor["kind"] == "container" and survivor["name"] == unreadable_id[:12], survivor
        assert survivor["reason"] == "invalid_labels", survivor
        assert survivor["remove"] == "docker rm -f " + unreadable_id[:12], survivor
        print("GW-I-328: the unreadable one amber at its second pass, listed with {} (age {} s)".format(
            survivor["remove"], survivor["age_seconds"]), flush=True)
        # Removed by hand with the listed command: the next pass is green and lists nothing.
        stack.command(survivor["remove"].split())
        third = next_pass(admin, second["swept_at"])
        agree(stack, third)
        assert third["left_behind"]["containers"] == 0 and third["unowned"]["containers"] == 0, third
        assert listing(admin)["left_behind"] == [], "nothing listed once it is gone"
        print("GW-I-328: removed by its listed command; the next pass is green", flush=True)
    finally:
        for name in (stray, unreadable):
            if not gone(stack, "container", name):
                stack.command(["docker", "rm", "-f", name])
        if not gone(stack, "network", network):
            stack.command(["docker", "network", "rm", network])


# E2E-OPS-001: the Sandboxes page and the leak verdict as an administrator reads them. Titles nobody may see there.
TITLES = ["Ledger-{}".format(secrets.token_hex(4)) for _ in range(3)]
PAGE = "/settings/sandboxes"


def cookies(person):
    jar = next(handler.cookiejar for handler in person.client.handlers
               if isinstance(handler, urllib.request.HTTPCookieProcessor))
    return [{"name": cookie.name, "value": cookie.value} for cookie in jar]


def browse(person, out, step, **fields):
    """One step of sandboxes_page.mjs as `person`; its JSON answer. A console error or a failed request fails it."""
    argument = json.dumps(dict(fields, origin=person.stack.origin, cookies=cookies(person), out=str(out), step=step))
    answer = json.loads(person.stack.command(["node", "deploy/tests/sandboxes_page.mjs", argument]).splitlines()[-1])
    assert not answer["problems"], "{}: {}".format(step, answer["problems"])
    return answer


def untitled(what, text):
    """Nothing of any task's title in `text` (the page and its API answer show owners, never titles)."""
    for title in TITLES:
        assert title not in text, "{} shows a task's title".format(what)


def page_case(stack):
    """E2E-OPS-001: three sandboxes of two people (one working, two idle) on the page with their owners, states and
    use; the administrator stops one idle sandbox from the page, its owner's next message runs in a new sandbox on its
    files with the notice on the task page, and the stop has its audit row; a planted container the sweep can't read
    turns Status amber and the page's Left behind lists it with its command, which removes it; both green again.
    Screenshots at 1280 and 390 px in the evidence folder; no task's title on a page or in the API's answers."""
    out = Path(stack.env.get("TQA_ROBUSTNESS_EVIDENCE_DIR") or tempfile.mkdtemp(prefix="e2e-ops-001-")) / "e2e-ops-001"
    out.mkdir(parents=True, exist_ok=True)
    admin = administrator(stack)
    other = member(stack, admin)
    owner = "robustness@example.invalid"
    quiet = admin.task(TITLES[0], "Quiet task, the first")
    note = other.task(TITLES[1], "Write the note")
    wait(lambda: ended(admin, quiet), 180, "the quiet task's run ended")
    wait(lambda: ended(other, note), 180, "the note's first run ended")
    wait(lambda: all((row(listing(admin), task) or {}).get("usage") for task in (quiet, note)), PASSES_WITHIN,
         "both idle sandboxes measured")
    busy = admin.task(TITLES[2], "Busy twice")
    # Read while the busy one works and every row is measured (its calls keep it working for twice BUSY_SECONDS).
    answer = wait(lambda: (lambda a: a if (row(a, busy) or {}).get("state") == "working"
                           and all((row(a, task) or {}).get("usage") for task in (busy, quiet, note)) else None)(listing(admin)),
                  PASSES_WITHIN, "the busy sandbox working and measured")
    read = browse(admin, out, "read", path=PAGE, name="1-three-sandboxes")
    (out / "1-listing.json").write_text(json.dumps(answer, indent=1))
    untitled("the listing's answer", json.dumps(answer))
    for width, text in read["texts"].items():
        untitled("the page at " + width, text)
        assert other_email_and_states(text, owner), "the page at {} lists the three: {}".format(width, text[:600])
        assert "Measuring" not in text, "every row measured at {}: {}".format(width, text[:600])
    print("E2E-OPS-001: three listed with owners, states and use at 1280 and 390", flush=True)
    # The administrator stops the member's idle sandbox from the page.
    before = containers(stack, note)
    stopped = browse(admin, out, "stop", email=MEMBER)
    assert "The task keeps its files and history." in stopped["asked"], stopped["asked"]
    wait(lambda: not containers(stack, note), GONE_WITHIN, "the stopped container gone")
    assert audit_rows(stack, note) == ["sandboxes|stop|{}|admin".format(note)], audit_rows(stack, note)
    wait(lambda: not row(listing(admin), note), 30, "its row gone from the listing")
    after_stop = browse(admin, out, "read", path=PAGE, name="2-after-stop")
    for width, text in after_stop["texts"].items():
        assert MEMBER not in text, "the stopped row is gone at {}: {}".format(width, text[:600])
    other.ok("/api/v1/conversations/{}/messages".format(note), {"text": "Read the note", "attachments": []})
    wait(lambda: ended(other, note, 2), 180, "the note's second run ended")
    again = containers(stack, note)
    assert len(again) == 1 and again != before, (before, again)
    results = [frame["event"] for frame in other.events(note) if frame["event"]["type"] == "tool.result"]
    assert "kept" in json.dumps(results[-1]), "the task's file is intact: {}".format(results[-1])
    task_page = browse(other, out, "read", path="/c/" + note, name="3-task-notice", widths=[1280])
    assert "An administrator stopped this task's sandbox." in task_page["texts"]["1280"], task_page["texts"]["1280"][:800]
    print("E2E-OPS-001: stopped from the page; the next message ran in a new sandbox on its file; the notice and the "
          "audit row", flush=True)
    # A container of this installation the sweep can't read: Status amber and the page's Left behind, then removed.
    prefix, image = stack.env["TQA_SANDBOX_NETWORK"], demo.runtime_image(stack.env)
    name = "tqa-stray-unreadable-" + secrets.token_hex(4)
    planted = stack.command(["docker", "run", "-d", "--name", name, "--network", "none", "--label",
                             "tqa.instance=" + prefix, "--label", "tqa.conversation=not a task!", "--label",
                             "tqa.created_at=1", "--entrypoint", "sleep", image, "3600"])
    try:
        first = next_pass(admin, detail(admin)["swept_at"] or "")
        amber = next_pass(admin, first["swept_at"])
        assert amber["left_behind"]["containers"] == 1, amber
        status = browse(admin, out, "read", path="/settings/status", name="4-status-amber")
        left = browse(admin, out, "read", path=PAGE, name="5-left-behind")
        command = "docker rm -f " + planted[:12]
        # Its reason in the page's own words (fix-18): the gateway couldn't read its labels, never "the engine wouldn't
        # remove it"; and its age from the engine's clock, never 0 s, by its second pass.
        words = "the gateway couldn't read its labels"
        for width in status["texts"]:
            assert "Left behind: 1 container ({})".format(words) in status["texts"][width], status["texts"][width][:800]
            assert command in left["texts"][width] and "Left behind" in left["texts"][width], left["texts"][width][:800]
            assert words in left["texts"][width] and "wouldn't remove" not in left["texts"][width], left["texts"][width][:800]
        survivor = listing(admin)["left_behind"][0]
        assert survivor["age_seconds"] >= 10, "its engine age by its second pass: {}".format(survivor)
        (out / "5-listing.json").write_text(json.dumps(listing(admin), indent=1))
        print("E2E-OPS-001: Status amber, and the page lists {} with its command".format(planted[:12]), flush=True)
        # The command as the page shows it, run on the engine host.
        stack.command(command.split())
        green = next_pass(admin, amber["swept_at"])
        assert green["left_behind"]["containers"] == 0, green
        status = browse(admin, out, "read", path="/settings/status", name="6-status-green")
        left = browse(admin, out, "read", path=PAGE, name="7-page-green")
        for width in status["texts"]:
            assert "Nothing left behind." in status["texts"][width], status["texts"][width][:800]
            assert "Left behind" not in left["texts"][width], left["texts"][width][:800]
        print("E2E-OPS-001: removed by the page's command; Status and the page green again", flush=True)
    finally:
        if not gone(stack, "container", name):
            stack.command(["docker", "rm", "-f", name])
    for path in out.glob("*"):
        if path.suffix in (".json",):
            untitled(path.name, path.read_text())
    print("E2E-OPS-001: evidence in " + str(out), flush=True)


def other_email_and_states(text, owner):
    """The page lists the administrator's two (as "You") and the member's one, one working and two idle."""
    return MEMBER in text and text.count("Working") >= 1 and text.count("Idle") >= 2


CASES = {"listing": listing_case, "stop": stop_case, "verdict": verdict_case, "page": page_case}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", choices=sorted(CASES))
    args = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("Docker tests require TQA_DOCKER_TESTS=1")
    env = os.environ.copy()
    env.pop("COMPOSE_PROJECT_NAME", None)
    compose.teardown_on_signals()
    compose.preflight(env)
    temporary = tempfile.TemporaryDirectory(prefix="tqa-sandboxes-")
    directory = compose.installation(temporary)
    stack, failure = robustness.Stack(directory, env), None
    try:
        stack.prepare(args.case, scenario=SCENARIO, settings=SWEEP if args.case in ("verdict", "page") else None, **METRICS)
        CASES[args.case](stack)
    except BaseException as error:
        failure = error
        print("Sandboxes contract failed: " + compose.redact(str(error), env), flush=True)
    finally:
        robustness.keep_logs(env, failure, stack.configuration)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)


if __name__ == "__main__":
    main()
