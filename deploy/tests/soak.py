"""SOAK-001…003: 2 h (SOAK-002 short) or 24 h, eight conversations with two independent clients each, seeded
reconnect chaos and gateway restarts. Over a long run each conversation slot starts a new conversation (a generation)
before its script or message cap runs out; the retiring one gets the same receipt and run checks as the end."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

import compose
import demo
from soak_audit import ReceiptLog, RunAudit, require, timestamp_seconds
from soak_chaos import restart_times, schedule
from soak_disruptions import GLOBAL, backend_kinds, disruptions
from soak_growth import verdict
import soak_real
from soak_orphans import findings as orphan_findings
from soak_scenario import RESPONSES, ScenarioProgress
from soak_stack import READY_MARGIN_SECONDS, Stack, cleanup_actions, ready_wait  # noqa: F401 (soak.ready_wait is the soak's own)
from soak_audit import DISK_RESUME_MARGIN_SECONDS
from soak_stream import DISK_RECOVERY_SECONDS, SseClient

DRAIN_SECONDS = 600
MESSAGE_CAP = 50
HOUR = 3600
FAULT_KINDS = frozenset({"client_sse", "runtime_link", "gateway_restart"})
# Faults on the whole stack, with no target: checked where they are injected, not by a later witness.
STACK_WIDE_FAULTS = frozenset({"db_restart", "db_crash", "gateway_restart", "gateway_kill"})
WITNESS_SECONDS = 180
# After the drain the gateway may still be retiring idle sandboxes; the inventory must settle within this bound.
ORPHAN_SETTLE_SECONDS = 120
GROWTH_METRICS = ("gateway_rss_bytes", "open_fds", "database_connections")
# The driver's own threads: 16 stream readers, the sampler, and the engine proxy's two per open engine connection stay
# well below this. Threads count against the user's process limit, shared with everything else the user runs, so a leak
# stops the run long before it can starve other work (R2 leaked about 30 a second).
MAX_DRIVER_THREADS = 1000


def check_own_threads(count):
    """Stops the run when the driver's own thread count passes its bound."""
    require(count <= MAX_DRIVER_THREADS,
            "the soak driver has {} threads; its bound is {}: a leak in the harness, stopped before it exhausts this "
            "user's process limit".format(count, MAX_DRIVER_THREADS))


def run_seconds(hours, rehearsal):
    """The 2 h run (SOAK-002 short), the 8 h microVM run (R3) and the 24 h run (SOAK-001…003, PRD M5) are gate
    evidence; a rehearsal never is."""
    require(hours in (2, 4, 8, 24), "--hours must be 2, 4 (real model), 8 or 24")
    return 300 if rehearsal else hours * HOUR


def journal_cap(hours):
    """The chaos journal's cap: 16 MiB per two hours."""
    return 16 * 1024 * 1024 * max(1, int(hours) // 2)


def needs_generation(completed, sent, rotate_after=None):
    """Replace a conversation two cycles before its script or its message cap runs out (a rehearsal may rotate
    after `rotate_after` responses to prove the path)."""
    limit = RESPONSES - 6 if rotate_after is None else rotate_after
    return completed >= limit or sent >= MESSAGE_CAP - 5


def check_rotate_after(rotate_after, rehearsal):
    """Early rotation is test-only: a rehearsal's, never gate evidence."""
    if rotate_after is not None:
        require(rehearsal, "--rotate-after is test-only and needs --rehearsal")
        require(0 < rotate_after < RESPONSES - 6, "--rotate-after must be between 1 and the script's own limit")


def log_windows(start, now, last, final=False):
    """The whole hours since `last` (collected hourly from `start`), and at the end the rest."""
    windows = []
    while last + HOUR <= now:
        windows.append((last, last + HOUR))
        last += HOUR
    if final and now > last:
        windows.append((last, now))
    return windows


def slot_action(rotate, owes_completion, pending_fault):
    """What a settled slot does next. A conversation still owes a successful run after its latest fault; it gets one
    more message first (the rotation margin allows for it). It rotates only when no fault in it, and no global fault
    (target None: a new conversation's first start would meet it), awaits its witness."""
    if not rotate or owes_completion:
        return "submit"
    return "wait" if pending_fault else "rotate"


def control_frames(clients):
    """The unknown unsequenced control frames a conversation's clients counted, by kind."""
    counts = {}
    for client in clients:
        for kind, count in client.control_frames.items():
            counts[kind] = counts.get(kind, 0) + count
    return counts


def disk_recovery(window, steps, completions, now, final=False, margin=DISK_RECOVERY_SECONDS):
    """A conversation that got `io` in a disk_full window recovers in two parts: a step completes after the free and
    within the margin (progress resumed promptly), and a run completes after the free before the soak ends (the work
    still finishes). A scripted run lasts several steps, longer than the margin (the c14b soak, 5 Oct 2026: a step's
    median 69 s, a run's p90 356 s), so the margin bounds the first step, not a whole run. `failed_step` once the
    margin passes without a step, `failed_run` when the soak ends with no run completed, `recovered` with both, else
    `pending`. A step or run completed before the free proves nothing."""
    end = window[1]
    if end == float("inf"):
        return "pending"
    stepped = any(end <= step <= end + margin for step in steps)
    if not stepped:
        return "failed_step" if now > end + margin else "pending"
    if any(completion >= end for completion in completions):
        return "recovered"
    return "failed_run" if final else "pending"


def io_recovery(window, completions, now, margin=DISK_RECOVERY_SECONDS):
    """A run must complete within the margin after `window` closes: `recovered` once one does, `failed` when the margin
    passes without one, else `pending`. A store_unavailable outside a database fault uses it (store_recoveries); a full
    disk uses disk_recovery. A completion before the window's end proves nothing."""
    end = window[1]
    if end == float("inf"):
        return "pending"
    if any(end <= completion <= end + margin for completion in completions):
        return "recovered"
    return "failed" if now > end + margin else "pending"


def planned_kinds(backend, excluded):
    """The disruption kinds this run plans: the backend's, less those left out on purpose (`--exclude`, recorded in the
    result). Only disruptions can be left out; the reconnect plan and the restarts always run."""
    kinds = backend_kinds(backend)
    for kind in excluded:
        require(kind in kinds, "--exclude takes a disruption kind of this backend ({}), not {}".format(
            ", ".join(kinds), kind))
    return tuple(kind for kind in kinds if kind not in excluded)


def replacement_times(log_text, conversation, resumed_at):
    """When the gateway destroyed and started this conversation's sandbox before the run resumed at `resumed_at`
    (Unix seconds), from its own log lines; None where the log holds no such line."""
    destroyed = started = None
    for line in log_text.splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if not isinstance(record, dict) or record.get("span", {}).get("conversation_id") != conversation:
            continue
        stamp = timestamp_seconds(record["timestamp"])
        if stamp > resumed_at:
            continue
        message = record.get("fields", {}).get("message")
        if message == "destroyed sandbox":
            destroyed, started = stamp, None
        elif message == "started sandbox" and destroyed is not None:
            started = stamp
    return {"destroyed": destroyed, "started": started, "resumed": resumed_at,
            "destroy_to_resume_seconds": None if destroyed is None else resumed_at - destroyed}


# The gateway's store may time out on a busy host outside every database fault (its clients get store_unavailable).
# Accepted as load, not a fault: the conversation must complete a run within this margin after the notice, the
# notices are counted in the result, and more than STORE_EVENTS_PER_HOUR stalls in any hour fail the run.
STORE_RECOVERY_SECONDS = 180
STORE_EVENTS_PER_HOUR = 5


def store_events(times, gap=10.0):
    """One stall reaches several clients within seconds: notices closer than `gap` to the event's first are one."""
    events = []
    for when in sorted(times):
        if not events or when - events[-1] > gap:
            events.append(when)
    return events


def too_many_store_events(events, per_hour=STORE_EVENTS_PER_HOUR):
    """Whether any rolling hour holds more than `per_hour` events."""
    events = sorted(events)
    return any(sum(1 for later in events[i:] if later - first < 3600) > per_hour for i, first in enumerate(events))


# Resource samples on a busy host: a sample may miss while the engine is loaded (builds, other work); the run fails
# only when the sampler has stopped working (no success for this long) or too few points remain to judge growth.
SAMPLE_STALL_SECONDS = 15 * 60


def missed_sample_problem(missed, attempts):
    """Why the missed resource samples leave growth unjudgeable (more than a tenth, and more than two), else None."""
    if missed > max(2, attempts // 10):
        return "missed {} of {} resource samples: growth can't be judged".format(missed, attempts)
    return None


def replacements_on_full_disk(times, windows, margin):
    """The replacement times inside a full-disk window or its margin: the gateway replaced those runtimes by design."""
    return sorted(when for when in times if any(start <= when <= end + margin for start, end in windows))


def probe_verdict(refusal):
    """The full-disk probe's message: a clean typed refusal (an HTTP error whose body names a code), or accepted. An
    accepted message must then complete within the margin after the free, which the recovery check enforces like any
    other conversation's; only an untyped refusal fails here."""
    if refusal is not None:
        return "refused: {} {}".format(refusal["status"], refusal["code"]) if refusal.get("code") else "failed"
    return "accepted"


def probe_slot(settled):
    """A conversation with no run in flight, so the probe's message starts a run on the full disk; None when every
    one is busy. A message into a busy conversation is answered `busy`, which the full-disk rule doesn't excuse."""
    return next((index for index, ready in enumerate(settled) if ready), None)


def wait_for_idle(settled, *, poll, seconds, clock=time.monotonic, sleep=time.sleep):
    """The first idle conversation within `seconds`, draining events between checks so a run's end is seen; None if
    none became idle in time."""
    deadline = clock() + seconds
    while True:
        index = probe_slot(settled())
        if index is not None or clock() >= deadline:
            return index
        poll()
        sleep(0.25)


def probe_outcome(probe):
    """A skipped probe (no conversation was idle while the disk was full) is recorded, not judged; a sent one is
    judged by its refusal."""
    return "skipped" if probe.get("skipped") else probe_verdict(probe["refusal"])


def link_witnessed(backend, gateway_closed, runtime_logged, snapshots_grew):
    """A paused runtime's link came back: the gateway closed it and, on Docker, the runtime logged its replay; a VM
    host's log may not carry the guest's, so on a microVM a client's new snapshot stands in for it."""
    if backend == "firecracker":
        return gateway_closed and snapshots_grew
    return gateway_closed and runtime_logged


def progressed(receipts, cursor, restored_at):
    """Frames past `cursor`, made after the link was restored, reached a client: they came from the runtime over the
    restored link. A cut shorter than the gateway's link liveness bound (90 s) leaves the link up, so the runtime logs no
    disconnect and the clients take no new snapshot; a run in flight streaming on is its witness."""
    return any(receipt.last_seq > cursor and receipt.last_ts is not None and receipt.last_ts >= restored_at
               for receipt in receipts)


def started_and_connected(audit, clients):
    """A conversation is under way once its first run started (it may already have ended: a fast script and automatic
    approvals can finish it before the last conversation opens) and both its clients are connected."""
    return audit.started >= 1 and all(client.ready.is_set() for client in clients)


def covered(kinds_per_conversation, kinds=FAULT_KINDS):
    """Every fault kind reached some conversation (a restart is marked on every live one)."""
    return set().union(*kinds_per_conversation) == set(kinds)


class Soak:
    """Own readers, lifecycle audits, the seeded schedule and bounded evidence files."""
    def __init__(self, stack, options):
        self.stack, self.options = stack, options
        stack.on_slow = self.record_slow
        # A renewed session (it ended 24 h after sign-in, past its known end): a journal event, with its age.
        stack.on_renewal = lambda fields: self.log("session_renewed", **fields)
        self.seconds = run_seconds(options.hours, options.rehearsal)
        self.slots = options.conversations
        restarts = restart_times(self.seconds, options.rehearsal)
        kinds = planned_kinds(options.backend, options.exclude)
        planned = [] if options.reconnect_only else disruptions(options.seed, self.seconds, options.rehearsal,
                                                                  self.slots, restarts, kinds)
        self.plan = schedule(options.seed, self.seconds, options.rehearsal, slots=self.slots, restarts=restarts,
                             disruptions=planned)
        self.kinds = FAULT_KINDS if options.reconnect_only else FAULT_KINDS | frozenset(kinds)
        if not options.rehearsal:
            for kind, count in (("client_sse", 2 * self.slots), ("runtime_link", self.slots)):
                require({fault["target"] for fault in self.plan if fault["kind"] == kind} == set(range(count)),
                        "seed {} leaves some {} targets unplanned; pick another".format(options.seed, kind))
        self.database_windows, self.disk_windows = [], []
        self.recovered = set()
        self.store_recovered = set()
        # Conversations probed during a full disk, with the window they must recover from.
        self.probed = {}
        self.disk_replacements = []
        self.events = queue.Queue(maxsize=512)
        self.clients, self.conversations, self.audits, self.progress = [], [], [], []
        self.sent, self.windows, self.pending = [], [], []
        self.faults, self.samples = [], []
        self.missed_samples, self.clock = 0, time.monotonic
        self.last_sample_ok = self.clock()
        self.sampler = concurrent.futures.ThreadPoolExecutor(max_workers=1, thread_name_prefix="sampler")
        self.pending_sample = None
        self.generation, self.finished, self.retired = [0] * self.slots, [], []
        self.log_start = self.log_last = None
        self.journal = None
        self.started, self.ended = None, None

    def record_slow(self, measurement):
        """A harness engine command that was slow on a saturated engine: a measurement in the journal, not a failure.
        While the harness was blocked in it nothing could be witnessed, so every pending fault's witness deadline moves
        on by that time (R2 on candidate 13e: a pending link fault ran out during an 83 s kill). Only the deadline moves;
        a fault's own window (its resume, its free) stays the stack's time."""
        for fault in self.pending:
            fault["deadline"] += measurement["seconds"]
        self.log("engine_slow", **measurement)

    def log(self, kind, **fields):
        record = {"kind": kind, "time": time.time(), **fields}
        line = json.dumps(record, sort_keys=True) + "\n"
        if self.journal is not None:
            require(self.journal.tell() + len(line.encode()) <= journal_cap(self.options.hours),
                    "soak journal exceeded its cap")
            self.journal.write(line)
            self.journal.flush()
        print(line, end="", flush=True)

    def initialize(self):
        self.journal = (self.stack.output / "chaos.jsonl").open("x", buffering=1)
        self.log("configuration", seed=self.options.seed, rehearsal=self.options.rehearsal, hours=self.options.hours,
                 excluded_kinds=sorted(self.options.exclude),
                 conversations=self.slots, chaos_seconds=self.seconds, drain_seconds=DRAIN_SECONDS,
                 images=self.stack.images, plan=self.plan,
                 # The load sampler counts this project's containers only (soak_load.py).
                 project=self.stack.env.get("COMPOSE_PROJECT_NAME"))
        self.conversations, self.audits = [None] * self.slots, [None] * self.slots
        self.forgotten = [0] * self.slots
        self.progress, self.sent, self.clients = [None] * self.slots, [0] * self.slots, [None] * (2 * self.slots)
        for index in range(self.slots):
            self.open_conversation(index)
        self.wait_active(range(self.slots))

    def open_conversation(self, index):
        """A new conversation in slot `index`, with its two clients, its audits and its first message."""
        generation = self.generation[index]
        conversation = self.stack.request("/api/v1/conversations", {
            "title": "Soak {} generation {}".format(index, generation), "model_route": "local"})["id"]
        self.conversations[index] = conversation
        real = self.options.model == "real"
        audit = RunAudit(scripted_tools=not real)
        audit.restart_windows, audit.disk_windows = self.windows, self.disk_windows
        audit.database_windows = self.database_windows
        progress = soak_real.RealProgress() if real else ScenarioProgress()
        self.audits[index], self.progress[index], self.sent[index] = audit, progress, 0
        # The new audit counts its own replacements from none, so the slot's forgotten count starts again too.
        self.forgotten[index] = 0
        for member in range(2):
            name = "{}/{}".format(index, member)
            receipts = ReceiptLog(self.stack.output / "client-{}-{}-g{}.jsonl".format(index, member, generation))
            address = self.stack.origin, self.path(index) + "/events?stream=1&protocol=1", self.stack.cookie, name
            client = SseClient(address, receipts, self.events if member == 0 else None, self.windows,
                               self.database_windows, self.disk_windows, session=self.stack)
            self.clients[2 * index + member] = client
            client.start()
        self.submit(index)
        self.log("conversation_opened", slot=index, generation=generation, conversation=conversation)

    def wait_active(self, indices):
        deadline = time.monotonic() + self.stack.ready_wait_seconds
        while not all(started_and_connected(self.audits[index], self.clients[2 * index:2 * index + 2])
                      for index in indices):
            require(time.monotonic() < deadline, "conversations and their clients were not ready")
            self.poll()
        for index in indices:
            self.stack.sandbox(self.conversations[index])

    def path(self, index):
        return "/api/v1/conversations/" + self.conversations[index]

    def submit(self, index):
        require(self.sent[index] < MESSAGE_CAP, "user-turn cap reached")
        text = (soak_real.prompt(self.sent[index]) if self.options.model == "real"
                else "Continue scripted work {}".format(self.sent[index]))
        self.stack.request(self.path(index) + "/messages", {"text": text, "attachments": []})
        self.sent[index] += 1

    def poll(self):
        check_own_threads(threading.active_count())
        for client in self.clients:
            if client.failure is not None:
                raise client.failure
        for _ in range(512):
            try:
                name, frame = self.events.get_nowait()
            except queue.Empty:
                break
            index = int(name.split("/")[0])
            try:
                self.audits[index].record(frame)
                self.progress[index].record(frame.get("event", {}))
            except BaseException:
                self.log("rejected_frame", conversation=index, frame=frame)
                raise
            kind = frame.get("event", {}).get("type")
            if kind in ("run.completed", "run.failed", "provider.retry"):
                self.log("run_event", conversation=index, frame=frame)
        self.forget_replaced_runtimes()

    def forget_replaced_runtimes(self):
        """A runtime the gateway replaced on a full disk (a resumed run, or an idle sandbox's creating notice) is not
        the one the harness remembers; forgetting it keeps the check that a healthy runtime was never replaced, which
        still fails for a replacement anywhere else (a gateway restart that replaced one, for instance)."""
        for index in range(self.slots):
            times = list(self.audits[index].disk_resumes) + [
                when for client in self.clients[2 * index:2 * index + 2] for when in client.recovery.replaced_at]
            replaced = replacements_on_full_disk(times, self.disk_windows, DISK_RESUME_MARGIN_SECONDS)
            if len(replaced) > self.forgotten[index]:
                self.forgotten[index] = len(replaced)
                self.stack.runtimes.pop(self.conversations[index], None)
                self.log("runtime_replaced_on_full_disk", conversation=index, at=replaced[-1])

    def database_down(self):
        """Inside a database fault's window, nothing is sent and nothing reads the database."""
        now = time.time()
        return any(start <= now <= end for start, end in self.database_windows)

    def sending_paused(self):
        """No new message while the database is down or the disk is full: only runs already in flight when the disk
        filled meet it, and a conversation's recovery is proven after the free."""
        return self.database_down() or any(end == float("inf") for _, end in self.disk_windows)

    def owes_io_recovery(self, index):
        clients = self.clients[2 * index:2 * index + 2]
        conversation = self.conversations[index]
        disk_resumed = bool(self.audits[index].disk_resumes) if getattr(self, "audits", None) else False
        return (any(client.io_times for client in clients) or disk_resumed
                or conversation in getattr(self, "probed", {})) and conversation not in self.recovered

    def replenish(self, draining):
        if self.sending_paused():
            return
        for index, audit in enumerate(self.audits):
            # A message accepted by HTTP may not yet have reached this client's journal.
            settled = audit.started == self.sent[index] and audit.active is None
            if not settled or self.conversations[index] in self.stack.paused:
                continue
            owes = audit.needs_completion() or self.owes_io_recovery(index)
            if draining:
                if owes:
                    self.submit(index)
                continue
            action = slot_action(
                needs_generation(self.progress[index].completed, self.sent[index], self.options.rotate_after),
                owes, any(self.slot_of(fault) in (index, None) for fault in self.pending))
            if action == "submit":
                self.submit(index)
            elif action == "rotate":
                self.rotate(index)

    @staticmethod
    def slot_of(fault):
        return fault["target"] // 2 if fault["kind"] == "client_sse" else fault["target"]

    def rotate(self, index):
        self.finished.append(self.close_conversation(index))
        self.generation[index] += 1
        self.open_conversation(index)
        self.wait_active([index])

    def close_conversation(self, index):
        """Slot `index`'s clients receive its complete history and stop; its receipts and runs are checked."""
        conversation, clients = self.conversations[index], self.clients[2 * index:2 * index + 2]
        self.io_recoveries(closing=index)
        self.store_recoveries(final=True)
        watermark = sum(len(page) for page in self.stack.stored(conversation))
        deadline = time.monotonic() + 60
        while any(client.receipts.last_seq < watermark for client in clients):
            require(time.monotonic() < deadline, "a client did not receive the complete history")
            self.poll()
            time.sleep(0.05)
        for client in clients:
            client.stop()
        for client in clients:
            client.join()
        self.poll()
        audit = self.audits[index]
        audit.verify()
        audit.verify_workload()
        require(audit.messages == self.sent[index], "accepted messages differ from recorded messages")
        # Every call runs (ADR-0035): nothing asks, so an approval request is a regression.
        require(audit.kinds.get("permission.request", 0) == 0, "a run asked for an approval")
        require(audit.kinds.get("tool.result", 0) >= 2, "no repeated tool work")
        counts = [client.receipts.verify(self.stack.stored(conversation)) for client in clients]
        for client in clients:
            client.receipts.output.close()
        self.retired += clients
        if audit.disk_resumes:
            for resumed in audit.disk_resumes:
                # Only the minutes before the resume: a 24 h log would pass the log cap and lose these lines.
                log = self.stack.window_logs(self.stack.gateway, resumed - 300, resumed + 5)
                timing = dict(replacement_times(log, conversation, resumed), conversation=conversation)
                self.disk_replacements.append(timing)
                self.log("disk_replacement", timing=timing)
        summary = {"id": conversation, "slot": index, "generation": self.generation[index], "client_events": counts,
                   "runs": audit.started, "completed": audit.completed, "failed_during_restart": audit.failed,
                   "retry_evidence": audit.retry_evidence, "event_kinds": audit.kinds, "faults": sorted(set(audit.faults)),
                   "tool_outcomes": audit.tool_outcomes, "pause_tool_timeouts": audit.tool_timeouts,
                   "script_responses": self.progress[index].completed,
                   "control_frames": control_frames(clients)}
        self.log("conversation_closed", summary=summary)
        return summary

    def collect_logs(self, final=False):
        """The gateway's and the model's logs, hour by hour, so a long run keeps all of them within caps."""
        for since, until in log_windows(self.log_start, time.time(), self.log_last, final):
            hour = int((since - self.log_start) // HOUR)
            for name, identity in (("gateway", self.stack.gateway), ("fake-model", self.stack.model)):
                compose.write(self.stack.output / "{}-hour-{:02d}.log".format(name, hour),
                              self.stack.window_logs(identity, since, until))
            self.log_last = until

    def mark_fault(self, kind, indices):
        for index in indices:
            sequence = max(client.receipts.last_seq for client in self.clients[index * 2:index * 2 + 2])
            self.audits[index].fault(kind, sequence)

    def inject(self, fault):
        kind, target = fault["kind"], fault["target"]
        record = dict(fault, actual=time.monotonic() - self.started, timestamp=time.time())
        self.log("fault_started", fault=record)
        if kind == "client_sse":
            client = self.clients[target]
            self.mark_fault(kind, [target // 2])
            record.update(cursor=client.receipts.last_seq, connections=client.connections, snapshots=client.snapshots)
            client.drop()
            record["deadline"] = time.monotonic() + 60
            self.pending.append(record)
        elif kind == "runtime_link":
            require(not any(start <= time.time() <= end for start, end in self.windows), "overlapping link/restart fault")
            self.mark_fault(kind, [target])
            record["pause_window"] = {"start": time.time(), "end": None}
            self.audits[target].pause_windows.append(record["pause_window"])
            record.update(identity=self.stack.pause(self.conversations[target]), resume_at=time.monotonic() + fault["pause_seconds"])
            record["deadline"] = record["resume_at"] + 60
            self.pending.append(record)
        elif kind in ("db_restart", "db_crash"):
            self.mark_fault(kind, range(self.slots))
            window = [time.time(), time.time() + 120]
            self.database_windows.append(window)
            self.stack.database_fault(kind)
            window[1] = time.time() + 30
            record["window"] = list(window)
        elif kind == "db_outage":
            self.mark_fault(kind, range(self.slots))
            record["window"] = [time.time(), float("inf")]
            self.database_windows.append(record["window"])
            self.stack.stop_database()
            record.update(resume_at=time.monotonic() + fault["seconds"],
                          deadline=time.monotonic() + fault["seconds"] + WITNESS_SECONDS)
            self.pending.append(record)
        elif kind == "disk_full":
            self.mark_fault(kind, range(self.slots))
            record["window"] = [time.time(), float("inf")]
            self.disk_windows.append(record["window"])
            record.update(left=self.stack.storage.fill(), free_at=time.monotonic() + fault["seconds"],
                          deadline=time.monotonic() + fault["seconds"] + WITNESS_SECONDS)
            # Half the fill at most, so a probe that waited still meets the full disk.
            record["probe"] = self.probe_full_disk(record["window"], seconds=fault["seconds"] / 2)
            self.pending.append(record)
        elif kind == "engine_stall":
            self.mark_fault(kind, range(self.slots))
            record.update(affected_before=self.stack.proxy.affected, window_end=time.monotonic() + fault["seconds"],
                          deadline=time.monotonic() + fault["seconds"] + WITNESS_SECONDS)
            self.stack.proxy.stall(fault["action"], fault["hold"], fault["seconds"])
            # The proxy stalls only the calls that change containers, which come when a sandbox is made or retired: an
            # idle conversation's sandbox is retired now, so its replacement goes through the stalled engine.
            slot = self.idle_slot()
            record["retired"] = {"slot": slot, "killed": self.stack.kill_sandbox(self.conversations[slot])}
            self.audits[slot].kill_windows.append(time.time())
            self.pending.append(record)
        elif kind == "sandbox_kill":
            self.mark_fault(kind, [target])
            self.audits[target].kill_windows.append(time.time())
            record.update(killed=self.stack.kill_sandbox(self.conversations[target]),
                          deadline=time.monotonic() + WITNESS_SECONDS)
            self.pending.append(record)
        elif kind == "link_cut":
            self.mark_fault(kind, [target])
            record.update(cut=self.stack.cut_link(self.conversations[target]), resume_at=time.monotonic() + fault["seconds"],
                          deadline=time.monotonic() + fault["seconds"] + WITNESS_SECONDS, restored=None)
            self.pending.append(record)
        else:
            require(kind in ("gateway_restart", "gateway_kill"), "unknown fault")
            self.mark_fault(kind, range(self.slots))
            self.windows.append((time.time(), time.time() + 90))
            (self.stack.restart if kind == "gateway_restart" else self.stack.kill_gateway)()
            self.windows[-1] = (self.windows[-1][0], time.time())
            record["window"] = self.windows[-1]
            for conversation in self.conversations:
                self.stack.sandbox(conversation)
        self.faults.append(record)
        self.log("fault", fault=record)
        if kind in STACK_WIDE_FAULTS:
            # Never pending, so no fault_witness: its recovery was checked above, where it was injected (the database
            # ready again, or the same gateway ready and every runtime unchanged). Reaching here means it passed.
            self.log("fault_recovered", fault_kind=kind, target=None)

    def idle_slot(self):
        """A conversation with no run in flight and nothing unanswered, not paused; the first one when none is."""
        for index, audit in enumerate(self.audits):
            if audit.active is None and audit.started == self.sent[index] and \
                    self.conversations[index] not in self.stack.paused:
                return index
        return 0

    def resumed_link(self, fault):
        target, conversation = fault["target"], self.conversations[fault["target"]]
        clients = self.clients[target * 2:target * 2 + 2]
        if conversation in self.stack.paused:
            if time.monotonic() < fault["resume_at"]:
                return False
            fault["snapshots"] = sum(client.snapshots for client in clients)
            self.stack.unpause(conversation)
            fault["pause_window"]["end"] = time.time()
            fault["resumed_after_seconds"] = time.monotonic() - self.started
            gateway = [json.loads(line) for line in self.stack.logs(self.stack.gateway, fault["timestamp"]).splitlines()
                       if line.startswith("{")]
            fault["gateway_link_closed"] = any(record.get("fields", {}).get("message") == "runtime link closed"
                                                for record in gateway)
            self.log("runtime_unpaused", fault=fault)
        if sum(client.snapshots for client in clients) <= fault["snapshots"]:
            return False
        logged = False
        if self.options.backend == "docker":
            runtime = self.stack.sandbox(conversation)["Id"]
            logged = "runtime link disconnected; durable frames await replay" in self.stack.logs(runtime, fault["timestamp"])
        return link_witnessed(self.options.backend, fault["gateway_link_closed"], logged, snapshots_grew=True)

    def database_back(self, fault):
        if fault["window"][1] == float("inf"):
            if time.monotonic() < fault["resume_at"]:
                return False
            self.stack.start_database()
            fault["window"][1] = time.time() + 30
            fault["restarted_after_seconds"] = time.monotonic() - self.started
        return self.gateway_ready()

    def gateway_ready(self):
        try:
            with urllib.request.urlopen(self.stack.origin + "/readyz", timeout=3) as response:
                return response.status == 200
        except OSError:
            return False

    def link_back(self, fault):
        conversation = self.conversations[fault["target"]]
        clients = self.clients[fault["target"] * 2:fault["target"] * 2 + 2]
        if fault["restored"] is None:
            if time.monotonic() < fault["resume_at"]:
                return False
            fault["snapshots"] = sum(client.snapshots for client in clients)
            fault["cursor"] = max(client.receipts.last_seq for client in clients)
            fault["restored"] = self.stack.restore_link(conversation)
            fault["restored_at"] = time.time()
            fault["same_addresses"] = fault["restored"] == fault["cut"]["networks"]
        logs = self.stack.logs(fault["cut"]["container"], fault["timestamp"])
        return "runtime link disconnected; durable frames await replay" in logs or \
            sum(client.snapshots for client in clients) > fault["snapshots"] or \
            progressed([client.receipts for client in clients], fault["cursor"], fault["restored_at"])

    def probe_full_disk(self, window, seconds):
        """One idle conversation sends a message on purpose while the disk is full; the others stay quiet. With none
        idle within the fill (bounded), the probe is skipped and recorded: never a message into a busy run."""
        settled = lambda: [audit.active is None and audit.started == self.sent[index]  # noqa: E731
                           and self.conversations[index] not in self.stack.paused
                           for index, audit in enumerate(self.audits)]
        index = wait_for_idle(settled, poll=self.poll, seconds=seconds)
        if index is None:
            probe = {"slot": None, "skipped": "no conversation was idle while the disk was full", "refusal": None}
            self.log("full_disk_probe", probe=probe)
            return probe
        probe = {"slot": index, "conversation": self.conversations[index], "sent_at": time.time(), "refusal": None}
        try:
            self.submit(index)
        except urllib.error.HTTPError as error:
            try:
                code = json.loads(error.read(65536) or b"{}").get("error", {}).get("code")
            except ValueError:
                code = None
            probe["refusal"] = {"status": error.code, "code": code}
        self.probed[self.conversations[index]] = window
        self.log("full_disk_probe", probe=probe)
        return probe

    def io_recoveries(self, final=False, closing=None):
        """Each conversation that met a full disk (an `io` notice, a disk resume, or the probe) must complete a step
        within the margin after the free and a run before its end (disk_recovery). `final` judges every slot as at the
        soak's end; `closing` judges that one slot so, as it rotates: a run still owed then is a failure, whichever way
        the conversation was considered."""
        for index in range(self.slots):
            times = sorted([when for client in self.clients[2 * index:2 * index + 2] for when in client.io_times]
                           + list(self.audits[index].disk_resumes))
            conversation = self.conversations[index]
            if (not times and conversation not in self.probed) or conversation in self.recovered:
                continue
            window = self.probed.get(conversation) or next(
                (w for w in self.disk_windows if w[0] <= times[0] <= w[1] + DISK_RECOVERY_SECONDS), None)
            require(window is not None, "an io notice outside every disk_full window reached conversation " + conversation)
            audit = self.audits[index]
            ends = final or index == closing
            state = disk_recovery(window, audit.step_times, audit.completed_times, time.time(), final=ends)
            if state == "recovered":
                self.recovered.add(conversation)
                self.log("io_recovered", conversation=conversation, slot=index, notices=len(times))
                continue
            require(state != "failed_step",
                    "conversation {} got io in a full disk and completed no step within {} s after the free".format(
                        conversation, DISK_RECOVERY_SECONDS))
            require(state != "failed_run" and not (ends and state == "pending"),
                    "conversation {} got io in a full disk and completed no run after the free before the soak "
                    "ended".format(conversation))

    def store_recoveries(self, final=False):
        """Each store_unavailable outside a database fault needs a completed run in its conversation within the margin
        after it, and the stalls stay rare (see STORE_RECOVERY_SECONDS)."""
        now = time.time()
        for index in range(self.slots):
            conversation = self.conversations[index]
            times = sorted({when for client in self.clients[2 * index:2 * index + 2] for when in client.store_times})
            for when in times:
                if (conversation, when) in self.store_recovered:
                    continue
                state = io_recovery((when, when), self.audits[index].completed_times, now,
                                    margin=STORE_RECOVERY_SECONDS)
                if state == "recovered":
                    self.store_recovered.add((conversation, when))
                    self.log("store_recovered", conversation=conversation, slot=index, at=when)
                    continue
                require(state != "failed" and not (final and state == "pending"),
                        "conversation {} got store_unavailable outside a database fault and did not complete a run "
                        "within {} s".format(conversation, STORE_RECOVERY_SECONDS))
        require(not too_many_store_events(store_events(when for _, when in self.store_recovered)),
                "more than {} store_unavailable stalls in an hour".format(STORE_EVENTS_PER_HOUR))

    def witnesses(self):
        waiting = []
        for fault in self.pending:
            target = fault["target"]
            if fault["kind"] == "client_sse":
                client = self.clients[target]
                ready = client.connections > fault["connections"] and client.snapshots > fault["snapshots"]
                # Its replay needs the database: a client dropped during a database fault gets its full witness time
                # after the database is back (its window ends 30 s after the restart).
                if not ready and self.database_down():
                    fault["deadline"] = max(fault["deadline"], time.monotonic() + 60)
            elif fault["kind"] == "db_outage":
                ready = self.database_back(fault)
            elif fault["kind"] == "disk_full":
                if fault["window"][1] == float("inf") and time.monotonic() >= fault["free_at"]:
                    self.stack.storage.free()
                    fault["window"][1] = time.time()
                    probe = fault["probe"]
                    probe["verdict"] = probe_outcome(probe)
                    self.log("full_disk_probe_verdict", probe=probe)
                    require(probe["verdict"] != "failed", "the message sent on a full disk met an untyped refusal: "
                            + json.dumps(probe))
                ready = fault["window"][1] != float("inf") and self.gateway_ready()
            elif fault["kind"] == "engine_stall":
                ready = time.monotonic() >= fault["window_end"] and self.gateway_ready()
                if ready:
                    # Witnessed only when something met the stall and the retired sandbox was replaced through it.
                    fault["affected_calls"] = self.stack.proxy.affected - fault["affected_before"]
                    retired = fault["retired"]
                    replacement = self.stack.replacement(self.conversations[retired["slot"]], retired["killed"])
                    ready = fault["affected_calls"] >= 1 and replacement is not None
                    if ready:
                        fault["replacement"] = replacement
            elif fault["kind"] == "sandbox_kill":
                replacement = self.stack.replacement(self.conversations[target], fault["killed"])
                ready = replacement is not None
                if ready:
                    fault["replacement"] = replacement
            elif fault["kind"] == "link_cut":
                ready = self.link_back(fault)
            else:
                ready = self.resumed_link(fault)
            require(ready or time.monotonic() < fault["deadline"], "injected fault has no reconnect/replay witness")
            if ready:
                self.log("fault_witness", fault_kind=fault["kind"], target=target)
            else:
                waiting.append(fault)
        self.pending = waiting

    def measure(self):
        """Collects a finished resource sample and starts the next on the sampler's own worker, so a slow sample (a
        helper container slow to start on a busy host) never holds the loop that drains client events. A
        failed sample is recorded as missed; the run fails only if none has succeeded for SAMPLE_STALL_SECONDS."""
        if self.pending_sample is not None and not self.pending_sample.done():
            return
        self.collect_sample()
        self.pending_sample = self.sampler.submit(self.stack.resources)
        if self.pending_sample.done():
            self.collect_sample()

    def collect_sample(self):
        pending, self.pending_sample = self.pending_sample, None
        if pending is None:
            return
        try:
            sample = pending.result()
        except (OSError, RuntimeError, subprocess.SubprocessError) as error:
            self.missed_samples += 1
            self.log("resources_missed", reason=type(error).__name__, missed=self.missed_samples)
            require(self.clock() - self.last_sample_ok < SAMPLE_STALL_SECONDS,
                    "no resource sample succeeded for 15 minutes")
            return
        self.last_sample_ok = self.clock()
        self.samples.append(sample)
        self.log("resources", **sample, driver_threads=threading.active_count(), cursors=[client.receipts.last_seq for client in self.clients],
                 completed=[audit.completed for audit in self.audits])

    def exercise(self):
        self.started = time.monotonic()
        self.log_start = self.log_last = time.time()
        self.log("chaos_started")
        next_fault, next_sample = 0, 0
        deadline = self.started + self.seconds + DRAIN_SECONDS
        while True:
            self.poll()
            now = time.monotonic()
            elapsed = now - self.started
            if next_fault < len(self.plan) and elapsed >= self.plan[next_fault]["at"]:
                self.inject(self.plan[next_fault])
                next_fault += 1
                self.poll()
            self.witnesses()
            self.io_recoveries()
            self.store_recoveries()
            draining = elapsed >= self.seconds and next_fault == len(self.plan)
            self.replenish(draining)
            if elapsed >= next_sample and not self.database_down():
                self.measure()
                self.collect_logs()
                next_sample = elapsed + 30
            if draining and not self.pending and all(audit.active is None and audit.started == sent
                    and not audit.needs_completion() for audit, sent in zip(self.audits, self.sent)):
                break
            require(now < deadline, "soak drain deadline exceeded; a run or replay did not finish")
            time.sleep(0.05)
        self.ended = time.monotonic()
        self.measure()

    def verify(self):
        self.poll()
        conversations = self.finished + [self.close_conversation(index) for index in range(self.slots)]
        planned = {(fault["kind"], fault["target"]) for fault in self.plan}
        require(planned <= {(fault["kind"], fault["target"]) for fault in self.faults}, "a planned fault never ran")
        require(covered([set(conversation["faults"]) for conversation in conversations],
                        {kind for kind, _ in planned}), "missing fault kind")
        orphans = self.orphans({conversation["id"] for conversation in conversations})
        # M5 is judged on the 24 h run's one 12 h gateway lifetime (hours 12–24, free of restarts); the 2 h run has no
        # lifetime of 4 h and reports growth as not judged.
        problem = missed_sample_problem(self.missed_samples, len(self.samples) + self.missed_samples)
        require(problem is None, problem or "")
        growth = verdict(self.samples, self.windows, GROWTH_METRICS,
                         require_judged=self.options.hours in (8, 24) and not self.options.rehearsal,
                         require_floor=self.options.hours == 24 and not self.options.rehearsal)
        self.log("growth", verdict=growth)
        require(growth["passed"], "resource growth: " + growth["reason"])
        return {"status": "passed", "gate_evidence": self.stack.evidence(self.options.rehearsal),
                "seed": self.options.seed,
                "growth": growth, "orphans": orphans, "disk_replacements": self.disk_replacements,
                "missed_samples": self.missed_samples,
                "slow_engine_commands": self.stack.slow_commands,
                "gateway_link_retries": {"count": sum(len(audit.link_waits) for audit in self.audits),
                                         "waited_ms": sorted(w for audit in self.audits for w in audit.link_waits)},
                "store_unavailable_under_load": {
                    "notices": len(self.store_recovered),
                    "events": len(store_events(when for _, when in self.store_recovered)),
                    "note": "gateway store timed out outside every database fault; each conversation recovered"},
                "excluded_kinds": sorted(self.options.exclude),
                "hours": self.options.hours, "conversation_slots": self.slots, "generations": len(conversations),
                "chaos_seconds": self.seconds, "elapsed_seconds": self.ended - self.started,
                "conversations": conversations, "faults": self.faults, "images": self.stack.images,
                "model": self.stack.verify_model() if self.options.model == "fake" else {"real": self.stack.model_route},
                "samples": len(self.samples),
                "gateway_rss_high_water_bytes": max(sample["gateway_rss_bytes"] for sample in self.samples),
                "database_high_water_bytes": max(sample["database_bytes"] for sample in self.samples),
                "open_fds_high_water": max(sample["open_fds"] for sample in self.samples),
                "database_connections_high_water": max(sample["database_connections"] for sample in self.samples)}

    def orphans(self, conversations):
        """H6: nothing left behind (PRD M5). Returns what the settled inventory held."""
        deadline = time.monotonic() + ORPHAN_SETTLE_SECONDS
        while True:
            containers, rows, networks, volumes = self.stack.inventory()
            problems = orphan_findings(containers, rows, networks, volumes, conversations)
            if not problems:
                self.log("orphan_audit", containers=len(containers), rows=len(rows), networks=len(networks),
                         volumes=len(volumes))
                return {"containers": len(containers), "admission_rows": len(rows), "networks": len(networks),
                        "volumes": len(volumes), "problems": []}
            require(time.monotonic() < deadline, "left behind after the drain: " + "; ".join(problems[:10]))
            time.sleep(5)

    def close(self, failure):
        clients = [client for client in self.clients if client is not None]
        for client in clients:
            client.stop()
        actions = [client.join for client in clients]
        actions += [client.receipts.output.close for client in clients]
        if self.log_start is not None and getattr(self.stack, "gateway", None):
            actions.append(lambda: self.collect_logs(final=True))
        if self.journal is not None:
            actions.append(self.journal.close)
        return cleanup_actions(actions, failure)


def interrupted(_signal, _frame):
    raise KeyboardInterrupt("soak interrupted")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hours", type=int, default=2,
                        help="2 (SOAK-002 short), 8 (the microVM run) or 24 (SOAK-001…003)")
    parser.add_argument("--model", choices=("fake", "real"), default="fake",
                        help="real: TQA_SOAK_MODEL_URL and TQA_SOAK_MODEL_NAME name an endpoint the operator started")
    parser.add_argument("--backend", choices=("docker", "firecracker"), default="docker",
                        help="firecracker needs a KVM host, TQA_VMM_IMAGE and TQA_FC_KERNEL_DIR (deploy/firecracker)")
    parser.add_argument("--conversations", type=int, choices=(4, 8), default=8)
    parser.add_argument("--storage-mib", type=int, default=64,
                        help="the storage root's bounded tmpfs, in MiB, for disk_full (H5)")
    parser.add_argument("--exclude", action="append", default=[], metavar="KIND",
                        help="leave one disruption kind out on purpose (repeatable); recorded in the result")
    parser.add_argument("--reconnect-only", action="store_true",
                        help="only stream drops, link pauses and restarts (the Gate 2 shape); no disruptions")
    parser.add_argument("--seed", type=int, default=731)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rehearsal", action="store_true", help="five-minute plumbing check; never gate evidence")
    # Test-only: rotate each conversation after this many responses, to prove the generation path in a rehearsal.
    parser.add_argument("--rotate-after", type=int, help=argparse.SUPPRESS)
    options = parser.parse_args()
    run_seconds(options.hours, options.rehearsal)
    soak_real.check_mode(options.model, options.hours, options.rehearsal, options.conversations)
    check_rotate_after(options.rotate_after, options.rehearsal)
    # The real-model run (R4) keeps to reconnect faults and hourly restarts: the GPU slot is not where the engine and the
    # database are also stressed.
    options.reconnect_only = options.reconnect_only or options.model == "real"
    require(os.environ.get("TQA_DOCKER_TESTS") == "1", "Docker tests require TQA_DOCKER_TESTS=1")
    options.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if key != "COMPOSE_PROJECT_NAME"}
    # The soak reads its own database: no stack reader beside it; set here, not in soak_stack.Stack (perf wants one).
    env["TQA_STACK_READINGS"] = "off"
    temporary = tempfile.TemporaryDirectory(prefix="tqa-soak-")
    directory = compose.installation(temporary)
    stack = Stack(directory, env, options.output, backend=options.backend, model=options.model)
    soak, failure = Soak(stack, options), None
    previous = signal.signal(signal.SIGTERM, interrupted)
    result = {"status": "failed", "seed": options.seed, "gate_evidence": False}
    try:
        # A microVM's workspace is its own block image; the bounded tmpfs is a Docker-backend fault (H5).
        bounded = not options.reconnect_only and options.backend == "docker" and "disk_full" not in options.exclude
        stack.prepare(options.rehearsal, options.conversations, engine_proxy=not options.reconnect_only,
                      storage_mib=options.storage_mib if bounded else None)
        soak.initialize()
        soak.exercise()
        result = soak.verify()
    except BaseException as error:
        failure = error
        result["error"] = compose.redact(str(error), env)
        result["slow_engine_commands"] = stack.slow_commands
        print("Soak failed: " + result["error"], flush=True)
    finally:
        # A sample still on its worker is abandoned; its labelled helper is swept with the others.
        soak.sampler.shutdown(wait=False, cancel_futures=True)
        failure = soak.close(failure)
        failure = cleanup_actions([stack.cleanup_pauses, stack.cleanup_faults, stack.save_logs, stack.cleanup_helpers],
                                  failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        # The gateway may call the engine while it retires, so the stall proxy stops after it.
        # The storage tmpfs comes off before the installation directory is removed.
        failure = cleanup_actions([stack.stop_engine_proxy, stack.unbound_storage], failure)
        try:
            compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
        except BaseException as error:
            failure = error
        signal.signal(signal.SIGTERM, previous)
        if failure is not None:
            result.update(status="failed", error=compose.redact(str(failure), env), gate_evidence=False)
        compose.write(options.output / "result.json", json.dumps(result, indent=2, sort_keys=True) + "\n")
    require(failure is None, "the soak failed; see the bounded evidence directory")
    print("Soak {}: every client's receipts equal the stored history; every run terminal".format(
        "rehearsal passed (not gate evidence)" if options.rehearsal else "{} h passed".format(options.hours)),
        flush=True)


if __name__ == "__main__":
    main()
