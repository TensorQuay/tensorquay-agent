"""PERF-012: workspace I/O inside a Docker sandbox against the host, on the same filesystem. The sandbox is a container
of the runtime image with the Docker backend's own options (crates/tqa-sandbox/src/docker/config.rs) and the
gateway's default sandbox budget; each round runs perf_io_bench.py on both sides, alternating which goes first.
Target: every sandbox/host ratio of the p50s >= 0.80. Rounds are 64 MiB, each write phase ending in fsync, so the
numbers are durable throughput rather than page-cache speed, at about 7 GB written in all instead of 28 GB on an
NVMe that stalls under long sustained writes; --size-mib 256 is the full size, for when the numbers look off. Run from
the repository root, with no other stack running on the host:
    TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_io.py
"""
import argparse
import json
import os
import secrets
import subprocess
import sys
import sysconfig
import tempfile
from pathlib import Path

import compose
import demo
import perf_common

BENCH = Path(__file__).resolve().with_name("perf_io_bench.py")
MEASURES = ("seq_write_mbps", "seq_read_mbps", "rand_write_ops", "rand_read_ops")
RATIO_MIN = 0.80
# Both sides run this host's interpreter, so the ratios measure the mount and not two Pythons (26 Sep: the image's
# 3.13 against the host's 3.12 put every ratio above 1). The image's newer glibc runs the host binary; the reverse
# does not. It reaches the sandbox read-only, outside /workspace.
HOST_PYTHON = os.path.realpath(sys.executable)
HOST_STDLIB = sysconfig.get_path("stdlib")
SANDBOX_HOME = "/opt/hostpy"
SANDBOX_PYTHON = SANDBOX_HOME + "/bin/python3"
# The Docker backend's HostConfig for a sandbox, with the gateway's default [sandbox] budget (1 CPU, 512 MiB, 256 pids).
SANDBOX_OPTIONS = ["--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
                   "--security-opt", "seccomp=builtin", "--tmpfs", "/tmp:rw,nosuid,nodev,noexec,size=16777216,mode=1777",
                   "--pids-limit", "256", "--memory", "512m", "--memory-swap", "512m", "--cpu-period", "1000000",
                   "--cpu-quota", "1000000", "--network", "none", "--ipc", "private", "--user", "1000:1000",
                   "--workdir", "/workspace"]


def sandbox_arguments(name, image, workspace, cpu=None):
    """The sandbox container; `cpu` pins it to one logical CPU, as `host_command` pins the host side."""
    pin = [] if cpu is None else ["--cpuset-cpus", str(cpu)]
    return (["docker", "run", "-d", "--rm", "--name", name, "--label", "tqa.perf=perf-012"] + SANDBOX_OPTIONS + pin
            + ["--mount", "type=bind,source={},target=/workspace".format(workspace),
               "--mount", "type=bind,source={},target=/bench/perf_io_bench.py,readonly".format(BENCH),
               "--mount", "type=bind,source={},target={},readonly".format(HOST_PYTHON, SANDBOX_PYTHON),
               "--mount", "type=bind,source={},target={}/lib/{},readonly".format(
                   HOST_STDLIB, SANDBOX_HOME, os.path.basename(HOST_STDLIB)),
               "--entrypoint", "sleep", image, "infinity"])


# 4 KiB operations per random measure: enough that each lasts seconds, not the sub-second (noise-dominated) 4096.
DEFAULT_RANDOM_OPS = 16384
SYSFS_DEVICES = Path("/sys/devices")


def cpu_list(text):
    """The kernel's CPU list format (`0-7`, `0-1,4,8-9`) as numbers."""
    cpus = []
    for part in text.strip().split(","):
        if not part:
            continue
        first, _, last = part.partition("-")
        cpus.extend(range(int(first), int(last or first) + 1))
    return cpus


def performance_core(devices=SYSFS_DEVICES):
    """A performance core to pin both sides to. On a hybrid CPU, the last performance core (`cpu_core`); otherwise
    the last of the fastest cores by maximum frequency; `None` when the host says neither."""
    cores = devices / "cpu_core" / "cpus"
    if cores.exists():
        listed = cpu_list(cores.read_text())
        if listed:
            return listed[-1]
    speeds = {}
    for entry in sorted((devices / "system" / "cpu").glob("cpu[0-9]*")):
        frequency = entry / "cpufreq" / "cpuinfo_max_freq"
        if frequency.exists():
            speeds[int(entry.name[3:])] = int(frequency.read_text())
    if not speeds:
        return None
    fastest = max(speeds.values())
    return max(cpu for cpu, speed in speeds.items() if speed == fastest)


def pin_choice(value):
    """`auto` picks a performance core, `none` disables pinning, a number pins to that logical CPU."""
    if value == "auto":
        return performance_core()
    if value == "none":
        return None
    if value.isdigit():
        return int(value)
    raise ValueError("--pin-cpu takes auto, none or a CPU number")


def host_command(arguments, cpu=None):
    """The host side, pinned to the same logical CPU as the sandbox when `cpu` is set (a hybrid CPU's cores differ)."""
    return arguments if cpu is None else ["taskset", "-c", str(cpu)] + arguments


def same_interpreter(rounds):
    """Refuses rounds whose two sides ran different interpreters, or did not say which."""
    versions = {(sample["host"].get("python"), sample["sandbox"].get("python")) for sample in rounds}
    for host, sandbox in versions:
        if host is None or host != sandbox:
            raise ValueError("host and sandbox ran different interpreters: {} and {}".format(host, sandbox))
    return next(iter(versions))[0]


def summarize(rounds):
    """p50 of each measure on each side, and the sandbox/host ratio of those p50s."""
    summary = {}
    for measure in MEASURES:
        host = perf_common.percentile([sample["host"][measure] for sample in rounds], 50)
        sandbox = perf_common.percentile([sample["sandbox"][measure] for sample in rounds], 50)
        summary.update({"host_" + measure: host, "sandbox_" + measure: sandbox,
                        measure.rsplit("_", 1)[0] + "_ratio": round(sandbox / host, 3)})
    return summary


def verdict(summary):
    return "pass" if all(value >= RATIO_MIN for name, value in summary.items() if name.endswith("_ratio")) else "fail"


def bench(arguments, env):
    output = subprocess.run(arguments, env=env, capture_output=True, text=True, timeout=600, check=True).stdout
    return json.loads(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--rounds", type=int, default=50)
    parser.add_argument("--size-mib", type=int, default=64)
    parser.add_argument("--random-ops", type=int, default=DEFAULT_RANDOM_OPS)
    parser.add_argument("--pin-cpu", type=pin_choice, default="auto",
                        help="auto (a performance core; the default), none, or a CPU number: both sides run on it")
    options = parser.parse_args()
    if os.environ.get("TQA_DOCKER_TESTS") != "1":
        raise SystemExit("PERF-012 needs Docker: set TQA_DOCKER_TESTS=1 and run it with no other stack running")
    env = os.environ.copy()
    compose.preflight(env)
    image = demo.runtime_image(env, compose.required_image(env, "TQA_RUNTIME_IMAGE"))
    temporary = tempfile.TemporaryDirectory(prefix="tqa-perf-012-")
    root = Path(temporary.name)
    host, workspace = root / "host", root / "workspace"
    host.mkdir()
    workspace.mkdir()
    name, failure, started = "tqa-perf-io-" + secrets.token_hex(5), None, False
    sizes = ["--size-mib", str(options.size_mib), "--random-ops", str(options.random_ops)]
    try:
        compose.ownership(workspace, env, "1000:1000")
        record = perf_common.environment(env, {"runtime": image})
        record["filesystem"] = subprocess.run(["findmnt", "-no", "SOURCE,FSTYPE", "-T", str(root)],
                                              capture_output=True, text=True, check=True).stdout.strip()
        compose.command(sandbox_arguments(name, image, workspace, options.pin_cpu), env, capture=True)
        started = True
        rounds = []
        for index in range(options.warmups + options.rounds):
            seed = ["--seed", str(index)]
            sides = {"host": host_command([HOST_PYTHON, str(BENCH), str(host)] + sizes + seed, options.pin_cpu),
                     "sandbox": ["docker", "exec", "-e", "PYTHONHOME=" + SANDBOX_HOME, name, SANDBOX_PYTHON,
                                 "/bench/perf_io_bench.py", "/workspace"] + sizes + seed}
            order = ("host", "sandbox") if index % 2 == 0 else ("sandbox", "host")
            sample = {side: bench(sides[side], env) for side in order}
            if index >= options.warmups:
                rounds.append(sample)
        python = same_interpreter(rounds)
        summary = summarize(rounds)
        result = verdict(summary)
        path = perf_common.write("PERF-012", record, {
            "warmups": options.warmups, "rounds": options.rounds, "size_mib": options.size_mib,
            "random_ops": options.random_ops, "order": "alternating, host first on even rounds",
            "sandbox_options": SANDBOX_OPTIONS, "workspace": "bind mount at /workspace",
            "interpreter": "host python {} on both sides, read-only in the sandbox".format(python),
            "pinned_cpu": options.pin_cpu},
            rounds, summary, {"every_ratio_at_least": RATIO_MIN}, result)
        print("PERF-012 {} {} -> {}".format(result, {key: value for key, value in summary.items()
                                                      if key.endswith("_ratio")}, path), flush=True)
        if result != "pass":
            failure = AssertionError("PERF-012 ratio below {}: {}".format(RATIO_MIN, summary))
    except BaseException as error:
        failure = error
    finally:
        actions = [lambda: compose.ownership(workspace, env), temporary.cleanup]
        if started:
            actions.insert(0, lambda: compose.command(["docker", "rm", "--force", name], env, capture=True))
        for action in actions:
            try:
                action()
            except Exception as error:
                print("Cleanup failed: {}".format(type(error).__name__), file=sys.stderr)
                failure = failure or error
    if failure is not None:
        raise SystemExit("PERF-012 failed: {}".format(failure))


if __name__ == "__main__":
    main()
