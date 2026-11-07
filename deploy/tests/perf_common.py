"""TEST-PLAN §15.1: the environment record, percentiles, result files and the paired regression check shared by the
PERF harnesses. A result without its environment is not a result."""
import argparse
import datetime
import json
import math
import os
import subprocess
import sys
from pathlib import Path

REQUIRED = ("utc", "kernel", "cpu", "logical_cores", "memory_bytes", "docker", "compose", "rootless", "commit",
            "dirty_tree", "images", "load_before", "io_pressure_before")
RESULTS = Path(__file__).resolve().parents[2] / "tests" / "perf" / "results"
REGRESSION = 0.10
SAME_SESSION = datetime.timedelta(hours=24)


def _run(arguments, env=None):
    output = subprocess.run(arguments, env=env, capture_output=True, text=True, timeout=30, check=True)
    return output.stdout.strip()


def percentile(samples, percent):
    """Nearest-rank percentile; p99 of 50 samples is the maximum, as in PERF-001."""
    if not samples:
        raise ValueError("no samples")
    ordered = sorted(samples)
    rank = math.ceil(len(ordered) * percent / 100)
    return ordered[max(rank, 1) - 1]


def io_pressure():
    try:
        return Path("/proc/pressure/io").read_text().strip()
    except OSError:
        return "unavailable"


def environment(env, images):
    """The host, Docker and code this result was measured on; `images` maps each role to its digest or local ID."""
    cpu = next((line.split(":", 1)[1].strip() for line in _run(["lscpu"]).splitlines()
                if line.startswith("Model name")), "unknown")
    security = _run(["docker", "info", "--format", "{{json .SecurityOptions}}"], env)
    return {
        "utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "kernel": _run(["uname", "-sr"]), "cpu": cpu, "logical_cores": os.cpu_count(),
        "memory_bytes": os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES"),
        "disk": _run(["lsblk", "-dno", "MODEL", _run(["findmnt", "-no", "SOURCE", "/"])]) or "unknown",
        "docker": _run(["docker", "version", "--format", "{{.Server.Version}}"], env),
        "compose": _run(["docker", "compose", "version", "--short"], env),
        "rootless": "rootless" in security,
        "commit": _run(["git", "rev-parse", "HEAD"]), "dirty_tree": bool(_run(["git", "status", "--porcelain"])),
        "images": dict(images), "load_before": os.getloadavg(), "io_pressure_before": io_pressure(),
    }


def log_directory(identity):
    """Where a run keeps its gateway and model logs, beside its results (git-ignored)."""
    now = datetime.datetime.now(datetime.timezone.utc)
    directory = RESULTS / now.strftime("%Y-%m-%d") / "{}-logs-{}".format(identity.lower(), now.strftime("%H%M%S"))
    directory.mkdir(parents=True)
    return directory


def keep_readings(env, identity):
    """Names the run's own result folder for its stack readings (readings.py), unless the caller named one: a perf
    verdict comes after the stack is retired, so a slow run with no error reaches retire as a pass, and its readings,
    the ones most wanted, are kept only in a folder named before. Made only when the readings are kept."""
    if not env.get("TQA_STACK_READINGS_DIR"):
        now = datetime.datetime.now(datetime.timezone.utc)
        env["TQA_STACK_READINGS_DIR"] = str(RESULTS / now.strftime("%Y-%m-%d") / "{}-readings-{}".format(
            identity.lower(), now.strftime("%H%M%S")))
    return env["TQA_STACK_READINGS_DIR"]


def write(identity, environment_record, config, samples, summary, target, verdict):
    """Writes one result under tests/perf/results/<date>/ and returns its path; refuses an incomplete environment."""
    missing = [field for field in REQUIRED if field not in environment_record]
    if missing:
        raise ValueError("the environment record lacks: " + ", ".join(missing))
    record = {"id": identity, "environment": dict(environment_record, load_after=os.getloadavg(),
                                                  io_pressure_after=io_pressure()),
              "config": config, "samples": samples, "summary": summary, "target": target, "verdict": verdict}
    now = datetime.datetime.now(datetime.timezone.utc)
    directory = RESULTS / now.strftime("%Y-%m-%d")
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "{}-{}.json".format(identity.lower(), now.strftime("%H%M%S"))
    path.write_text(json.dumps(record, indent=2) + "\n")
    return path


def compare(before, after):
    """Paired A/B (§15.1 item 3): the same host and session; returns the regressions over 10 %, one line each."""
    for field in ("kernel", "cpu", "docker"):
        if before["environment"][field] != after["environment"][field]:
            raise ValueError("not a paired comparison: {} differs".format(field))
    moments = [datetime.datetime.strptime(record["environment"]["utc"], "%Y-%m-%dT%H:%M:%SZ")
               for record in (before, after)]
    if abs(moments[1] - moments[0]) > SAME_SESSION:
        raise ValueError("not a paired comparison: the runs are more than 24 h apart")
    if before["id"] != after["id"]:
        raise ValueError("not a paired comparison: different measurements")
    worse = []
    for name, old in before["summary"].items():
        new = after["summary"].get(name)
        higher_is_better = name.endswith(("_mbps", "_ops", "_ratio"))
        if isinstance(old, (int, float)) and isinstance(new, (int, float)) and old > 0:
            change = (old - new) / old if higher_is_better else (new - old) / old
            if change > REGRESSION:
                worse.append("{}: {} -> {} ({:+.0%})".format(name, old, new, change))
    return worse


def main():
    parser = argparse.ArgumentParser(description="Compare two PERF results from the same host and session")
    parser.add_argument("command", choices=["compare"])
    parser.add_argument("before")
    parser.add_argument("after")
    args = parser.parse_args()
    worse = compare(json.loads(Path(args.before).read_text()), json.loads(Path(args.after).read_text()))
    for line in worse:
        print("regression: " + line)
    sys.exit(1 if worse else 0)


if __name__ == "__main__":
    main()
