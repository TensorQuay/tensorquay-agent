"""The database and load readings beside a test stack: every few seconds while the stack lives, the host's load and
pressure, and a summary of the stack's own Postgres; when a session is blocked or idle in a transaction, its rows and
locks too. A hold like 16z's PERF-011 (a row held 50 s, nothing naming the holder) then leaves evidence.

`compose.prepare` starts a reader for its installation. `compose.retire` tells it to stop as it begins, so teardown
isn't read as an anomaly, and at its end waits for it and keeps the file: in `TQA_STACK_READINGS_DIR` when that is set,
and after any failure in a folder of its own whose path it prints, whatever the stack's harness. `compose.save_logs`
also copies it beside a failed run's logs. A reader never fails a run: what it can't read is noted in its file. Content-free: names, ids, modes and ages; a statement's text only for
the gateway's own sessions (whose values are bound parameters), and every error redacted as the stack's logs are.

The reader's own psql names itself (`application_name` tqa-stack-readings), so anything that counts the stack's
backends can leave it out: the soak's observer does (`soak_chaos._GATEWAY`), and soak stacks start no reader at all.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

APPLICATION = "tqa-stack-readings"
GATEWAY_APPLICATION = "tqa-gateway"
INTERVAL_SECONDS = 5.0
MINIMUM_INTERVAL_SECONDS = 2.0
BEAT_EVERY = 6  # a beat line every sixth poll: 30 s at the default interval
ALERT_CAP = 2000
POLL_TIMEOUT = 4  # each docker call's own bound: a stalled dockerd hangs the client, which Postgres's bounds can't end
# Inside the container each psql ends sooner, so a poll cut at POLL_TIMEOUT leaves no query running behind it: the
# server ends its statement and its lock wait, the client gives up connecting, and `timeout` ends the client itself.
CONTAINER_SECONDS = 3
PG_OPTIONS = "-c statement_timeout=3000 -c lock_timeout=1000"
STOP_TIMEOUT = 3 * POLL_TIMEOUT + 1  # one whole poll: a container lookup and two psql calls
# What makes a poll an alert.
BLOCKED_SECONDS = 1
IDLE_IN_TRANSACTION_SECONDS = 2
OLDEST_TRANSACTION_SECONDS = 5.0  # half the gateway's default I/O bound
IO_FULL_PERCENT = 10.0
CPU_SOME_PERCENT = 50.0
FILE = "readings.jsonl"
SUMMARY = "readings-summary.txt"
SEPARATOR = "\x1f"
CONTAINER = re.compile(r"[0-9a-f]{12,64}")

_OURS = ("datname = current_database() AND pid <> pg_backend_pid() AND application_name <> '" + APPLICATION + "'")
# One row: client backends, active, idle in a transaction, waiting on a lock, the oldest transaction's age, the longest
# idle transaction's age, sessions blocked for BLOCKED_SECONDS or more, idle in a transaction for the alert's seconds.
SUMMARY_QUERY = (
    "SELECT count(*) FILTER (WHERE backend_type = 'client backend'), "
    "count(*) FILTER (WHERE state = 'active'), "
    "count(*) FILTER (WHERE state LIKE 'idle in transaction%'), "
    "count(*) FILTER (WHERE wait_event_type = 'Lock'), "
    "coalesce(round(max(extract(epoch FROM clock_timestamp() - xact_start))::numeric, 1), 0), "
    "coalesce(round(max(extract(epoch FROM clock_timestamp() - state_change)) "
    "FILTER (WHERE state LIKE 'idle in transaction%')::numeric, 1), 0), "
    "count(*) FILTER (WHERE cardinality(pg_blocking_pids(pid)) > 0 "
    "AND clock_timestamp() - query_start >= interval '" + str(BLOCKED_SECONDS) + " s'), "
    "count(*) FILTER (WHERE state LIKE 'idle in transaction%' "
    "AND clock_timestamp() - state_change >= interval '" + str(IDLE_IN_TRANSACTION_SECONDS) + " s') "
    "FROM pg_stat_activity WHERE " + _OURS)
# The sessions worth keeping: blocked, idle in a transaction for the alert's seconds, or blocking one of those.
_PIDS = ("SELECT pid FROM pg_stat_activity WHERE " + _OURS + " AND ((cardinality(pg_blocking_pids(pid)) > 0 "
         "AND clock_timestamp() - query_start >= interval '" + str(BLOCKED_SECONDS) + " s') "
         "OR (state LIKE 'idle in transaction%' AND clock_timestamp() - state_change >= interval '"
         + str(IDLE_IN_TRANSACTION_SECONDS) + " s')) "
         "UNION SELECT unnest(pg_blocking_pids(pid)) FROM pg_stat_activity WHERE " + _OURS)
SESSIONS_QUERY = (
    "SELECT 'S', pid, coalesce(state, ''), application_name, "
    "coalesce(round(extract(epoch FROM clock_timestamp() - xact_start)::numeric, 1)::text, ''), "
    "coalesce(round(extract(epoch FROM clock_timestamp() - state_change)::numeric, 1)::text, ''), "
    "coalesce(wait_event_type || ':' || wait_event, ''), pg_blocking_pids(pid)::text, "
    "CASE WHEN application_name = '" + GATEWAY_APPLICATION + "' "
    "THEN left(regexp_replace(coalesce(query, ''), '\\s+', ' ', 'g'), 160) ELSE '' END "
    "FROM pg_stat_activity WHERE pid IN (" + _PIDS + ")")
LOCKS_QUERY = (
    "SELECT 'L', l.pid, l.locktype, l.mode, l.granted, coalesce(l.relation::regclass::text, ''), "
    "coalesce(l.tuple::text, ''), coalesce(l.transactionid::text, ''), coalesce(l.objid::text, '') "
    "FROM pg_locks l WHERE l.pid IN (" + _PIDS + ") ORDER BY l.pid, l.granted")

_READERS = {}
_READERS_LOCK = threading.Lock()


def now_text(clock=time.time):
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(clock()))


def number(text, kind=float):
    try:
        return kind(text)
    except (TypeError, ValueError):
        return None


def host(proc=Path("/proc")):
    """The 1-minute load and each pressure file's `some` and `full` avg10; what the kernel doesn't report is None."""
    reading = {"load1": None}
    try:
        reading["load1"] = number((proc / "loadavg").read_text().split()[0])
    except (OSError, IndexError):
        pass
    for resource in ("cpu", "io", "memory"):
        values = {}
        try:
            for line in (proc / "pressure" / resource).read_text().splitlines():
                parts = line.split()
                if parts and parts[0] in ("some", "full"):
                    field = next((part for part in parts[1:] if part.startswith("avg10=")), "")
                    values[parts[0]] = number(field.partition("=")[2])
        except OSError:
            pass
        reading[resource] = {"some": values.get("some"), "full": values.get("full")}
    return reading


def database_summary(text):
    """SUMMARY_QUERY's one row, or None when it isn't one."""
    parts = text.strip().split(SEPARATOR)
    if len(parts) != 8:
        return None
    counts = [number(part, int) for part in parts[:4]] + [number(part, int) for part in parts[6:]]
    ages = [number(part) for part in parts[4:6]]
    if any(value is None for value in counts + ages):
        return None
    return {"backends": counts[0], "active": counts[1], "idle_in_transaction": counts[2], "lock_waiters": counts[3],
            "oldest_transaction_seconds": ages[0], "longest_idle_transaction_seconds": ages[1],
            "blocked": counts[4], "idle_in_transaction_alerts": counts[5]}


def sessions_and_locks(text):
    """SESSIONS_QUERY's and LOCKS_QUERY's rows, told apart by their first field; malformed rows are counted."""
    sessions, locks, malformed = [], [], 0
    for line in text.splitlines():
        parts = line.split(SEPARATOR)
        if parts[0] == "S" and len(parts) == 9 and number(parts[1], int) is not None:
            blockers = [number(value, int) for value in parts[7].strip("{}").split(",") if value]
            sessions.append({"pid": int(parts[1]), "state": parts[2], "application": parts[3],
                             "transaction_seconds": number(parts[4]), "state_seconds": number(parts[5]),
                             "wait": parts[6], "blocked_by": [value for value in blockers if value is not None],
                             # The query keeps text only for the gateway's sessions; the reader checks again.
                             "statement": (parts[8] or None) if parts[3] == GATEWAY_APPLICATION else None})
        elif parts[0] == "L" and len(parts) == 9 and number(parts[1], int) is not None:
            locks.append({"pid": int(parts[1]), "type": parts[2], "mode": parts[3], "granted": parts[4] == "t",
                          "relation": parts[5] or None, "tuple": number(parts[6], int),
                          "transaction": parts[7] or None, "object": number(parts[8], int)})
        elif line.strip():
            malformed += 1
    return sessions, locks, malformed


def reasons(reading, database):
    """Why a poll is an alert: an empty list when it isn't one."""
    why = []
    if database:
        if database["blocked"]:
            why.append("blocked")
        if database["idle_in_transaction_alerts"]:
            why.append("idle_in_transaction")
        if database["oldest_transaction_seconds"] >= OLDEST_TRANSACTION_SECONDS:
            why.append("old_transaction")
    if (reading["io"]["full"] or 0) >= IO_FULL_PERCENT:
        why.append("io_pressure")
    if (reading["cpu"]["some"] or 0) >= CPU_SOME_PERCENT:
        why.append("cpu_pressure")
    return why


class Readings:
    """One stack's reader. `poll()` takes one reading; `start()` polls on a daemon thread until `stop()`."""

    def __init__(self, env, path, *, interval=INTERVAL_SECONDS, proc=Path("/proc"), run=subprocess.run,
                 clock=time.time):
        # A copy: the harness may change its own env while this thread hands it to subprocess.
        self.env, self.path, self.interval = dict(env), Path(path), interval
        self.proc, self.run, self.clock = proc, run, clock
        self.project = env["COMPOSE_PROJECT_NAME"]
        self.polls = self.alerts = self.errors = 0
        self.capped = False
        self.container = None
        self.peaks = {}
        self.longest_block = None
        self.longest_idle = None
        self.stopping = threading.Event()
        self.thread = None
        self.lock = threading.Lock()
        self.closed = False  # after stop: nothing more is written, so the removed folder isn't made again
        self.answered = False  # Postgres has answered once: until then a failed poll is its start, not a fault
        self.waiting_noted = False

    def redact(self, text):
        for name in ("TQA_POSTGRES_PASSWORD", "TQA_BOOTSTRAP_TOKEN", "TQA_STACK_PASSWORD"):
            if self.env.get(name):
                text = text.replace(self.env[name], "[REDACTED]")
        return text

    def write(self, line):
        line = dict({"t": now_text(self.clock)}, **line)
        with self.lock:
            if self.closed:
                return
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with open(self.path, "a", encoding="utf-8") as out:
                out.write(json.dumps(line, sort_keys=True) + "\n")

    def note(self, text):
        self.write({"kind": "note", "note": self.redact(text)[:500]})

    def docker(self, args):
        return self.run(["docker"] + args, env=self.env, check=True, timeout=POLL_TIMEOUT, text=True,
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout

    def find_container(self):
        listed = self.docker(["ps", "-q", "--filter", "label=com.docker.compose.project=" + self.project,
                              "--filter", "label=com.docker.compose.service=postgres"]).split()
        found = next((value for value in listed if CONTAINER.fullmatch(value)), None)
        if found and found != self.container:
            self.note("the stack's Postgres container: " + found[:12])
        self.container = found
        return found

    def psql(self, *queries):
        args = ["exec", "-e", "PGAPPNAME=" + APPLICATION, "-e", "PGOPTIONS=" + PG_OPTIONS,
                "-e", "PGCONNECT_TIMEOUT=" + str(CONTAINER_SECONDS), self.container,
                "timeout", str(CONTAINER_SECONDS), "psql", "-U", "tqa", "-d", "gateway", "-XAtq", "-F", SEPARATOR]
        for query in queries:
            args += ["-c", query]
        return self.docker(args)

    def database(self):
        """The summary and, when it calls for them, the sessions and locks; (None, [], []) while no Postgres is up."""
        if not (self.container or self.find_container()):
            return None, [], []
        try:
            summary = database_summary(self.psql(SUMMARY_QUERY))
        except (OSError, subprocess.SubprocessError):
            # A database restart or a replaced container: look for it again next time.
            self.container = None
            raise
        if summary is None:
            raise ValueError("an unreadable summary row")
        self.answered = True
        # Told to stop (teardown has begun): no second call into a stack that is going.
        if not (summary["blocked"] or summary["idle_in_transaction_alerts"]) or self.stopping.is_set():
            return summary, [], []
        sessions, locks, malformed = sessions_and_locks(self.psql(SESSIONS_QUERY, LOCKS_QUERY))
        if malformed:
            self.note("{} unreadable session or lock rows".format(malformed))
        return summary, sessions, locks

    def track(self, reading, database, sessions, locks):
        stamp = now_text(self.clock)
        for key, value in (("load1", reading["load1"]), ("io_full", reading["io"]["full"]),
                           ("cpu_some", reading["cpu"]["some"])):
            if value is not None and value > self.peaks.get(key, (-1, ""))[0]:
                self.peaks[key] = (value, stamp)
        by_pid = {session["pid"]: session for session in sessions}
        for session in sessions:
            if session["blocked_by"] and (self.longest_block is None
                                          or (session["state_seconds"] or 0) > self.longest_block["seconds"]):
                # A row wait's ungranted lock is on the holder's transaction id, which names no relation: the row
                # is named by the tuple lock the waiter took first (seen on Postgres 18.3, 22 Oct).
                mine = [lock for lock in locks if lock["pid"] == session["pid"] and lock["relation"]]
                waiting = next((lock["relation"] for lock in mine if not lock["granted"]),
                               next((lock["relation"] for lock in mine if lock["type"] == "tuple"), None))
                holder = by_pid.get(session["blocked_by"][0], {})
                self.longest_block = {"seconds": session["state_seconds"] or 0, "pid": session["pid"], "at": stamp,
                                      "relation": waiting, "holder": holder.get("pid", session["blocked_by"][0]),
                                      "holder_state": holder.get("state"),
                                      "holder_transaction_seconds": holder.get("transaction_seconds"),
                                      "holder_application": holder.get("application")}
        if database and database["longest_idle_transaction_seconds"] > (self.longest_idle or (0, ""))[0]:
            self.longest_idle = (database["longest_idle_transaction_seconds"], stamp)

    def poll(self):
        """One reading: written as a beat every BEAT_EVERY polls, and as an alert whenever it crosses a threshold."""
        reading = host(self.proc)
        database, sessions, locks, why = None, [], [], []
        try:
            database, sessions, locks = self.database()
        except (OSError, subprocess.SubprocessError, ValueError) as error:
            detail = str(getattr(error, "stderr", None) or error).strip()
            if self.stopping.is_set():
                self.note("a reading during teardown failed (not a fault): " + detail)
            elif not self.answered:
                # The container runs before Postgres accepts connections: its start isn't a fault.
                if not self.waiting_noted:
                    self.waiting_noted = True
                    self.note("waiting for the stack's Postgres to answer: " + detail)
            else:
                self.errors += 1
                why.append("poll_error")
                self.note("the database reading failed: " + detail)
        why = reasons(reading, database) + why
        self.track(reading, database, sessions, locks)
        line = {"host": reading, "db": database}
        if why:
            if self.alerts < ALERT_CAP:
                self.alerts += 1
                self.write(dict(line, kind="alert", why=why, sessions=sessions, locks=locks))
            elif not self.capped:
                self.capped = True
                self.note("{} alert lines written: beats only from here".format(ALERT_CAP))
        if self.polls % BEAT_EVERY == 0:
            self.write(dict(line, kind="beat"))
        self.polls += 1

    def loop(self):
        while not self.stopping.is_set():
            started = time.monotonic()
            try:
                self.poll()
            except Exception as error:  # noqa: BLE001 (a reader never fails a run)
                self.errors += 1
                try:
                    self.note("the reader's poll failed: " + repr(error))
                except OSError:
                    pass
            self.stopping.wait(max(0.0, self.interval - (time.monotonic() - started)))

    def start(self):
        self.note("start: every {} s, project {}".format(self.interval, self.project))
        self.thread = threading.Thread(target=self.loop, name="tqa-stack-readings", daemon=True)
        self.thread.start()

    def stop(self):
        self.stopping.set()
        if self.thread is not None:
            self.thread.join(STOP_TIMEOUT)
        running = self.thread is not None and self.thread.is_alive()
        self.note("stop: {} polls, {} alert lines, {} errors{}".format(
            self.polls, self.alerts, self.errors, "; the last poll was still running" if running else ""))
        with self.lock:
            self.closed = True

    def summary(self):
        """The readings in a few lines: peaks with their times, the longest block with its holder, the longest idle
        transaction, and the poll errors."""
        def peak(key, unit):
            value = self.peaks.get(key)
            return "{}{} at {}".format(value[0], unit, value[1]) if value else "not read"
        lines = ["peak load {}; io full {}; cpu some {}".format(peak("load1", ""), peak("io_full", " %"),
                                                               peak("cpu_some", " %"))]
        block = self.longest_block
        if block:
            holder = "holder pid {} ({}, {}, transaction {} s)".format(
                block["holder"], block["holder_state"] or "state unknown", block["holder_application"] or "unnamed",
                block["holder_transaction_seconds"] if block["holder_transaction_seconds"] is not None else "?")
            lines.append("longest block: pid {} waited {} s on {} at {}; {}".format(
                block["pid"], block["seconds"], block["relation"] or "a lock", block["at"], holder))
        else:
            lines.append("no session was blocked for {} s or more".format(BLOCKED_SECONDS))
        if self.longest_idle:
            lines.append("longest idle transaction: {} s at {}".format(*self.longest_idle))
        lines.append("{} polls, {} alert lines, {} poll errors".format(self.polls, self.alerts, self.errors))
        return lines

    def write_summary(self, folder):
        folder = Path(folder)
        folder.mkdir(parents=True, exist_ok=True)
        path = folder / SUMMARY
        path.write_text("\n".join(self.summary()) + "\n", encoding="utf-8")
        return path


def interval(env):
    value = number(env.get("TQA_STACK_READINGS_INTERVAL", INTERVAL_SECONDS))
    return max(MINIMUM_INTERVAL_SECONDS, value) if value else INTERVAL_SECONDS


def start(env):
    """`compose.prepare`'s hook: a reader for this installation, unless switched off (`TQA_STACK_READINGS=off`, as soak
    stacks are) or outside a Docker run (`TQA_DOCKER_TESTS` isn't 1: a unit test's fake installation). Its file lives in
    a folder of its own, not the installation's: `prepare` hands that one to the gateway's user, so this user can't
    write there. `stop` removes the folder once the file is copied out."""
    if env.get("TQA_STACK_READINGS") == "off" or env.get("TQA_DOCKER_TESTS") != "1":
        return None
    project = env.get("COMPOSE_PROJECT_NAME")
    if not project:
        return None
    folder = None
    try:
        folder = Path(tempfile.mkdtemp(prefix="tqa-stack-readings-"))
        reader = Readings(env, folder / FILE, interval=interval(env))
        reader.start()
    except Exception as error:  # noqa: BLE001 (a full /tmp or a thread that won't start must not fail the stack)
        if folder is not None:
            shutil.rmtree(folder, ignore_errors=True)
        detail = error.strerror if isinstance(error, OSError) and error.strerror else str(error)
        for name in ("TQA_POSTGRES_PASSWORD", "TQA_BOOTSTRAP_TOKEN", "TQA_STACK_PASSWORD"):
            if env.get(name):
                detail = detail.replace(env[name], "[REDACTED]")
        print("No stack readings for this run: the reader couldn't start ({})".format(detail), file=sys.stderr)
        return None
    with _READERS_LOCK:
        _READERS[project] = reader
    return reader


def reader_of(env):
    with _READERS_LOCK:
        return _READERS.get(env.get("COMPOSE_PROJECT_NAME"))


def keep(env, folder):
    """`compose.save_logs`' hook: the readings so far and their summary, copied into `folder`. Never raises."""
    reader = reader_of(env)
    if reader is None:
        return []
    written = []
    try:
        folder = Path(folder)
        if reader.path.exists():
            folder.mkdir(parents=True, exist_ok=True)
            with reader.lock:
                shutil.copyfile(reader.path, folder / FILE)
            written.append(folder / FILE)
        written.append(reader.write_summary(folder))
    except OSError as error:
        print("Could not keep the stack's readings: " + reader.redact(str(error)), file=sys.stderr)
    return written


def stopping(env):
    """The start of `compose.retire`: tells the reader to stop after its current poll, without waiting, so the teardown
    that follows isn't read as an anomaly and never waits on the reader."""
    reader = reader_of(env)
    if reader is not None:
        reader.stopping.set()
    return reader


def stop(env, failure=None):
    """The end of `compose.retire`: waits for the reader (one poll at most), prints its summary's lines and keeps the
    file and the summary: in `TQA_STACK_READINGS_DIR` when that is set, and after a failure without it in a folder of
    their own that outlives the run (its path printed), whatever the harness. Never raises."""
    with _READERS_LOCK:
        reader = _READERS.pop(env.get("COMPOSE_PROJECT_NAME"), None)
    if reader is None:
        return None
    try:
        reader.stop()
        for line in reader.summary():
            print("Stack readings: " + line, file=sys.stderr)
        target = env.get("TQA_STACK_READINGS_DIR")
        if target:
            target = Path(target)
        elif failure is not None:
            target = Path(tempfile.mkdtemp(prefix="tqa-stack-readings-kept-"))
        if target:
            target.mkdir(parents=True, exist_ok=True)
            if reader.path.exists():
                shutil.copyfile(reader.path, target / (reader.project + "-" + FILE))
            (target / (reader.project + "-" + SUMMARY)).write_text("\n".join(reader.summary()) + "\n",
                                                                   encoding="utf-8")
            print("Kept the stack's readings: " + str(target), file=sys.stderr)
    except OSError as error:
        print("Could not keep the stack's readings: " + reader.redact(str(error)), file=sys.stderr)
    if reader.path.parent.name.startswith("tqa-stack-readings-") and not \
            reader.path.parent.name.startswith("tqa-stack-readings-kept-"):
        shutil.rmtree(reader.path.parent, ignore_errors=True)
    return reader
