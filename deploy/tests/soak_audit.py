"""SOAK-002: strict receipt equality and explicit run outcomes, without deduplication."""
import datetime
import json
import re

MAX_RECEIPT_BYTES = 64 * 1024 * 1024
MODEL_ATTEMPTS = 3  # The runtime's bounded model retry contract.
TOOL_RECOVERY_SECONDS = 10
# After a sandbox kill, the one resume and the interrupted in-flight tool arrive within this bound.
KILL_RECOVERY_SECONDS = 180
# A resume on a full disk counts inside the disk_full window and this margin after its free (SOAK-001).
DISK_RESUME_MARGIN_SECONDS = 180
# The product's bound on replacing a sandbox that keeps stopping: 4 replacements in a rolling 10 minutes, waiting this
# long before each; a 5th loss is a retryable sandbox_failed instead (GW-I-112).
REPLACEMENT_BACKOFF_SECONDS = (0, 5, 15, 45)
REPLACEMENT_WINDOW_SECONDS = 600
FAULT_KINDS = ("client_sse", "runtime_link", "gateway_restart", "gateway_kill", "db_restart", "db_outage", "db_crash",
               "engine_stall", "disk_full", "sandbox_kill", "link_cut")
NO_SPACE = "No space left on device"


class AuditError(AssertionError):
    """The observed transcript or the harness could not prove the soak invariant."""


def require(condition, message):
    """Keep verification enabled even when Python assertions are optimized out."""
    if not condition:
        raise AuditError(message)


def timestamp_seconds(value):
    """Parse protocol nanoseconds on Python versions whose datetime accepts only microseconds."""
    normalized = re.sub(r"\.(\d+)(?=Z|[+-])", lambda match: "." + match[1][:6].ljust(6, "0"), value)
    timestamp = datetime.datetime.fromisoformat(normalized.replace("Z", "+00:00"))
    require(timestamp.tzinfo is not None, "event timestamp must include its timezone")
    return timestamp.timestamp()


def canonical_record(frame):
    """Retain JSON scalar types while allowing irrelevant object-key ordering differences."""
    return json.dumps(frame, sort_keys=True, separators=(",", ":"), allow_nan=False).encode() + b"\n"


class ReceiptLog:
    """One client's complete frames, in arrival order, under an explicit disk cap."""
    def __init__(self, path, max_bytes=MAX_RECEIPT_BYTES):
        self.path, self.max_bytes = path, max_bytes
        self.last_seq, self.bytes = 0, 0
        # When the last accepted frame was made (its `ts`, seconds), for the link_cut progress witness.
        self.last_ts = None
        self.output = path.open("x+b")
        path.chmod(0o600)

    def __enter__(self):
        return self

    def __exit__(self, *_error):
        self.output.close()

    def record(self, frame, event_id):
        """Accept exactly the next sequence; a refused frame never advances the cursor."""
        seq = frame.get("seq")
        require(type(seq) is int and seq == self.last_seq + 1,
                "receipt gap, duplicate or reorder after {}".format(self.last_seq))
        require(type(event_id) is int and event_id == seq, "SSE id/data sequence mismatch")
        encoded = canonical_record(frame)
        require(self.bytes + len(encoded) <= self.max_bytes, "receipt evidence exceeded its cap")
        self.output.write(encoded)
        self.bytes += len(encoded)
        self.last_seq = seq
        if isinstance(frame.get("ts"), str):
            self.last_ts = timestamp_seconds(frame["ts"])

    def verify(self, pages):
        """Compare independently read storage pages to the untouched ordered receipt file."""
        self.output.flush()
        self.output.seek(0)
        count = 0
        for page in pages:
            for stored in page:
                count += 1
                require(type(stored.get("seq")) is int and stored["seq"] == count,
                        "stored history has a gap, duplicate or reorder")
                line = self.output.readline()
                require(bool(line), "client missed a stored event")
                require(line == canonical_record(stored), "receipt payload differs from stored event")
        require(count > 0, "empty history cannot prove a soak")
        require(not self.output.read(1), "client has receipts missing from storage")
        require(count == self.last_seq, "receipt count differs from its cursor")
        return count


class RunAudit:
    """Account for every user turn and require successful progress after injected faults."""
    def __init__(self, scripted_tools=True):
        # The fake model's script calls only shell and read_file; a real model may use any registered tool.
        self.scripted_tools = scripted_tools
        self.messages, self.started, self.completed, self.failed = 0, 0, 0, 0
        self.active = None
        self.finished = set()
        self.step, self.attempts, self.retries = None, [], []
        self.last_completed_seq = 0
        self.faults = {}
        self.restart_windows = []
        # [start, end] of database faults: a model call inside one gets 503 store_unavailable (GW-E-020).
        self.database_windows = []
        # The step's attempts given back because the runtime's own link to the gateway was down (a gateway_link retry,
        # CORE-U-062), and every such wait in ms: they spend no attempt.
        self.given_back, self.link_waits = [], []
        self.retry_evidence = []
        self.kinds = {}
        self.pause_windows, self.tool_timeouts = [], []
        # Wall times at which this conversation's sandbox was killed; each allows one resume.
        self.kill_windows, self.used_kills, self.resumed = [], set(), 0
        # [start, end] of disk_full faults: a tool may fail for lack of space inside one, and nothing else.
        self.disk_windows = []
        self.completed_times = []
        # When each step completed: the first sign of progress after a full disk is freed (disk_recovery).
        self.step_times = []
        # Resume times per disk_full window, and every one of them.
        self.window_resumes, self.disk_resumes = {}, []
        self.tool_names, self.tool_started, self.tool_outcomes = {}, {}, {}

    def fault(self, kind, seq):
        """A successful run must complete after the latest occurrence of each fault kind."""
        require(kind in FAULT_KINDS, "unknown fault kind")
        self.faults[kind] = seq

    def record(self, frame):
        """Reject unexpected failures immediately; restart exceptions are narrowly bounded."""
        event = frame.get("event", {})
        kind = event.get("type", frame["type"])
        self.kinds[kind] = self.kinds.get(kind, 0) + 1
        if kind == "message.user":
            self.messages += 1
        elif kind == "run.started":
            identity = event["run_id"]
            require(self.active is None and identity not in self.finished, "overlapping or repeated run")
            self.active = identity
            self.started += 1
            self.step, self.attempts, self.retries, self.given_back = None, [], [], []
        elif kind == "step.started":
            require(frame.get("run_id") == self.active, "step belongs to an inactive run")
            if self.step != event["step"]:
                self.step, self.attempts, self.retries, self.given_back = event["step"], [], [], []
            self.attempts.append(event["attempt"])
        elif kind == "provider.retry":
            require(frame.get("run_id") == self.active, "retry belongs to an inactive run")
            if event.get("reason") == "gateway_link":
                self.given_back.append(event["attempt"])
                self.link_waits.append(event["delay_ms"])
            else:
                self.retries.append(event["attempt"])
        elif kind in ("run.completed", "run.failed", "run.cancelled"):
            self.terminal(frame)
        elif kind == "step.completed":
            self.step_times.append(timestamp_seconds(frame["ts"]))
        elif kind == "tool.call":
            require(not self.scripted_tools or event["name"] in ("shell", "read_file"), "unexpected scripted tool")
            require(len(self.tool_names) < 256, "tool audit exceeded its cap")
            self.tool_names[event["call_id"]] = event["name"]
        elif kind == "tool.start":
            self.tool_started[event["call_id"]] = timestamp_seconds(frame["ts"])
        elif kind == "tool.result":
            self.tool_result(frame)
        elif kind == "run.resumed":
            when = timestamp_seconds(frame["ts"])
            kills = [kill for kill in self.kill_windows if kill <= when <= kill + KILL_RECOVERY_SECONDS
                     and kill not in self.used_kills]
            disks = [window for window in self.disk_windows
                     if window[0] <= when <= window[1] + DISK_RESUME_MARGIN_SECONDS]
            if kills:
                self.used_kills.add(kills[0])
            elif disks:
                # On a full disk the gateway replaces a runtime whose storage failed and the run resumes, within the
                # product's bound: up to 4 per conversation per window, each after its backoff (the loop guard).
                earlier = self.window_resumes.setdefault(disks[0][0], [])
                require(len(earlier) < len(REPLACEMENT_BACKOFF_SECONDS),
                        "a {}th resume in one disk_full window: more replacements than the product's bound "
                        "({} in 10 minutes)".format(len(earlier) + 1, len(REPLACEMENT_BACKOFF_SECONDS)))
                if earlier:
                    backoff = REPLACEMENT_BACKOFF_SECONDS[len(earlier)]
                    require(when > earlier[-1] and when - earlier[-1] >= backoff - 1,
                            "a resume {:.0f} s after the last in one disk_full window, sooner than the product's "
                            "backoff of {} s".format(when - earlier[-1], backoff))
                earlier.append(when)
                self.disk_resumes.append(when)
            else:
                raise AuditError("the run resumed without a sandbox kill of its own (runtime replaced)")
            self.resumed += 1
        elif kind == "run.parked":
            raise AuditError("the soak unexpectedly parked an active run")

    def terminal(self, frame):
        event, identity = frame["event"], frame["event"]["run_id"]
        require(identity == self.active and identity not in self.finished,
                "unexpected or duplicate terminal run event")
        if event["type"] == "run.completed":
            self.completed += 1
            self.completed_times.append(timestamp_seconds(frame["ts"]))
            self.last_completed_seq = frame["seq"]
        else:
            require(self.allowed_restart_failure(frame), "unexpected run.failed or run.cancelled")
            self.failed += 1
            self.retry_evidence.append({"run_id": identity, "seq": frame["seq"], "ts": frame["ts"],
                                        "attempts": list(self.attempts), "retries": list(self.retries)})
        self.finished.add(identity)
        self.active = None

    def allowed_restart_failure(self, frame):
        event = frame["event"]
        if event["type"] != "run.failed" or event.get("code") != "provider_error":
            return False
        # Every attempt spent: numbers run 1, 2, 3, … with no gap, and those not given back are exactly MODEL_ATTEMPTS,
        # each but the last followed by its retry.
        spent = [attempt for attempt in self.attempts if attempt not in self.given_back]
        if (self.attempts != list(range(1, len(self.attempts) + 1)) or len(spent) != MODEL_ATTEMPTS
                or self.retries != spent[:-1]):
            return False
        timestamp = timestamp_seconds(frame["ts"])
        if any(start <= timestamp <= end for start, end in self.restart_windows):
            return True
        # GW-E-020: model calls during a database outage get 503 store_unavailable, and a run whose retries outlast the
        # outage ends run.failed with its transcript complete. That is the documented outcome, not a missed fault.
        return "store_unavailable" in event.get("reason", "") and any(
            start <= timestamp <= end for start, end in self.database_windows)

    def tool_result(self, frame):
        event = frame["event"]
        identity, outcome = event["call_id"], event["status"]
        name = self.tool_names[identity]
        if outcome == "interrupted":
            when, started = timestamp_seconds(frame["ts"]), self.tool_started[identity]
            if any(started < kill <= when <= kill + KILL_RECOVERY_SECONDS for kill in self.kill_windows):
                outcome = "interrupted_by_kill"
            else:
                # A sandbox lost on a full disk is replaced and its run resumes (the run.resumed the full-disk rule
                # allows); the tool in flight when the window began comes back interrupted, as after a kill.
                require(any(started <= end and start <= when <= end + DISK_RESUME_MARGIN_SECONDS
                            for start, end in self.disk_windows),
                        "tool interrupted outside its sandbox-kill or full-disk window")
                outcome = "interrupted_by_full_disk"
        elif outcome == "error" and NO_SPACE in event.get("content", "") and any(
                start <= timestamp_seconds(frame["ts"]) <= end + TOOL_RECOVERY_SECONDS
                for start, end in self.disk_windows):
            outcome = "no_space"
        elif outcome != "ok":
            when, started = timestamp_seconds(frame["ts"]), self.tool_started[identity]
            matching = [window for window in self.pause_windows if window["end"] is not None and
                        started < window["start"] <= when <= window["end"] + TOOL_RECOVERY_SECONDS]
            require(outcome == "error" and event["content"] == "timeout: tool deadline elapsed" and matching,
                    "tool error outside its in-flight pause window")
            outcome = "timeout_while_paused"
            self.tool_timeouts.append({"call_id": identity, "seq": frame["seq"], "ts": frame["ts"], "pause": matching[-1]})
        key = name + "." + outcome
        self.tool_outcomes[key] = self.tool_outcomes.get(key, 0) + 1

    def verify_workload(self):
        """A run of tool errors cannot stand in for the promised repeated workspace work."""
        require(all(self.tool_outcomes.get(name + ".ok", 0) > 0 for name in ("shell", "read_file")),
                "missing successful shell or read_file work")

    def needs_completion(self):
        return self.completed == 0 or any(seq >= self.last_completed_seq for seq in self.faults.values())

    def verify(self):
        """A terminal of the wrong kind, an unfinished turn or an idle-only soak cannot pass."""
        require(self.messages > 0, "no accepted user message")
        require(self.messages == self.started == self.completed + self.failed and self.active is None,
                "a message or run never reached exactly one terminal event")
        require(not self.needs_completion(), "missing successful completion after a fault")
