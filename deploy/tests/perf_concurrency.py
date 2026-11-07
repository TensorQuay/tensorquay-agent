"""PERF-020 (PRD M4): 16 live conversations on a gateway capped at 2 vCPU / 4 GB, with the model fixture streaming
every run at once. No client may miss an event; the gateway's CPU stays under 70 % of its 2 CPUs at the p95 of
1-second samples (the max and mean are reported), and its RSS under 1.5 GB. Three whole-load runs; the worst decides.
Run from the repository root, with no other stack running on the host:
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_concurrency.py
"""
import argparse
import json
import os
import re
import tempfile
import threading
import time
from pathlib import Path

import compose
import perf_common
from perf_start import StartStack, event_type, stream
from soak_audit import AuditError, require
from soak_stack import cleanup_actions

CONVERSATIONS, STEPS, DELAY_MS, RUNS, FINISH_WITHIN = 16, 20, 10, 3, 600
CPUS, CPU_P95_MAX, RSS_MAX = 2, 70.0, 1_500_000_000


def scenario(steps=STEPS, delay_ms=DELAY_MS):
    """Each run is `steps` model steps: six text deltas each, and a shell call on every step but the last."""
    replies = []
    for step in range(steps):
        events = [{"type": "text", "text": "Step {} part {}.\n".format(step, part)} for part in range(6)]
        if step < steps - 1:
            events.append({"type": "tool_call", "name": "shell", "split_into": 2,
                           "arguments": {"command": "printf perf-{} > perf.txt; cat perf.txt".format(step)}})
        replies.append({"events": events, "delay_ms": delay_ms, "usage": {"prompt_tokens": 100, "completion_tokens": 20}})
    return {"model": "fixture", "responses": replies}


def sandbox_budget(text, conversations):
    """gateway.toml with its one `max_live` raised to admit every conversation at once, never lowered: the stack writes
    its own budget (the soak's rule, twice its slots), so whatever number is there is taken as it is."""
    lines = re.findall(r"^max_live=(\d+)\n", text, flags=re.MULTILINE)
    require(len(lines) == 1, "the stack's gateway.toml has {} max_live lines, not one".format(len(lines)))
    budget = max(int(lines[0]), conversations)
    return re.sub(r"^max_live=\d+\n", "max_live={}\n".format(budget), text, count=1, flags=re.MULTILINE)


def queued_with_room(before, max_live, queued):
    """How many messages were queued although admission had room: the i-th message (0-based, in sending order) finds
    at most `before + i` sandbox rows, so below `max_live` its task was admitted by count, and a `queued` notice on
    its stream means the slot table was full where the rows said it wasn't (PERF-020's 429 on c16, which the gateway
    now answers 202 queued). `queued` holds one flag per message, in sending order."""
    return sum(1 for index, flag in enumerate(queued) if flag and before + index < max_live)


def gaps(sequence):
    """The first few faults in one client's receipts, which must read 1, 2, 3, ... with nothing missing or repeated."""
    return ["position {} has seq {}".format(index + 1, seq) for index, seq in enumerate(sequence)
            if seq != index + 1][:5]


def cpu_percent(before, after):
    """Percent of the gateway's CPU allocation used between two (monotonic seconds, usage_usec) readings."""
    seconds = after[0] - before[0]
    require(seconds > 0, "CPU readings out of order")
    return (after[1] - before[1]) / 1e6 / seconds / CPUS * 100


class Cgroup:
    """The gateway container's own cgroup v2 counters: exact, and without docker stats' averaging."""
    def __init__(self, pid):
        lines = Path("/proc/{}/cgroup".format(pid)).read_text().splitlines()
        path = next(line.split("::", 1)[1] for line in lines if line.startswith("0::"))
        self.root = Path("/sys/fs/cgroup") / path.lstrip("/")

    def read(self, name):
        return (self.root / name).read_text()

    def stat(self, name, key):
        return next(int(line.split()[1]) for line in self.read(name).splitlines() if line.split()[0] == key)

    def sample(self):
        return {"at": time.monotonic(), "usage_usec": self.stat("cpu.stat", "usage_usec"),
                "memory_current": int(self.read("memory.current")), "anon": self.stat("memory.stat", "anon")}


class Sampler(threading.Thread):
    """Samples the gateway's cgroup every second until stopped."""
    def __init__(self, cgroup):
        super().__init__(daemon=True)
        self.cgroup, self.stopping, self.samples, self.failure = cgroup, threading.Event(), [], None

    def run(self):
        try:
            self.samples.append(self.cgroup.sample())
            while not self.stopping.wait(1):
                self.samples.append(self.cgroup.sample())
        except BaseException as error:
            self.failure = error

    def summary(self):
        require(self.failure is None and len(self.samples) >= 3, "too few gateway samples")
        cpu = [cpu_percent((before["at"], before["usage_usec"]), (after["at"], after["usage_usec"]))
               for before, after in zip(self.samples, self.samples[1:])]
        return {"cpu_p95_percent": round(perf_common.percentile(cpu, 95), 1), "cpu_max_percent": round(max(cpu), 1),
                "cpu_mean_percent": round(sum(cpu) / len(cpu), 1),
                "rss_peak_bytes": max(sample["anon"] for sample in self.samples),
                "memory_current_peak_bytes": max(sample["memory_current"] for sample in self.samples),
                "seconds": len(cpu)}, [round(value, 1) for value in cpu]


class Client(threading.Thread):
    """One conversation's live SSE client: records every sequenced frame through the run's completion."""
    def __init__(self, stack, conversation):
        super().__init__(daemon=True)
        self.stack, self.conversation = stack, conversation
        self.connected, self.seqs, self.last, self.failure = threading.Event(), [], None, None
        self.queued = False  # a `queued` notice: the task waited for admission

    def run(self):
        try:
            for identity, frame in stream(self.stack, self.conversation, FINISH_WITHIN):
                if frame.get("type") == "session.ready":
                    self.queued = self.queued or (frame.get("sandbox") or {}).get("state") == "queued"
                    self.connected.set()
                    continue
                if frame.get("type") == "error":
                    # A control error ends the stream; keep its code and message so a failure says why.
                    require(False, "error frame {} {}".format(frame.get("code"), str(frame.get("message", ""))[:200]))
                require(frame.get("type") == "event", "unexpected frame {}".format(frame.get("type")))
                require(identity == frame.get("seq"), "SSE id/data sequence mismatch")
                self.seqs.append(frame["seq"])
                self.last = event_type(frame)
                event = frame.get("event", {})
                require(self.last != "run.failed", "run {} failed: {} {}".format(
                    event.get("run_id"), event.get("code"), str(event.get("message", ""))[:300]))
                if self.last == "run.completed":
                    return
        except BaseException as error:
            self.failure = error


class ConcurrencyStack(StartStack):
    """The demo stack with the gateway at 2 vCPU / 4 GB and 16 live sandboxes admitted."""
    def __init__(self, directory, env, output):
        super().__init__(directory, env, output)
        self.configuration = self.configuration + ["-f", "deploy/compose/perf.yml"]
        self.relay_capacity, self.steps, self.delay_ms = None, STEPS, DELAY_MS

    def configure(self, rehearsal):
        compose.write(self.directory / "scenario.json", json.dumps(scenario(self.steps, self.delay_ms)))
        compose.write(self.directory.parent / "soak.json", json.dumps({"services": {"fake-model": {
            "image": self.images["fake-model"], "command": ["--scenario", "/scenarios/soak.json"],
            "volumes": [{"type": "bind", "source": str(self.directory / "scenario.json"),
                         "target": "/scenarios/soak.json", "read_only": True}]}}}))
        config = self.directory / "gateway.toml"
        relay = "" if self.relay_capacity is None else "[relay]\ncapacity={}\n".format(self.relay_capacity)
        budgeted = sandbox_budget(config.read_text(), CONVERSATIONS)
        self.max_live = int(re.search(r"^max_live=(\d+)\n", budgeted, flags=re.MULTILINE).group(1))
        compose.write(config, budgeted + "\n" + relay)

    def sandbox_rows(self):
        """The sandbox rows admission counts, read from the database."""
        return int(self.command(["docker", "exec", self.database, "psql", "-U", "tqa", "-d", "gateway", "-At", "-c",
                                 "SELECT count(*) FROM sandbox_instances"]))


def verify(stack, client):
    """Contiguous receipts from 1 through run.completed, each of them stored, and nothing stored the client lacks."""
    require(client.failure is None, "conversation {} after {} events: {}".format(
        client.conversation, len(client.seqs), client.failure))
    require(client.last == "run.completed", "the run did not complete")
    require(not gaps(client.seqs), "dropped or repeated events: {}".format(gaps(client.seqs)))
    stored = [item["seq"] for page in stack.stored(client.conversation) for item in page]
    require(stored == list(range(1, len(stored) + 1)), "stored history has a gap")
    require(len([seq for seq in stored if seq <= client.seqs[-1]]) == len(client.seqs),
            "the client's count differs from the stored count")
    return len(client.seqs), len(stored) - len(client.seqs)


def one_run(stack, cgroup):
    conversations = [stack.request("/api/v1/conversations", {"title": "perf-020 {}".format(index),
                                                              "model_route": "local"})["id"]
                     for index in range(CONVERSATIONS)]
    clients = [Client(stack, conversation) for conversation in conversations]
    for client in clients:
        client.start()
    require(all(client.connected.wait(15) for client in clients), "a stream did not open")
    before = stack.sandbox_rows()
    sampler = Sampler(cgroup)
    sampler.start()
    started = time.monotonic()
    for index, conversation in enumerate(conversations):
        stack.request("/api/v1/conversations/{}/messages".format(conversation), {"text": "perf-020 {}".format(index)})
    for client in clients:
        client.join(max(0.0, started + FINISH_WITHIN - time.monotonic()))
    elapsed = time.monotonic() - started
    sampler.stopping.set()
    sampler.join(5)
    counts = [verify(stack, client) for client in clients]
    summary, cpu = sampler.summary()
    for conversation in conversations:
        stack.delete(conversation)
    deadline = time.monotonic() + 120
    while stack.command(["docker", "ps", "-aq", "--filter", "label=tqa.instance=" + stack.env["TQA_SANDBOX_NETWORK"]]):
        require(time.monotonic() < deadline, "sandboxes were not removed after delete")
        time.sleep(0.5)
    summary.update(events=sum(count for count, _ in counts), events_after_completion=sum(extra for _, extra in counts),
                   dropped=0, run_seconds=round(elapsed, 1), sandbox_rows_before=before,
                   queued=sum(client.queued for client in clients),
                   queued_with_room=queued_with_room(before, stack.max_live, [client.queued for client in clients]))
    return summary, {"cpu_percent": cpu, "events_per_conversation": [count for count, _ in counts]}


def verdict(summaries):
    """The worst run decides; dropped events already failed the run in verify()."""
    worst = {"cpu_p95_percent": max(run["cpu_p95_percent"] for run in summaries),
             "cpu_max_percent": max(run["cpu_max_percent"] for run in summaries),
             "cpu_mean_percent": max(run["cpu_mean_percent"] for run in summaries),
             "rss_peak_bytes": max(run["rss_peak_bytes"] for run in summaries),
             "events_min": min(run["events"] for run in summaries),
             "queued_with_room": sum(run.get("queued_with_room", 0) for run in summaries)}
    passed = worst["cpu_p95_percent"] < CPU_P95_MAX and worst["rss_peak_bytes"] < RSS_MAX
    return worst, "pass" if passed else "fail"


def main():
    compose.teardown_on_signals()  # a SIGTERM (gate9_run's time limit, a stop) tears the stack down like Ctrl-C
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("PERF-020 needs Docker: set TQA_DOCKER_TESTS=1 and run it with no other stack running")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--relay-capacity", type=int, help="the [relay] per-user burst; default: the product's")
    parser.add_argument("--steps", type=int, default=STEPS, help="a recorded variant: model steps per run")
    parser.add_argument("--delay-ms", type=int, default=DELAY_MS, help="a recorded variant: model delay per frame")
    options = parser.parse_args()
    env = os.environ.copy()
    perf_common.keep_readings(env, "PERF-020")
    temporary = tempfile.TemporaryDirectory(prefix="tqa-perf-020-")
    directory = Path(temporary.name) / "installation"
    directory.mkdir(mode=0o700)
    stack, failure = ConcurrencyStack(directory, env, perf_common.log_directory("PERF-020")), None
    stack.relay_capacity, stack.steps, stack.delay_ms = options.relay_capacity, options.steps, options.delay_ms
    try:
        stack.prepare(rehearsal=True)
        record = perf_common.environment(env, stack.images)
        gateway = stack.inspect(stack.gateway)
        cgroup = Cgroup(gateway["State"]["Pid"])
        limits = {"cpu_max": cgroup.read("cpu.max").strip(), "memory_max": cgroup.read("memory.max").strip()}
        require(limits["cpu_max"] == "200000 100000", "the gateway is not capped at 2 CPUs: " + limits["cpu_max"])
        require(limits["memory_max"] == str(4 * 1024 ** 3), "the gateway is not capped at 4 GB")
        runs = [one_run(stack, cgroup) for _ in range(RUNS)]
        worst, result = verdict([summary for summary, _ in runs])
        path = perf_common.write("PERF-020", record, {
            "conversations": CONVERSATIONS, "steps": options.steps, "model_delay_ms": options.delay_ms,
            "variant": "default" if (options.steps, options.delay_ms) == (STEPS, DELAY_MS) else "longer",
            "relay_capacity": options.relay_capacity, "runs": RUNS, "cgroup": limits,
            "deviation": "3 whole-load runs, not 50 iterations (approved); CPU is the p95 of 1 s cgroup samples",
            "rss": "memory.stat anon of the gateway cgroup; memory.current is reported alongside"},
            [samples for _, samples in runs], dict(worst, runs=[summary for summary, _ in runs]),
            {"dropped": 0, "cpu_p95_percent_below": CPU_P95_MAX, "rss_bytes_below": RSS_MAX}, result)
        print("PERF-020 {} cpu p95={}% max={}% mean={}% rss={:.0f} MB events>={} queued-with-room={} -> {}".format(
            result, worst["cpu_p95_percent"], worst["cpu_max_percent"], worst["cpu_mean_percent"],
            worst["rss_peak_bytes"] / 1e6, worst["events_min"], worst["queued_with_room"], path), flush=True)
        if result != "pass":
            failure = AssertionError("PERF-020 missed its target: {}".format(worst))
    except BaseException as error:
        failure = error
    finally:
        failure = cleanup_actions([stack.save_logs], failure)
        failure = compose.retire(env, failure, configuration=stack.configuration)
        compose.cleanup(directory, env, temporary, failure, compose=stack.configuration)
    if failure is not None:
        raise SystemExit("PERF-020 failed: {}".format(failure))


if __name__ == "__main__":
    main()
