"""GW-I-326 and GW-I-327: the administrator's Sandboxes page on a real Docker engine with the testkit's fake model
(the Sandboxes page design note; GW-U-216 to GW-U-218 on the fixture's engine are the same rules without one).

Each case starts its own stack (robustness.Stack: its own name, pool, port and installation), with a fake-model
scenario that answers each task by a phrase of its first message (prompt selection), so tasks of two people run their
own scripts side by side. The cases read only the public API, the engine and the stack's own database.

Usage: TQA_DOCKER_TESTS=1 python3 deploy/tests/sandboxes_stack.py listing|stop
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

import compose
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


CASES = {"listing": listing_case, "stop": stop_case}


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
        stack.prepare(args.case, scenario=SCENARIO, **METRICS)
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
