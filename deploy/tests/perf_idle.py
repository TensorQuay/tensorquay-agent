"""PERF-021: idle streams. 16 conversations each run one short task, then sit idle with their runtime links open while
64 SSE clients (4 a conversation) stay connected, for one hour. Every 60 s the gateway's cgroup CPU, RSS, open file
descriptors and database connections are sampled; each client counts its keep-alives and bytes.
- Keep-alive cost: the gateway's CPU per minute (p50, p99) and per hour, and bytes per stream per minute, after a
  5-minute warm-up.
- No growth: SOAK-001's rule (soak_growth.judge_metric) scaled to the hour, the 5-minute warm-up left out: RSS and
  the fds other than DB connections under 5 % (the median of the 10 minutes after the warm-up against the last 10's,
  and the slope projected over an hour); the gateway's DB connections within the pool and not stuck at its maximum
  (its floor test needs a 12 h lifetime, so an hour doesn't judge it).
- Every stream still open and kept alive at the end; every runtime link the same session (the sandboxes and the
  gateway unchanged).
Run from the repository root, with no other stack running on the host (it holds the stack for about 70 minutes):
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_idle.py
"""
import re
import http.client
import os
import tempfile
import threading
import time
import urllib.parse
from pathlib import Path

import compose
import perf_common
from perf_concurrency import Cgroup, Client, sandbox_budget
from perf_start import StartStack, last_seq
from soak_stack import cleanup_actions
from soak_audit import AuditError, require
from soak_growth import LIMIT, judge_metric

__all__ = ["AuditError", "growth", "keepalive_cost", "link_faults", "stream_faults", "verdict"]

MINUTE = 60
DURATION = 3600
WARM_UP = 5 * MINUTE
CONVERSATIONS, CLIENTS_PER_CONVERSATION = 16, 4
KEEPALIVE = 15  # the gateway's default stream_keepalive, seconds
KEEPALIVE_SLACK = 10  # keep-alives a stream may be short over the hour, for timer drift
IDLE_SECONDS = DURATION + 30 * MINUTE  # the sandboxes' idle window outlasts the measurement
FINISH_WITHIN = 300
GROWTH_METRICS = ("gateway_rss_bytes", "open_fds", "database_connections")
GROWTH_RULE = {"warm_up": WARM_UP, "window": 10 * MINUTE, "min_lifetime": 50 * MINUTE, "horizon": DURATION}


def settled(samples):
    """The samples from the end of the warm-up on."""
    start = samples[0]["at"]
    return [sample for sample in samples if sample["at"] >= start + WARM_UP]


def cpu_seconds_per_minute(samples):
    """The gateway's CPU seconds in each interval between samples, scaled to one minute."""
    rates = []
    for before, after in zip(samples, samples[1:]):
        seconds = after["at"] - before["at"]
        require(seconds > 0, "CPU samples out of order")
        rates.append((after["usage_usec"] - before["usage_usec"]) / 1e6 / seconds * MINUTE)
    return rates


def keepalive_cost(samples, streams):
    """What keeping the streams and links alive costs the gateway, after the warm-up."""
    steady = settled(samples)
    require(len(steady) >= 11, "fewer than 10 minutes measured after the warm-up")
    per_minute = cpu_seconds_per_minute(steady)
    seconds = steady[-1]["at"] - steady[0]["at"]
    cpu_seconds = (steady[-1]["usage_usec"] - steady[0]["usage_usec"]) / 1e6
    bytes_per_minute = [item["bytes"] / item["seconds"] * MINUTE for item in streams if item["seconds"] > 0]
    require(bytes_per_minute, "no stream measured")
    return {"minutes": len(per_minute),
            "cpu_seconds_per_minute_p50": perf_common.percentile(per_minute, 50),
            "cpu_seconds_per_minute_p99": perf_common.percentile(per_minute, 99),
            "cpu_seconds_per_hour": cpu_seconds / seconds * 3600,
            "bytes_per_stream_per_minute_p50": perf_common.percentile(bytes_per_minute, 50),
            "bytes_per_stream_per_minute_max": max(bytes_per_minute)}


def stream_faults(streams, duration):
    """Each client stream that failed, closed early, went silent or missed keep-alives, in plain words. Events on an
    idle stream are recorded by the harness, not faults."""
    faults = []
    for number, item in enumerate(streams, 1):
        name = "stream {} (conversation {})".format(number, item["conversation"])
        if item["failure"]:
            faults.append("{} failed: {}".format(name, item["failure"]))
            continue
        if not item["open_at_end"]:
            faults.append("{} closed after {:.0f} s of {}".format(name, item["seconds"], duration))
        if item["longest_silence"] > 2 * KEEPALIVE:
            faults.append("{} was silent for {:.0f} s (keep-alive every {} s)".format(
                name, item["longest_silence"], KEEPALIVE))
        expected = int(item["seconds"] // KEEPALIVE) - KEEPALIVE_SLACK
        if item["keepalives"] < expected:
            faults.append("{} got {} keep-alives in {:.0f} s (expected at least {})".format(
                name, item["keepalives"], item["seconds"], expected))
    return faults


def link_faults(before, after):
    """The runtime links stayed up: the gateway never restarted and every conversation kept its sandbox."""
    faults = []
    if before["gateway_started"] != after["gateway_started"]:
        faults.append("the gateway restarted during the hour")
    for conversation, sandbox in sorted(before["sandboxes"].items()):
        now = after["sandboxes"].get(conversation)
        if now is None:
            faults.append("conversation {}'s sandbox is gone: its runtime link did not stay up".format(conversation))
        elif now != sandbox:
            faults.append("conversation {}'s sandbox changed ({} -> {}): its runtime link did not stay up".format(
                conversation, sandbox, now))
    return faults


def growth(samples):
    """M5's rule over the hour (GROWTH_RULE), each metric by its own (SOAK-001): RSS and the fds other than DB
    connections under 5 %; the gateway's connections within the pool, not stuck at it (the floor, a 12 h test, isn't
    judged on an hour). One lifetime, since a restart is already a link fault."""
    results = {metric: judge_metric(samples, metric, **GROWTH_RULE) for metric in GROWTH_METRICS}
    if any(result is None for result in results.values()):
        return {"passed": False, "metrics": results,
                "reason": "growth not judged: under {} minutes sampled".format(GROWTH_RULE["min_lifetime"] // MINUTE)}
    over = [metric for metric, result in results.items() if not result["passed"]]
    return {"passed": not over, "metrics": results,
            "reason": "over its limit in the hour: {}".format(", ".join(over)) if over
            else "every metric within its limit in the hour"}


def verdict(growth_result, faults_in_streams, faults_in_links):
    """Pass with no stream or link fault and no growth; otherwise every reason, streams first."""
    reasons = list(faults_in_streams) + list(faults_in_links)
    if not growth_result["passed"]:
        reasons.append(growth_result["reason"])
    return ("fail" if reasons else "pass"), reasons


def idle_settings(text):
    """gateway.toml admitting every conversation at once, with an idle window longer than the run."""
    text = sandbox_budget(text, CONVERSATIONS)
    require(text.count("[sandbox]\n") == 1 and text.count("idle_seconds=") <= 1, "unexpected [sandbox] section")
    # The start stack writes the returning task's short window (perf_start); the hour's idle streams need this one.
    text = re.sub(r"(?m)^idle_seconds=\d+\n", "", text)
    return text.replace("[sandbox]\n", "[sandbox]\nidle_seconds={}\n".format(IDLE_SECONDS), 1)


class IdleStack(StartStack):
    """The soak stack's installation with 16 sandboxes admitted and kept through the hour."""
    def configure(self, rehearsal):
        super().configure(rehearsal)
        config = self.directory / "gateway.toml"
        compose.write(config, idle_settings(config.read_text()))


class IdleClient(threading.Thread):
    """One idle SSE client from the conversation's last event: counts keep-alive comments, bytes, data records (the
    stream's session.ready among them) and the longest silence, raw, since the stack's SSE decoder drops comments."""
    def __init__(self, stack, conversation, after):
        super().__init__(daemon=True)
        self.stack, self.conversation, self.after = stack, conversation, after
        self.stopping, self.connected = threading.Event(), threading.Event()
        self.keepalives = self.bytes = self.events = 0
        self.longest_silence, self.seconds, self.open_at_end, self.failure = 0.0, 0.0, False, None

    def run(self):
        address = urllib.parse.urlsplit(self.stack.origin)
        connection = http.client.HTTPConnection(address.hostname, address.port, timeout=4 * KEEPALIVE)
        started = time.monotonic()
        try:
            connection.request("GET", "/api/v1/conversations/{}/events?stream=1&protocol=1".format(self.conversation),
                               headers={"Cookie": self.stack.cookie, "Origin": self.stack.origin,
                                        "Accept": "text/event-stream", "Last-Event-ID": str(self.after)})
            response = connection.getresponse()
            require(response.status == 200, "the event stream was refused ({})".format(response.status))
            self.connected.set()
            last = time.monotonic()
            while not self.stopping.is_set():
                line = response.readline(65_537)
                now = time.monotonic()
                require(bool(line), "the stream ended")
                self.longest_silence = max(self.longest_silence, now - last)
                last, self.bytes = now, self.bytes + len(line)
                if line.startswith(b":"):
                    self.keepalives += 1
                elif line.startswith(b"data:"):
                    self.events += 1
            self.open_at_end = True
        except BaseException as error:
            self.failure = str(error) or type(error).__name__
        finally:
            self.seconds = time.monotonic() - started
            connection.close()

    def record(self):
        return {"conversation": self.conversation, "seconds": round(self.seconds, 1), "keepalives": self.keepalives,
                "bytes": self.bytes, "longest_silence": round(self.longest_silence, 1),
                "open_at_end": self.open_at_end, "events": self.events, "failure": self.failure}


def running_sandbox(stack, conversation):
    """The conversation's running sandbox container (12 characters), or None: a lost one is a fault, not an error."""
    label = "label=tqa.instance=" + stack.env["TQA_SANDBOX_NETWORK"]
    identity = stack.command(["docker", "ps", "-q", "--filter", label, "--filter",
                              "label=tqa.conversation=" + conversation, "--filter", "status=running"])
    return identity.split()[0][:12] if identity else None


def links(stack, conversations):
    """What must not change over the hour: the gateway container's start and each conversation's sandbox."""
    sandboxes = {conversation: running_sandbox(stack, conversation) for conversation in conversations}
    return {"gateway_started": stack.inspect(stack.gateway)["State"]["StartedAt"],
            "sandboxes": {conversation: sandbox for conversation, sandbox in sandboxes.items() if sandbox}}


def settle(stack):
    """16 conversations, each with one finished task, so each has a live sandbox and runtime link."""
    conversations = [stack.request("/api/v1/conversations", {"title": "perf-021 {}".format(index),
                                                              "model_route": "local"})["id"]
                     for index in range(CONVERSATIONS)]
    clients = [Client(stack, conversation) for conversation in conversations]
    for client in clients:
        client.start()
    require(all(client.connected.wait(15) for client in clients), "a stream did not open")
    for index, conversation in enumerate(conversations):
        stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "perf-021 {}".format(index)})
    deadline = time.monotonic() + FINISH_WITHIN
    for client in clients:
        client.join(max(0.0, deadline - time.monotonic()))
        require(client.failure is None and client.last == "run.completed",
                "conversation {}'s task did not finish: {}".format(client.conversation, client.failure))
    return conversations


def measure(stack, cgroup, conversations):
    """The idle hour: 64 clients connected, one gateway sample a minute."""
    after = {conversation: last_seq([item for page in stack.stored(conversation) for item in page])
             for conversation in conversations}
    clients = [IdleClient(stack, conversation, after[conversation])
               for conversation in conversations for _ in range(CLIENTS_PER_CONVERSATION)]
    for client in clients:
        client.start()
    require(all(client.connected.wait(15) for client in clients), "an idle stream did not open")
    before = links(stack, conversations)
    require(len(before["sandboxes"]) == CONVERSATIONS, "a conversation has no running sandbox before the hour")
    samples, started = [], time.monotonic()
    while True:
        samples.append({**stack.resources(), **cgroup.sample()})  # the cgroup's monotonic `at` wins
        if time.monotonic() - started >= DURATION:
            break
        time.sleep(max(0.0, started + len(samples) * MINUTE - time.monotonic()))
    after_links = links(stack, conversations)
    for client in clients:
        client.stopping.set()
    for client in clients:
        client.join(2 * KEEPALIVE + 5)
    return samples, [client.record() for client in clients], before, after_links


def main():
    compose.teardown_on_signals()  # a SIGTERM (gate9_run's time limit, a stop) tears the stack down like Ctrl-C
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("PERF-021 needs Docker: set TQA_DOCKER_TESTS=1 and run it with no other stack running")
    env = os.environ.copy()
    perf_common.keep_readings(env, "PERF-021")
    temporary = tempfile.TemporaryDirectory(prefix="tqa-perf-021-")
    directory = Path(temporary.name) / "installation"
    directory.mkdir(mode=0o700)
    stack, failure = IdleStack(directory, env, perf_common.log_directory("PERF-021")), None
    try:
        stack.prepare(rehearsal=True)
        record = perf_common.environment(env, stack.images)
        cgroup = Cgroup(stack.inspect(stack.gateway)["State"]["Pid"])
        conversations = settle(stack)
        samples, streams, before, after = measure(stack, cgroup, conversations)
        cost = keepalive_cost(samples, streams)
        grew = growth(samples)
        result, reasons = verdict(grew, stream_faults(streams, DURATION), link_faults(before, after))
        path = perf_common.write("PERF-021", record, {
            "conversations": CONVERSATIONS, "clients": CONVERSATIONS * CLIENTS_PER_CONVERSATION,
            "seconds": DURATION, "sample_every_seconds": MINUTE, "warm_up_seconds": WARM_UP,
            "keepalive_seconds": KEEPALIVE, "idle_seconds": IDLE_SECONDS, "growth_rule": GROWTH_RULE,
            "deviation": "one 1 h run, not 50 iterations: the row is a duration measurement"},
            {"gateway": samples, "streams": streams, "links_before": before, "links_after": after},
            dict(cost, growth=grew, events_on_idle_streams=sum(item["events"] for item in streams)),
            {"growth_below": LIMIT, "streams_open": CONVERSATIONS * CLIENTS_PER_CONVERSATION}, result)
        print("PERF-021 {} cpu {:.2f} s/h (p50 {:.3f} s/min) bytes/stream/min {:.0f} -> {}".format(
            result, cost["cpu_seconds_per_hour"], cost["cpu_seconds_per_minute_p50"],
            cost["bytes_per_stream_per_minute_p50"], path), flush=True)
        if result != "pass":
            failure = AssertionError("PERF-021 missed its target: " + "; ".join(reasons))
    except BaseException as error:
        failure = error
    finally:
        failure = cleanup_actions([stack.save_logs, stack.cleanup_helpers], failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
    if failure is not None:
        raise SystemExit("PERF-021 failed: {}".format(failure))


if __name__ == "__main__":
    main()
