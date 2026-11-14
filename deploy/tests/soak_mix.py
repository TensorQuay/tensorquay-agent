"""SOAK-001's mix of agent teams and Desktop tasks (ADR-0034; ADR-0039 §10), off unless asked: with neither
`--teams-every` nor `--desktops-every`, the scenario, the gateway's file, the room for sandboxes and the result are
today's, byte for byte.

Two lanes run beside the scripted slots, each with one conversation at a time, outside the slots' fault plan; the
stack-wide faults reach them as they reach everything:
- the team lane: a lead whose first message names TEAM_PHRASE starts two helpers on their own copies, asks them, reads
  their reports and calls `done`; the team must end `done` with both helpers, a report from each and one end note.
  Then the lead is deleted with its helpers;
- the Desktop lane: a task on the Desktop template whose first message names DESK_PHRASE types an address, waits and
  observes, so the desktop acts and keeps screenshots; its run must complete with pictures kept. Then it is deleted.
After each delete the lane waits for what that conversation held to go (soak_mix_audit), then the next starts once
its period has passed. A lane never blocks the soak's loop: it polls the API every POLL_SECONDS."""
import dataclasses
import subprocess
import threading
import time
import tomllib
import urllib.error

from soak_audit import require
from soak_mix_audit import (COUNTS, desktop_leftovers, end_problems, sample_problems,
                            team_leftovers)
import soak_stack

TEAM_PHRASE = "SOAK-TEAM"
DESK_PHRASE = "SOAK-DESK"
HELPERS = ("soak-analyst", "soak-fixer")
# The lanes' own scripted model and its route; the fake model's cap on a scenario's replies.
MIX_SERVICE, MIX_ROUTE, MAX_RESPONSES = "fake-model-mix", "mix", 128
# A lane's first conversation starts this long after the chaos starts, once the slots are under way.
FIRST_START_SECONDS = 60
POLL_SECONDS = 5
# Why the gateway drops a message whose environment never started (GW-I-322): sent again by the lane, as a person does.
START_FAILED_REASONS = ("sandbox_failed", "runtime_start_failed", "decision_engine_unavailable")
# How long a lane conversation may take to end, and then to give back what it held after its delete (the purge
# sweeps every minute and removes a deleted task's files and rows within minutes).
CONVERSATION_SECONDS = 900
SETTLE_SECONDS = 300
# The resources sampled at most this often during the run (each sample lists the storage root from a container).
SAMPLE_SECONDS = 300
# A team adds its lead and two helpers; a Desktop task one sandbox.
TEAM_SANDBOXES, DESKTOP_SANDBOXES = 1 + len(HELPERS), 1
USAGE = {"prompt_tokens": 40, "completion_tokens": 20}


@dataclasses.dataclass(frozen=True)
class MixOptions:
    """Minutes between the lanes' conversation starts; None leaves the lane out."""
    teams_every: int = None
    desktops_every: int = None

    @property
    def teams(self):
        return self.teams_every is not None

    @property
    def desktops(self):
        return self.desktops_every is not None

    @property
    def on(self):
        return self.teams or self.desktops


def check_mix(options, backend, model):
    for name, value in (("--teams-every", options.teams_every), ("--desktops-every", options.desktops_every)):
        require(value is None or (type(value) is int and 2 <= value <= 60),
                "{} takes whole minutes from 2 to 60".format(name))
    if options.on:
        require(model == "fake", "agent teams and Desktop tasks in the mix are scripted: they need the fake model")
        require(backend == "docker", "agent teams and Desktop tasks in the mix run on Docker only")


def call(name, arguments, split_into=1):
    return {"type": "tool_call", "name": name, "arguments": arguments, "split_into": split_into}


def reply(when, event):
    return {"when": when, "events": [event], "usage": dict(USAGE)}


def brief(name):
    return {"objective": "Write result.md with one line naming you, {}, and report it.".format(name),
            "output": "result.md, reported to the lead.", "sources": None,
            "boundaries": "Don't edit the lead's files."}


def team_replies():
    """The lead's script, then each helper's. A reply is chosen by turn among those naming the task's first message's
    phrase, and the last one repeats: as the agents suite's lead, the lead ends its turn with text after `done` (a
    repeated `done` kept its run going)."""
    read = call("read_reports", {"wait": 30, "limit": None}, 2)
    lead = [call("start_member", {"role": name, "workspace": "own_copy", "profile": None, "brief": brief(name)}, 2)
            for name in HELPERS]
    lead += [call("ask_all", {"text": "Anything blocking?"}), read, read, read,
             call("done", {"summary": "Both helpers reported."}, 2),
             {"type": "text", "text": "Done: both helpers reported."}]
    replies = [reply(TEAM_PHRASE, event) for event in lead]
    for name in HELPERS:
        own = "You are " + name
        write = "printf '%s\\n' {} > result.md && cat result.md".format(name)
        replies += [reply(own, call("shell", {"command": write})),
                    reply(own, call("send_to_lead", {"type": "report", "text": "Wrote result.md.",
                                                     "files": ["result.md"]}, 2)),
                    reply(own, {"type": "text", "text": "Reported to the lead."})]
    return replies


def computer(action, **arguments):
    fields = {"action": action, "ref": None, "text": None, "keys": None, "ms": None, "direction": None, "amount": None}
    fields.update(arguments)
    return call("computer", fields)


def desktop_replies():
    """The Desktop stack suite's look at a page (its DESK-A): keys, typing, a wait and `observe` only, since a click
    needs a ref from a snapshot a script can't know. Each action keeps a before and an after screenshot."""
    page = "<!doctype html><html><head><title>Soak page</title></head><body><h1>Soak page</h1></body></html>"
    events = [call("shell", {"command": "printf '%s\\n' '{}' > /workspace/page.html".format(page)}),
              computer("key", keys="ctrl+l"), computer("type", text="file:///workspace/page.html"),
              computer("key", keys="Return"), computer("wait", ms=3000), computer("observe"),
              {"type": "text", "text": "Looked at the page."}]
    return [reply(DESK_PHRASE, event) for event in events]


def lane_scenario(teams, desktops):
    """The lanes' own scripted model's scenario: prompt selection, each lane's replies by its phrase. The slots' model
    and script stay today's: the fake model takes at most MAX_RESPONSES replies, and the slots' script holds 127."""
    if not teams and not desktops:
        return None
    replies = (team_replies() if teams else []) + (desktop_replies() if desktops else [])
    require(len(replies) <= MAX_RESPONSES, "the lanes' script passes the fake model's reply cap")
    return {"model": "fixture", "select": "prompt", "responses": replies}


def models_text(text):
    """The soak's models file with a MIX_ROUTE route: the slots' route as written (its protocol, key and limits), on the
    lanes' model. A helper takes its lead's route (no `member_route` is set), so a team stays on it."""
    marker = "[[routes]]\nname='local'\n"
    require(text.count("[[routes]]") == 1 and marker in text, "the soak's models file has one route, `local`")
    route = text[text.index(marker):]
    require("http://fake-model:8080/v1" in route, "the soak's route isn't the scripted model's")
    mix = route.replace("name='local'", "name='{}'".format(MIX_ROUTE)).replace(
        "http://fake-model:8080/v1", "http://{}:8080/v1".format(MIX_SERVICE))
    return text + ("" if text.endswith("\n") else "\n") + mix


def lane_service(source, image, demo_file):
    """The lanes' model: the demo's fake model as it is (user, read-only root, caps, limits, health check), with its own
    scenario mounted read-only."""
    return {"extends": {"file": demo_file, "service": "fake-model"}, "image": image,
            "command": ["--scenario", "/scenarios/mix.json"],
            "volumes": [{"type": "bind", "source": source, "target": "/scenarios/mix.json", "read_only": True}]}


def gateway_text(text, teams):
    """Agent teams on (off by default, ADR-0034's install switch), as the agents suite turns them on."""
    return text + "\n[agent_teams]\nenabled = true\n" if teams else text


def listeners(teams):
    """The gateway's database connections outside its pool: the team listener, only with agent teams on
    (teams/changed.rs; SOAK-001's D1 and D4)."""
    return 1 if teams else 0


def max_live(slots, teams, desktops):
    """Today's room for the slots, plus a team's and a Desktop's sandboxes when they run (20 at 8 slots, within the
    network pool's 32 subnets)."""
    live = soak_stack.max_live(slots) + (TEAM_SANDBOXES if teams else 0) + (DESKTOP_SANDBOXES if desktops else 0)
    if teams or desktops:
        subnets = 2 ** (29 - pool_prefix(MixOptions(teams_every=10)))
        require(2 * live <= subnets, "the mix's pool of {} subnets holds max_live {} at most".format(subnets,
                                                                                              subnets // 2))
    return live


def pool_prefix(options):
    """The sandbox network pool's size: today's /24 (32 subnets of /29) without the mix; a /23 (64) with it, since
    the gateway wants at least twice max_live subnets and the mix's max_live is 20 at 8 slots."""
    return 23 if options.on else 24


def network_pool(config_text):
    """The pool the gateway's file names, for the evidence."""
    pool = tomllib.loads(config_text)["sandbox"]["network_pool"]
    return {"cidr": pool["cidr"], "prefix": pool["prefix"]}


def desktop_image(env, desktops, inspect):
    """The Desktop image under test, by ID, with its repository digests; None without the Desktop lane."""
    if not desktops:
        return None
    reference = env.get("TQA_DESKTOP_TEST_IMAGE")
    require(bool(reference), "the Desktop lane needs TQA_DESKTOP_TEST_IMAGE: the Desktop image under test")
    record = inspect(reference)
    return {"id": record["Id"], "repo_digests": record.get("RepoDigests") or [], "reference": reference}


def check_pinned(config_text, image_id):
    """The gateway's file pins the Desktop built-in to exactly that image ID."""
    pins = tomllib.loads(config_text).get("runtime", {}).get("builtin_images", {})
    require(pins.get("desktop") == image_id, "the gateway's file pins no Desktop built-in to the image under test")


def max_sessions(limits):
    value = next((item["value"] for item in limits.get("items", []) if item.get("key") == "desktop.max_sessions"),
                 None)
    require(isinstance(value, int), "the admin limits name no desktop.max_sessions")
    return value


def transient(error):
    """A failure the lane meets during a stack-wide fault (a restart, a database outage): tried again at its next
    poll, within the conversation's bound. A refusal (4xx) is not."""
    if isinstance(error, urllib.error.HTTPError):
        return error.code >= 500
    return isinstance(error, (urllib.error.URLError, OSError, TimeoutError, subprocess.SubprocessError))


# A capture dropped while the disk was full may be refused for this long after the free (as the slots' clients allow).
DISK_RECOVERY_SECONDS = 180


def disk_full_window(opened, ended, windows, margin=DISK_RECOVERY_SECONDS):
    """The soak's disk-full window `[start, end]` (end inf while the disk is still full) that a Desktop task running from
    `opened` to `ended` (wall-clock seconds) met, within the store's recovery after the free; None when none did."""
    return next((window for window in windows if window[0] <= ended and opened <= window[1] + margin), None)


def dropped_capture(events):
    """Whether the product recorded a capture it could not keep: a `tool.result` whose evidence reads a phase false
    (CORE-U-083; the screen store drops what it cannot keep and the action goes on)."""
    return any(event_kind(item) == "tool.result"
               and any(kept is False for kept in ((item.get("event") or {}).get("evidence") or {}).values())
               for item in events)


def event_kind(item):
    event = item.get("event") or {}
    return event.get("type", item.get("type"))


class Lane:
    """One conversation at a time: waiting for its start, running until it ends and is judged, then settling after its
    delete. Times are the soak's seconds since the chaos started."""
    kind = None

    def __init__(self, api, every_seconds, start, log, disk_windows=lambda: (), wall=time.time):
        self.api, self.every, self.log = api, every_seconds, log
        self.state, self.next_start, self.next_poll = "waiting", start + FIRST_START_SECONDS, start
        self.count, self.records, self.retries = 0, [], 0
        self.conversation, self.started, self.deadline, self.sent = None, None, None, False
        self.resent = set()
        # The soak's disk-full windows (wall-clock) and the clock that places a conversation among them.
        self.disk_windows, self.wall, self.opened_at = disk_windows, wall, None

    def idle(self):
        return self.state == "waiting"

    def path(self):
        return "/api/v1/conversations/" + self.conversation

    def step(self, now, paused, draining):
        if self.state == "waiting":
            if draining or paused or now < self.next_start:
                return
        elif paused or now < self.next_poll:
            self.check_deadline(now)
            return
        self.next_poll = now + POLL_SECONDS
        try:
            self.advance(now)
        except Exception as error:
            if not transient(error):
                raise
            self.retries += 1
            self.log("mix_retry", lane=self.kind, conversation=self.conversation, error=type(error).__name__)
        if self.state != "waiting":
            self.check_deadline(now)

    def check_deadline(self, now):
        """Past its bound the lane fails; only then is the message built (a settling lane's names what is left, which
        takes a probe)."""
        if now >= self.deadline:
            require(False, self.overdue())

    def overdue(self):
        if self.state == "running":
            return "{} conversation {} did not end within {} s".format(self.kind, self.conversation,
                                                                       CONVERSATION_SECONDS)
        return "{} conversation {} left behind after its delete: {}".format(
            self.kind, self.conversation, "; ".join(self.leftovers(self.api.mix_probe(self.helpers()))))

    def advance(self, now):
        if self.state == "waiting":
            self.conversation = self.api.request("/api/v1/conversations", self.create())["id"]
            self.started, self.deadline, self.state, self.sent = now, now + CONVERSATION_SECONDS, "running", False
            self.resent = set()
            self.opened_at = self.wall()
            self.log("mix_opened", lane=self.kind, conversation=self.conversation, number=self.count)
        if self.state == "running" and not self.sent:
            # Sent on the next poll again if a stack-wide fault met it.
            self.api.request(self.path() + "/messages", {"text": self.message(), "attachments": []})
            self.sent = True
        elif self.state == "running":
            if self.resend_dropped():
                return
            record = self.ended()
            if record is not None:
                record.update(conversation=self.conversation, number=self.count, seconds=round(now - self.started, 1))
                self.api.delete(self.delete_path())
                self.record, self.state, self.deadline = record, "settling", now + SETTLE_SECONDS
                self.log("mix_ended", lane=self.kind, record=record)
        else:
            problems = self.leftovers(self.api.mix_probe(self.helpers()))
            if not problems:
                self.record["settle_seconds"] = round(now - self.started - self.record["seconds"], 1)
                self.records.append(self.record)
                self.log("mix_settled", lane=self.kind, conversation=self.conversation)
                self.state, self.count = "waiting", self.count + 1
                self.next_start = max(self.started + self.every, now)

    def resend_dropped(self):
        """A message the gateway dropped because its environment could not start (GW-I-322, listed with the start's
        reason) is sent again, once per dropped record, as a person does with "Not sent, because the environment
        couldn't start"; the record itself stays for the idempotency retention. A message a failed run dropped is that
        run's verdict (`ended` judges it), and a slow start is waited for within the conversation's bound: no timer and
        no reading of the sandbox's state sends anything (the 24 h run of 14 Nov 2026 ended on a first start that
        failed under an engine stall, with nothing sent again)."""
        for item in self.api.request(self.path() + "/messages/queued")["items"]:
            if (item.get("state") == "dropped" and item.get("reason") in START_FAILED_REASONS
                    and item["id"] not in self.resent):
                self.resent.add(item["id"])
                self.api.request(self.path() + "/messages", {"text": self.message(), "attachments": []})
                self.retries += 1
                self.log("mix_retry", lane=self.kind, conversation=self.conversation, error=item["reason"])
                return True
        return False

    def helpers(self):
        return set()

    def delete_path(self):
        return self.path()


class TeamLane(Lane):
    kind = "team"

    def __init__(self, api, every_seconds, start, log):
        super().__init__(api, every_seconds, start, log)
        self.seen = {}  # every helper conversation seen, by the lead it served

    def create(self):
        return {"title": "Soak team {}".format(self.count), "model_route": MIX_ROUTE}

    def message(self):
        return "{} {}: two helpers each write and report a file, then end the team.".format(TEAM_PHRASE, self.count)

    def helpers(self):
        return {conversation for members in self.seen.values() for conversation in members}

    def delete_path(self):
        return self.path() + "?with_helpers=true"

    def ended(self):
        try:
            team = self.api.request(self.path() + "/team")
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return None
            raise
        self.seen[self.conversation] = {helper["conversation"] for helper in team.get("helpers", [])
                                        if helper.get("conversation")}
        if team["state"] != "ended":
            return None
        items = self.api.request(self.path() + "/team/messages?after=0&limit=200")["items"]
        names = sorted(helper["name"] for helper in team["helpers"])
        reports = [item for item in items if item["type"] == "report"]
        ended = sum(1 for item in items if item["type"] == "ended")
        where = "team of {}".format(self.conversation)
        require(team["stop_reason"] == "done", "{} ended {}, not done".format(where, team["stop_reason"]))
        require(names == sorted(HELPERS), "{} had helpers {}".format(where, names))
        missing = sorted(set(HELPERS) - {item["from"].get("name") for item in reports})
        require(not missing, "{} has no report from {}".format(where, ", ".join(missing)))
        require(ended == 1, "{} has {} end notes".format(where, ended))
        return {"team": team["id"], "stop_reason": team["stop_reason"], "helpers": names, "reports": len(reports),
                "messages": len(items), "helper_conversations": sorted(self.seen[self.conversation])}

    def leftovers(self, probe):
        return team_leftovers(probe["counts"], probe["sandboxes"],
                              {self.conversation} | self.seen.get(self.conversation, set()))


class DesktopLane(Lane):
    kind = "desktop"

    def create(self):
        return {"title": "Soak desktop {}".format(self.count), "model_route": MIX_ROUTE, "template": "desktop"}

    def message(self):
        return "{} {}: look at a page.".format(DESK_PHRASE, self.count)

    def ended(self):
        events = [item for page in self.api.stored(self.conversation) for item in page]
        kinds = [event_kind(item) for item in events]
        require("run.failed" not in kinds, "Desktop task {} failed".format(self.conversation))
        if "run.completed" not in kinds:
            return None
        pictures = self.api.mix_probe(set())["screens"].get(self.conversation, 0)
        record = {"captures": kinds.count("screen.capture"), "pictures": pictures,
                  "tool_calls": kinds.count("tool.start")}
        if pictures < 1:
            # Inside a disk-full fault the screen store drops what it cannot keep and the run goes on, by design
            # (CORE-U-083): judged, never avoided, and only on the product's own record of a dropped capture (the 24 h
            # run of 13 Nov 2026 ended on a task opened 3 ms into such a window).
            window = disk_full_window(self.opened_at, self.wall(), self.disk_windows())
            require(window is not None, "Desktop task {} kept no picture".format(self.conversation))
            require(dropped_capture(events), "Desktop task {} kept no picture in a full-disk window, and its events "
                    "said nothing of a dropped capture".format(self.conversation))
            record.update(pictures_not_kept="disk full",
                          disk_full_window=[window[0], None if window[1] == float("inf") else window[1]])
        return record

    def leftovers(self, probe):
        return desktop_leftovers(probe["counts"], probe["sandboxes"], probe["screens"], self.conversation)


class MixRun:
    """The lanes a run asked for, the audit's samples and its end check."""

    def __init__(self, options, api, log, clock=time.monotonic, sleep=time.sleep, disk_windows=lambda: ()):
        self.options, self.api, self.log, self.clock, self.sleep = options, api, log, clock, sleep
        self.disk_windows = disk_windows
        self.lanes, self.samples, self.next_sample, self.missed_samples = [], 0, 0.0, 0
        self.start_counts = self.high_water = self.sessions = None
        self.thread, self.failure, self.stopping = None, None, threading.Event()

    def start(self, now, limits):
        if not self.options.on:
            return
        self.sessions = max_sessions(limits)
        self.start_counts = self.probe()["counts"]
        problems = end_problems(self.start_counts)
        require(not problems, "the mix's counts before the first lane conversation: " + "; ".join(problems))
        self.high_water = dict(self.start_counts)
        if self.options.teams:
            self.lanes.append(TeamLane(self.api, self.options.teams_every * 60, now, self.log))
        if self.options.desktops:
            self.lanes.append(DesktopLane(self.api, self.options.desktops_every * 60, now, self.log,
                                          disk_windows=self.disk_windows))
        self.log("mix_started", counts=self.start_counts, max_sessions=self.sessions)

    def probe(self):
        helpers = {conversation for lane in self.lanes if isinstance(lane, TeamLane) for conversation in lane.helpers()}
        return self.api.mix_probe(helpers)

    def step(self, now, paused, draining):
        for lane in self.lanes:
            lane.step(now, paused, draining)

    def idle(self):
        return all(lane.idle() for lane in self.lanes)

    def sample(self, now=None):
        """The counts now, judged against a sample's limits; at most every SAMPLE_SECONDS when given the time."""
        if not self.options.on or (now is not None and now < self.next_sample):
            return
        self.next_sample = (now or 0.0) + SAMPLE_SECONDS
        try:
            counts = self.probe()["counts"]
        except Exception as error:
            if not transient(error):
                raise
            self.missed_samples += 1
            self.log("mix_resources_missed", reason=type(error).__name__, missed=self.missed_samples)
            return
        self.samples += 1
        self.high_water = {name: max(self.high_water[name], counts[name]) for name in COUNTS}
        self.log("mix_resources", counts=counts)
        problems = sample_problems(counts, self.sessions)
        require(not problems, "mix sample: " + "; ".join(problems))

    def begin(self, elapsed, paused, draining, period=1.0):
        """The lanes and the samples on their own thread, so their engine and database calls never hold the soak's
        loop; `elapsed`, `paused` and `draining` read the soak's state. Nothing is sampled while sending is paused (the
        database may be down)."""
        if not self.options.on:
            return
        self.thread = threading.Thread(target=self.run, args=(elapsed, paused, draining, period), name="soak-mix",
                                       daemon=True)
        self.thread.start()

    def run(self, elapsed, paused, draining, period):
        try:
            while not self.stopping.is_set():
                now, hold = elapsed(), paused()
                self.step(now, hold, draining())
                if not hold:
                    self.sample(now)
                self.stopping.wait(period)
        except BaseException as error:
            self.failure = error

    def check(self):
        """Raises, on the soak's loop, what the lanes' thread met."""
        if self.failure is not None:
            raise self.failure

    def end(self, raise_failure=True):
        """Stops the lanes' thread (bounded by an engine command's own bound) and raises what it met."""
        self.stopping.set()
        if self.thread is not None:
            self.thread.join(timeout=200)
            require(not self.thread.is_alive(), "the mix's thread did not stop")
        if raise_failure:
            self.check()

    def finish(self):
        """After the drain: every count back to its floor within SETTLE_SECONDS; the lanes' records."""
        if not self.options.on:
            return None
        deadline = self.clock() + SETTLE_SECONDS
        while True:
            counts = self.probe()["counts"]
            problems = end_problems(counts)
            if not problems:
                break
            require(self.clock() < deadline, "left at the end of the mix: " + "; ".join(problems))
            self.sleep(POLL_SECONDS)
        self.log("mix_audit", counts=counts)
        summary = {"start": self.start_counts, "end": counts, "high_water": self.high_water, "samples": self.samples,
                   "missed_samples": self.missed_samples, "max_sessions": self.sessions}
        for lane in self.lanes:
            summary[lane.kind + "s"] = {"conversations": len(lane.records), "retries": lane.retries,
                                        "records": lane.records}
        for name in ("teams", "desktops"):
            summary.setdefault(name, {"conversations": 0, "retries": 0, "records": []})
        return summary
