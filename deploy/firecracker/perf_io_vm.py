"""PERF-012's microVM half (TEST-PLAN: a Firecracker block image at >= 70 % of the host disk): the Docker half's
workload (deploy/tests/perf_io_bench.py) inside a task's guest, on its workspace image, against the host disk on the
filesystem that holds the stack's storage, where the workspace image lives.

A guest has no `docker exec`, so the guest side is the fixture model's scripted shell calls: one run writes the bench
onto the workspace image, then each round is four runs, one per phase (sequential write, random write, sequential read,
random read). Inside a guest an fsync and an eviction stop at the guest: the workspace image is a file on the host, read
and written through the host's page cache. So between the phases the driver flushes and evicts that image on the host
(fsync, then posix_fadvise DONTNEED, timed), charges each flush to the write before it, and the reads then come from the
disk, as on the host side. Both sides measure "data on the disk" whatever the drive's cache mode (finding 6, 6 Oct 2026). The host side is the same file run by
the same interpreter, the runtime image's python3 that the guest's rootfs is built from, in a container on a bind mount
of a host directory: PERF-012's Docker half shows a bind mount reads as the host disk. Rounds alternate which side goes
first. A round must finish within the shell tool's 30 s deadline; one that doesn't fails the run with its reason.

Run from the repository root, one stack at a time on the host (the variables are stack.py's):
    TQA_DOCKER_TESTS=1 TQA_VMM_IMAGE=<tqa-vmm image> TQA_FC_KERNEL_DIR=<kernel dir> python3 deploy/firecracker/perf_io_vm.py
"""
import argparse
import json
from pathlib import Path
import subprocess
import time

import stack
# stack.py puts deploy/tests on the path.
import perf_common  # noqa: E402

BENCH = stack.ROOT / "deploy" / "tests" / "perf_io_bench.py"
GUEST_BENCH = "/workspace/.perf_io_bench.py"
GUEST_DIRECTORY = "/workspace/perf-io"
HEREDOC = "PERF_IO_BENCH"
# The bench's phases, in order; the host acts between them.
PHASES = ("seq-write", "rand-write", "seq-read", "rand-read")
MEASURES = ("seq_write_mbps", "seq_read_mbps", "rand_write_ops", "rand_read_ops")
RATIO_MIN = 0.70
# Each guest round within the shell tool's 30 s deadline with room to spare: 64 MiB sequential, 4096 random 4 KiB.
DEFAULT_SIZE_MIB = 64
DEFAULT_RANDOM_OPS = 4096
RUN_SECONDS = 120
USAGE = {"prompt_tokens": 10, "completion_tokens": 10}


def shell(command):
    return {"events": [{"type": "tool_call", "name": "shell", "split_into": 1, "arguments": {"command": command}}],
            "usage": USAGE}


def answer(text):
    return {"events": [{"type": "text", "text": text}], "usage": USAGE}


def bench_arguments(directory, sizes, seed):
    return [directory, "--size-mib", str(sizes["size_mib"]), "--random-ops", str(sizes["random_ops"]),
            "--seed", str(seed)]


def scenario(rounds, sizes, source):
    """The fixture model's script: the bench written onto the workspace image, then one call per phase of each round."""
    if any(line.strip() == HEREDOC for line in source.splitlines()):
        raise ValueError("the bench holds the heredoc's end line")
    body = source if source.endswith("\n") else source + "\n"
    setup = "cat > {} <<'{}'\n{}{}".format(GUEST_BENCH, HEREDOC, body, HEREDOC)
    responses = [shell(setup), answer("The bench is in place.")]
    for index in range(rounds):
        for phase in PHASES:
            arguments = bench_arguments(GUEST_DIRECTORY, sizes, index) + ["--phase", phase]
            command = "mkdir -p {} && python3 {} {}".format(GUEST_DIRECTORY, GUEST_BENCH, " ".join(arguments))
            responses += [shell(command), answer("Round {} {} measured.".format(index, phase))]
    return {"model": "fixture", "responses": responses}


def flush_arguments(runtime, storage):
    """A container on the stack's storage that fsyncs and evicts the task's workspace image, timing it. Its root is the
    service account's namespaced root, which may open the gateway's files."""
    script = ("import glob, json, os, time\n"
              "images = glob.glob('/storage/workspaces/*.ext4')\n"
              "assert len(images) == 1, images\n"
              "descriptor = os.open(images[0], os.O_RDONLY)\n"
              "started = time.monotonic()\n"
              "os.fsync(descriptor)\n"
              "flushed = time.monotonic() - started\n"
              "os.posix_fadvise(descriptor, 0, 0, os.POSIX_FADV_DONTNEED)\n"
              "os.close(descriptor)\n"
              "print(json.dumps({'flush_seconds': flushed}))\n")
    return ["docker", "run", "--rm", "--network", "none", "--user", "0:0",
            "--mount", "type=bind,source={},target=/storage".format(storage),
            "--entrypoint", "python3", runtime, "-c", script]


def guest_sample(phases, flushes, sizes):
    """The guest's measures: each write charged the host flush after it, the reads as measured (from the disk)."""
    size, count = sizes["size_mib"] * 1024 * 1024, sizes["random_ops"]
    return {
        "seq_write_mbps": round(size / 1e6 / (phases["seq-write"]["seq_write_seconds"] + flushes["seq-write"]), 1),
        "rand_write_ops": round(count / (phases["rand-write"]["rand_write_seconds"] + flushes["rand-write"]), 1),
        "seq_read_mbps": phases["seq-read"]["seq_read_mbps"],
        "rand_read_ops": phases["rand-read"]["rand_read_ops"],
        "host_flush_seconds": {phase: round(seconds, 4) for phase, seconds in flushes.items()},
        "python": phases["rand-read"]["python"],
    }


def order(index):
    """Host first on even rounds, the guest first on odd ones."""
    return ("host", "guest") if index % 2 == 0 else ("guest", "host")


def ended_runs(stored):
    """How many runs have completed; a failed run fails the measurement with its code."""
    for frame in stored:
        event = frame.get("event", {})
        if event.get("type") == "run.failed":
            raise AssertionError("a guest round's run failed: {}".format(event.get("code", "unknown")))
    return sum(frame.get("event", {}).get("type") == "run.completed" for frame in stored)


def guest_answer(stored):
    """The bench's JSON from the last tool result: its last line that parses as an object."""
    results = [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]
    if not results:
        raise AssertionError("the guest round has no tool result")
    last = results[-1]
    if last.get("status") != "ok":
        raise AssertionError("the guest round failed: {}".format(str(last.get("content", ""))[:300]))
    for line in reversed(str(last.get("content", "")).splitlines()):
        try:
            parsed = json.loads(line)
        except ValueError:
            continue
        if isinstance(parsed, dict):
            return parsed
    raise AssertionError("the guest round printed no JSON: {}".format(str(last.get("content", ""))[:300]))


def setup_done(stored):
    """The setup run wrote the bench: its one call succeeded."""
    results = [frame["event"] for frame in stored if frame.get("event", {}).get("type") == "tool.result"]
    if len(results) != 1 or results[0].get("status") != "ok":
        raise AssertionError("the bench was not written into the guest: {}".format(
            [(result.get("status"), str(result.get("content", ""))[:200]) for result in results]))


def host_arguments(runtime, directory, bench, sizes, seed):
    """The host round: the runtime image's interpreter on a bind mount of `directory`, nothing else reachable."""
    return (["docker", "run", "--rm", "--network", "none", "--read-only", "--user", "1000:1000",
             "--mount", "type=bind,source={},target=/host".format(directory),
             "--mount", "type=bind,source={},target=/bench/perf_io_bench.py,readonly".format(bench),
             "--entrypoint", "python3", runtime, "/bench/perf_io_bench.py"] + bench_arguments("/host", sizes, seed))


def same_interpreter(rounds):
    """Refuses rounds whose two sides ran different interpreters, or did not say which."""
    versions = {(sample["host"].get("python"), sample["guest"].get("python")) for sample in rounds}
    for host, guest in versions:
        if host is None or host != guest:
            raise ValueError("host and guest ran different interpreters: {} and {}".format(host, guest))
    return next(iter(versions))[0]


def summarize(rounds):
    """p50 of each measure on each side, and the guest/host ratio of those p50s."""
    summary = {}
    for measure in MEASURES:
        host = perf_common.percentile([sample["host"][measure] for sample in rounds], 50)
        guest = perf_common.percentile([sample["guest"][measure] for sample in rounds], 50)
        summary.update({"host_" + measure: host, "guest_" + measure: guest,
                        measure.rsplit("_", 1)[0] + "_ratio": round(guest / host, 3)})
    return summary


def verdict(summary):
    return "pass" if all(value >= RATIO_MIN for name, value in summary.items() if name.endswith("_ratio")) else "fail"


class Transcript:
    """A conversation's stored events, read on from the last one seen: the whole run of rounds holds more than one
    replay page."""

    def __init__(self, demo, conversation):
        self.demo, self.conversation, self.stored = demo, conversation, []

    def refresh(self):
        after = self.stored[-1]["seq"] if self.stored else 0
        self.stored += stack.events(self.demo, self.conversation, after)
        return self.stored

    def wait_for_runs(self, runs, seconds=RUN_SECONDS):
        """The stored events once `runs` runs have completed (bounded)."""
        deadline = time.monotonic() + seconds
        while True:
            stored = self.refresh()
            if ended_runs(stored) >= runs:
                return stored
            if time.monotonic() >= deadline:
                raise AssertionError("run {} did not complete within {} s: {}".format(runs, seconds,
                                                                                     stack.timeline(stored)))
            time.sleep(0.5)


def measure(demo, options, sizes):
    """The rounds, each side once per round in alternating order, after the warm-ups."""
    env = demo.env
    runtime = stack.image_id(stack.compose.required_image(env, "TQA_RUNTIME_IMAGE"), env)
    host = demo.directory.parent / "perf-io-host"
    host.mkdir(mode=0o777)
    host.chmod(0o777)
    demo.sign_in()
    conversation = demo.request("/api/v1/conversations", {"title": "PERF-012", "model_route": "local"})["id"]
    path, transcript = "/api/v1/conversations/{}/messages".format(conversation), Transcript(demo, conversation)
    demo.request(path, {"text": "Set up the bench", "attachments": []})
    setup_done(transcript.wait_for_runs(1))
    storage, runs = demo.directory / "storage", 1

    def flush():
        output = subprocess.run(flush_arguments(runtime, storage), env=env, capture_output=True, text=True,
                                timeout=600, check=True).stdout
        return json.loads(output)["flush_seconds"]

    rounds = []
    for index in range(options.warmups + options.rounds):
        sample = {}
        for side in order(index):
            if side == "host":
                output = subprocess.run(host_arguments(runtime, host, BENCH, sizes, index), env=env,
                                        capture_output=True, text=True, timeout=600, check=True).stdout
                sample["host"] = json.loads(output)
                continue
            phases, flushes = {}, {}
            for phase in PHASES:
                demo.request(path, {"text": "Round {} {}".format(index, phase), "attachments": []})
                runs += 1
                phases[phase] = guest_answer(transcript.wait_for_runs(runs))
                if phase != "rand-read":
                    # After each write the flush is the rest of its trip to the disk; after the sequential read it
                    # only evicts what that read brought into the host's cache.
                    flushes[phase] = flush()
            flushes.pop("seq-read")
            sample["guest"] = guest_sample(phases, flushes, sizes)
        if index >= options.warmups:
            rounds.append(sample)
    return runtime, host, rounds


def check(options):
    sizes = {"size_mib": options.size_mib, "random_ops": options.random_ops}

    def run(demo):
        runtime, host, rounds = measure(demo, options, sizes)
        python = same_interpreter(rounds)
        summary = summarize(rounds)
        result = verdict(summary)
        record = perf_common.environment(demo.env, {"runtime": runtime, "vmm": demo.env["TQA_VMM_IMAGE"]})
        record["filesystem"] = subprocess.run(["findmnt", "-no", "SOURCE,FSTYPE", "-T", str(host)],
                                              capture_output=True, text=True, check=True).stdout.strip()
        path = perf_common.write("PERF-012", record, {
            "backend": "firecracker", "warmups": options.warmups, "rounds": options.rounds,
            "size_mib": options.size_mib, "random_ops": options.random_ops,
            "order": "alternating, host first on even rounds",
            "workspace": "the task's workspace image (ext4 on a block device) at /workspace; between the guest's phases "
                         "the image is fsynced and evicted on the host, the flush charged to the write before it",
            "drive_cache": "Writeback (workspace and state, SBX-F-039)",
            "host": "a bind mount on the filesystem of the stack's storage, in a container",
            "interpreter": "the runtime image's python {} on both sides".format(python)},
            rounds, summary, {"every_ratio_at_least": RATIO_MIN}, result)
        print("PERF-012 (microVM) {} {} -> {}".format(result, {key: value for key, value in summary.items()
                                                                if key.endswith("_ratio")}, path), flush=True)
        if result != "pass":
            raise AssertionError("PERF-012 (microVM) ratio below {}: {}".format(RATIO_MIN, summary))
    return run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--rounds", type=int, default=50)
    parser.add_argument("--size-mib", type=int, default=DEFAULT_SIZE_MIB)
    parser.add_argument("--random-ops", type=int, default=DEFAULT_RANDOM_OPS)
    options = parser.parse_args()
    sizes = {"size_mib": options.size_mib, "random_ops": options.random_ops}
    script = scenario(options.warmups + options.rounds, sizes, BENCH.read_text())
    stack.run("perf-012", script, check(options))


if __name__ == "__main__":
    main()
