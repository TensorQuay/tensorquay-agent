"""SOAK-002: independently owned SSE readers with complete-record receipt cursors."""
import http.client
import json
import queue
import socket
import threading
import time
import urllib.parse

from soak_audit import REPLACEMENT_BACKOFF_SECONDS, REPLACEMENT_WINDOW_SECONDS, AuditError, require

MAX_RECORD_BYTES = 256 * 1024


def invalid_number(_value):
    """Nonfinite JavaScript extensions are not JSON protocol values."""
    raise AuditError("invalid nonfinite SSE JSON number")


class SseDecoder:
    """Deliver complete JSON records only; partial records never advance a cursor."""
    def __init__(self, max_bytes=MAX_RECORD_BYTES):
        self.max_bytes = max_bytes
        self.data, self.identity, self.bytes = [], None, 0

    def line(self, line):
        self.bytes += len(line)
        require(self.bytes <= self.max_bytes, "SSE record exceeded its cap")
        require(line.endswith(b"\n"), "partial SSE line")
        line = line.rstrip(b"\r\n")
        if not line:
            value = None
            if self.data:
                try:
                    frame = json.loads(b"\n".join(self.data), parse_constant=invalid_number)
                except (ValueError, UnicodeError) as error:
                    raise AuditError("invalid SSE JSON") from error
                require(isinstance(frame, dict), "SSE frame must be an object")
                value = self.identity, frame
            else:
                require(self.identity is None, "SSE id without data")
            self.data, self.identity, self.bytes = [], None, 0
            return value
        if line.startswith(b":"):
            return None
        field, _, value = line.partition(b":")
        if value.startswith(b" "):
            value = value[1:]
        if field == b"data":
            self.data.append(value)
        elif field == b"id":
            require(value.isdigit() and len(value) <= 20, "invalid SSE id")
            require(self.identity is None, "repeated SSE id")
            self.identity = int(value)
        else:
            require(field == b"event", "unexpected SSE field")
        return None

    def finish(self):
        require(not self.data and self.identity is None, "partial SSE record at EOF")


# After the disk is freed, a conversation that met the full disk must complete a run within this bound (SOAK-001).
DISK_RECOVERY_SECONDS = 180
# What a full disk may tell a stream, inside its window and margin: the runtime's storage failed, or its replacement
# could not be created. Whether the soak passes is decided by the recovery, not by which of these arrived.
DISK_NOTICES = ("io", "sandbox_failed")


def expected_notice(frame, when, windows, database_windows=(), disk_windows=()):
    """Only an announced gateway restart may end a stream with a control error, only a database fault may report the
    store unavailable, and only a full disk may report `io` (the runtime's storage I/O failed), within the window and
    its recovery margin."""
    code = frame.get("code")
    if code == "restarting":
        return any(start <= when <= end for start, end in windows)
    if code in DISK_NOTICES:
        return any(start <= when <= end + DISK_RECOVERY_SECONDS for start, end in disk_windows)
    return code == "store_unavailable" and any(start <= when <= end for start, end in database_windows)


# A connection that began this shortly before a failed notice may have missed the sandbox_failed sent just before it.
FRESH_CONNECTION_SECONDS = 2


# A replacement whose create failed is retried on the gateway's CREATE_RETRIES (5, 20, 65, 155, 335, 515, 600 s from
# the first failure), and each retry's countdown re-announces the same loss as destroyed (manager/recovery.rs,
# try_replacing). Its countdown never exceeds the largest gap between two retries.
CREATE_RETRY_GAP_SECONDS = 180
# A full disk's notice may reach a stream a moment before the driver records the window's start.
FULL_DISK_LEAD_SECONDS = 5


class RecoveryNotices:
    """The product's bound on replacing a sandbox that keeps stopping, as one client sees it: a loss is announced as
    destroyed with the delay before its replacement (0, 5, 15, 45 s for the 1st to 4th), the replacement as creating
    no sooner, at most 4 in a rolling 10 minutes, and a 5th loss as a retryable sandbox_failed before the failed
    state. Notices without a recovery (a first start, a connection's snapshot) are not losses."""

    def __init__(self, disk_windows=()):
        # The driver's full-disk windows (shared, so a window added later is seen): a failed create, and so its retry
        # countdown, only comes from a full disk in the soak.
        self.disk_windows = disk_windows
        self.announced = {}
        self.creating = []
        # When each replacement was announced, for the whole run (the driver forgets a runtime replaced on a full disk).
        self.replaced_at = []
        self.connected_at = None
        self.failed_notice = False
        self.replacements = 0

    def in_full_disk(self, when):
        """Within a full disk's window, from just before it starts (the fault and its notice race) to its recovery
        margin after it ends."""
        return any(start - FULL_DISK_LEAD_SECONDS <= when <= end + DISK_RECOVERY_SECONDS
                   for start, end in self.disk_windows)

    def connected(self, when):
        self.connected_at, self.announced, self.failed_notice = when, {}, False

    def observe(self, when, frame):
        if frame.get("type") == "error":
            if frame.get("code") == "sandbox_failed":
                require(frame.get("retryable") is True, "a sandbox_failed notice that is not retryable")
                self.failed_notice = True
            return
        sandbox = frame.get("sandbox") or {}
        recovery = sandbox.get("recovery")
        if frame.get("type") != "session.ready" or not recovery:
            return
        state, losses = sandbox.get("state"), recovery.get("losses")
        require(isinstance(losses, int) and losses >= 1, "a recovery notice without its loss count")
        if state == "destroyed":
            delay = recovery.get("retry_in_secs")
            on_schedule = (losses <= len(REPLACEMENT_BACKOFF_SECONDS) and isinstance(delay, int)
                           and 0 <= delay <= REPLACEMENT_BACKOFF_SECONDS[losses - 1])
            if not on_schedule and self.in_full_disk(when):
                # The replacement's create failed on the full disk: this is the create retry's countdown, whether or
                # not the loss was announced on this stream before (soaks 6 and 7, 8 Oct 2026).
                require(isinstance(delay, int) and 0 <= delay <= CREATE_RETRY_GAP_SECONDS,
                        "loss {}'s create retry announced with a delay of {} s, past the product's create retries"
                        .format(losses, delay))
            else:
                require(on_schedule,
                        "loss {} announced with a delay of {} s, outside the product's schedule".format(losses, delay))
            self.announced[losses] = (when, delay)
        elif state == "creating":
            require(losses <= len(REPLACEMENT_BACKOFF_SECONDS),
                    "a replacement for the {}th loss: past the product's bound".format(losses))
            if losses in self.announced:
                since, delay = self.announced.pop(losses)
                require(when - since >= delay - 1, "loss {} replaced {:.0f} s after it was announced, sooner than its "
                        "announced {} s".format(losses, when - since, delay))
            self.creating = [t for t in self.creating if when - t < REPLACEMENT_WINDOW_SECONDS] + [when]
            require(len(self.creating) <= len(REPLACEMENT_BACKOFF_SECONDS),
                    "more than {} replacements in 10 minutes".format(len(REPLACEMENT_BACKOFF_SECONDS)))
            self.replacements += 1
            self.replaced_at.append(when)
        elif state == "failed":
            fresh = self.connected_at is not None and when - self.connected_at <= FRESH_CONNECTION_SECONDS
            require(self.failed_notice or fresh, "a failed sandbox after {} losses without a sandbox_failed notice"
                    .format(losses))


class Refused(Exception):
    """A reconnect the gateway refused with a typed error the soak expects (a restart or a database window): not a
    disconnect. The client waits for `Retry-After` (1 to 5 s) and tries again."""

    def __init__(self, status, retry_after):
        super().__init__("gateway answered {} during a planned reconnect".format(status))
        self.wait = self.wait_seconds(retry_after)

    @staticmethod
    def wait_seconds(retry_after):
        try:
            return min(5, max(1, int(retry_after)))
        except (TypeError, ValueError):
            return 1


def error_code(response):
    """The `code` of the gateway's JSON error body ({"error": {"code": …}}), or None."""
    try:
        return json.loads(response.read(64 * 1024))["error"]["code"]
    except (ValueError, KeyError, TypeError, OSError, http.client.HTTPException):
        return None


class SseClient:
    """One reader, one receipt file, bounded reconnects, and explicit stop plus join."""
    def __init__(self, address, receipts, events, windows, database_windows=(), disk_windows=(), session=None):
        self.origin, self.path, self.cookie, self.name = address
        # The stack's session (soak_stack.Stack: `cookie`, `renew`): each connect reads its cookie, and a reconnect
        # refused with 401 after the session ended signs in again. Without one, `cookie` is fixed and a 401 fails.
        self.session, self.renewed = session, None
        self.receipts, self.events, self.windows = receipts, events, windows
        self.database_windows, self.disk_windows = database_windows, disk_windows
        # When this client got `io` notices: each conversation that did must prove it recovers.
        self.io_times = []
        # When this client got store_unavailable outside every database fault: accepted as load on a busy host, and
        # the driver requires that conversation to recover and caps how often it happens.
        self.store_times = []
        self.recovery = RecoveryNotices(disk_windows)
        self.stopping = threading.Event()
        self.ready = threading.Event()
        self.thread = threading.Thread(target=self.run, name=self.name)
        self.connection, self.failure = None, None
        self.connections, self.snapshots, self.notices = 0, 0, 0
        # Unsequenced control frames of kinds this client doesn't know, by kind (counted, not failed).
        self.control_frames = {}
        self.offline_until, self.reconnect_until = 0.0, time.monotonic() + 60

    def start(self):
        self.thread.start()

    def interrupt(self):
        connection = self.connection
        if connection is not None and connection.sock is not None:
            try:
                connection.sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass  # The reader may already have observed the peer's close.

    def drop(self, offline_seconds=3):
        require(0 < offline_seconds <= 10, "client outage is out of bounds")
        self.offline_until = time.monotonic() + offline_seconds
        self.reconnect_until = self.offline_until + 60
        self.interrupt()

    def stop(self):
        self.stopping.set()
        self.interrupt()

    def join(self):
        self.thread.join(timeout=25)
        require(not self.thread.is_alive(), "SSE reader did not stop")
        if self.failure is not None:
            raise self.failure

    def recovering(self):
        if any(start <= time.time() <= end for start, end in self.windows):
            self.reconnect_until = time.monotonic() + 60
        return time.monotonic() <= self.reconnect_until

    def database_down(self):
        """Inside a database fault's window: a reconnect gets a typed error until the database is back."""
        return any(start <= time.time() <= end for start, end in self.database_windows)

    def run(self):
        try:
            while not self.stopping.is_set():
                remaining = self.offline_until - time.monotonic()
                if remaining > 0:
                    self.stopping.wait(min(remaining, 0.25))
                    continue
                try:
                    self.read_connection()
                except Refused as refused:
                    self.stopping.wait(refused.wait)
                    continue
                except (OSError, http.client.HTTPException) as error:
                    if not self.stopping.is_set():
                        require(self.recovering(), "unexpected SSE disconnect: " + str(error))
                finally:
                    if self.connection is not None:
                        self.connection.close()
                        self.connection = None
                self.stopping.wait(0.25)
        except BaseException as error:
            self.failure = error

    def read_connection(self):
        parsed = urllib.parse.urlsplit(self.origin)
        require(parsed.scheme == "http", "owned soak endpoint must use HTTP")
        self.connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=20)
        cookie = self.cookie if self.session is None else self.session.cookie
        self.connection.request("GET", self.path, headers={
            "Cookie": cookie, "Origin": self.origin, "Accept": "text/event-stream",
            "Last-Event-ID": str(self.receipts.last_seq)})
        if self.stopping.is_set():
            return
        response = self.connection.getresponse()
        if response.status == 401 and self.session is not None:
            # The session ended (it lasts session_ttl from sign-in, 24 h by default): sign in again, then reconnect as
            # after a refusal. Refused again before any stream opened on the new session is a failure, not an end.
            require(self.renewed != cookie, "SSE refused a fresh session (401)")
            self.renewed = self.session.renew(cookie)
            raise Refused(response.status, None)
        if response.status != 200:
            # Inside a database fault's window a reconnect gets a typed error until the database is back, however long
            # since the drop: 502/503, or 504 timeout when a database call hangs to its bound (only in that window).
            # Streams already open stay open through it (GW-I-007), so a disconnect is not excused there.
            expected = (response.status in (502, 503) and (self.recovering() or self.database_down())
                        or response.status == 504 and self.database_down() and error_code(response) == "timeout")
            require(expected, "unexpected SSE HTTP status {}".format(response.status))
            raise Refused(response.status, response.getheader("Retry-After"))
        require(response.getheader("Content-Type", "").startswith("text/event-stream"), "not an SSE response")
        self.connections += 1
        self.renewed = None
        self.recovery.connected(time.time())
        self.reconnect_until = 0.0
        decoder = SseDecoder()
        while not self.stopping.is_set():
            line = response.readline(MAX_RECORD_BYTES + 1)
            if self.stopping.is_set():
                return
            if not line:
                if self.recovering():
                    return  # An interrupted partial record is replayed from the last whole receipt.
                decoder.finish()
                raise AuditError("unexpected SSE EOF")
            if not line.endswith(b"\n") and self.recovering():
                return
            value = decoder.line(line)
            if value is not None:
                self.receive(*value)

    def receive(self, identity, frame):
        kind = frame.get("type")
        if kind == "error":
            require(identity is None and frame.get("seq") is None, "sequenced error frame")
            now = time.time()
            if frame.get("code") == "store_unavailable" and not any(
                    start <= now <= end for start, end in self.database_windows):
                self.store_times.append(now)
            else:
                require(expected_notice(frame, now, self.windows, self.database_windows, self.disk_windows),
                        "unexpected client error frame on client {}: {} ({})".format(
                        self.name, frame.get("code"), str(frame.get("message", ""))[:200]))
            self.notices += 1
            if frame.get("code") in DISK_NOTICES:
                self.io_times.append(time.time())
            self.recovery.observe(time.time(), frame)
            self.reconnect_until = time.monotonic() + 60
        elif kind == "session.ready":
            require(identity is None and "seq" not in frame, "sequenced readiness frame")
            self.snapshots += 1
            self.recovery.observe(time.time(), frame)
            self.ready.set()
        elif (isinstance(kind, str) and kind and kind != "event" and identity is None
              and frame.get("seq") is None):
            # An unsequenced control frame this client doesn't know, such as message.queue (a message held while a run
            # finishes): counted in the result, never part of the record.
            self.control_frames[kind] = self.control_frames.get(kind, 0) + 1
        else:
            # Revision 9 has no approval frames (ADR-0035): a sequenced frame other than an event is unknown.
            require(kind == "event", "unknown server frame")
            self.receipts.record(frame, identity)
            if self.events is not None:
                try:
                    self.events.put_nowait((self.name, frame))
                except queue.Full as error:
                    raise AuditError("soak event queue exceeded its cap") from error
