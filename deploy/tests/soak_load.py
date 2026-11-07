"""The soak's load record, beside the run and never part of its verdict: every 30 s, `docker stats` for every container,
the host's pressure stall information (CPU, memory, I/O) and the disk, as JSON lines; then a summary of the soak's own
containers, with disk growth projected to 24 hours. The soak's database size and gateway memory are in its own
journal (`resources`).

    soak_load.py sample --output load.jsonl --path EVIDENCE [--path DIR ...] --while-pid PID [--interval 30]
    soak_load.py summary load.jsonl --journal EVIDENCE/chaos.jsonl   (or --project tqa-compose-...)
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time

UNITS = {"B": 1, "kB": 10 ** 3, "KB": 10 ** 3, "MB": 10 ** 6, "GB": 10 ** 9, "TB": 10 ** 12,
         "KiB": 2 ** 10, "MiB": 2 ** 20, "GiB": 2 ** 30, "TiB": 2 ** 40}
PRESSURE = ("cpu", "memory", "io")


def size(text):
    """Bytes from a size as `docker stats` prints it (`1.5MB`, `28.5MiB`, `0B`)."""
    match = re.fullmatch(r"\s*([0-9.]+)\s*([A-Za-z]+)\s*", text)
    if not match or match.group(2) not in UNITS:
        raise ValueError("unreadable size: " + text)
    return int(float(match.group(1)) * UNITS[match.group(2)])


def containers(stats):
    """One row per container from `docker stats --no-stream --format '{{json .}}'`."""
    rows = []
    for line in stats.splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        read, written = (size(part) for part in row["BlockIO"].split("/"))
        rows.append({"name": row["Name"], "cpu": float(row["CPUPerc"].rstrip("%")),
                     "memory": size(row["MemUsage"].split("/")[0]), "pids": int(row["PIDs"] or 0),
                     "read": read, "written": written})
    return rows


def pressure(text):
    """The avg10, avg60 and avg300 of each line of a /proc/pressure file."""
    averages = {}
    for line in text.splitlines():
        kind, *fields = line.split()
        values = dict(field.split("=") for field in fields)
        averages[kind] = [float(values["avg10"]), float(values["avg60"]), float(values["avg300"])]
    return averages


def run_command(arguments):
    return subprocess.run(arguments, check=True, capture_output=True, text=True, timeout=25).stdout


def read_pressure(name):
    with open("/proc/pressure/" + name, encoding="ascii") as handle:
        return handle.read()


def disk_usage(path):
    usage = shutil.disk_usage(path)
    return usage.total, usage.free


def sample(*, paths, run=run_command, read=read_pressure, usage=disk_usage, now=time.time):
    """One sample; a part that fails is recorded as missing with its error, never raised."""
    record, errors = {"time": now()}, []
    try:
        record["containers"] = containers(run(["docker", "stats", "--no-stream", "--format", "{{json .}}"]))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        record["containers"] = None
        errors.append("docker stats: {}".format(error))
    try:
        record["pressure"] = {name: pressure(read(name)) for name in PRESSURE}
    except (OSError, ValueError, KeyError) as error:
        record["pressure"] = None
        errors.append("pressure: {}".format(error))
    disk = {}
    for path in paths:
        try:
            used = int(run(["du", "-sk", path]).split()[0]) * 1024
            total, free = usage(path)
            disk[path] = {"used": used, "filesystem_used": total - free, "filesystem_free": free}
        except (OSError, ValueError, IndexError, subprocess.SubprocessError) as error:
            disk[path] = None
            errors.append("disk {}: {}".format(path, error))
    record["disk"] = disk
    if errors:
        record["errors"] = errors
    return record


def statistics(values):
    return {"max": max(values), "mean": round(sum(values) / len(values), 2)} if values else None


def summarize(samples, project):
    """The soak's containers (names starting with its Compose project), host pressure and disk growth over the run."""
    if not samples:
        raise ValueError("no load samples")
    counted = [row for row in samples if row.get("containers") is not None]
    cpu, memory, count, written = [], [], [], {}
    for row in counted:
        own = [container for container in row["containers"] if container["name"].startswith(project + "-")]
        cpu.append(round(sum(container["cpu"] for container in own), 2))
        memory.append(sum(container["memory"] for container in own))
        count.append(len(own))
        for container in own:
            # Counters are per container lifetime: its last value is what it wrote.
            written[container["name"]] = container["written"]
    summary = {"samples": len(samples), "missed_samples": len(samples) - len(counted),
               "seconds": samples[-1]["time"] - samples[0]["time"], "project": project,
               "soak": {"cpu_percent": statistics(cpu), "memory_bytes": statistics(memory),
                        "containers": statistics(count), "written_bytes": sum(written.values())}}
    measured = [row["pressure"] for row in samples if row.get("pressure")]
    summary["pressure"] = {}
    for name in PRESSURE:
        some = [row[name]["some"] for row in measured]
        summary["pressure"][name] = {"some_avg10_max": max((values[0] for values in some), default=None),
                                     "some_avg60_mean": round(sum(values[1] for values in some) / len(some), 2)
                                     if some else None}
    summary["disk"] = {}
    elapsed = summary["seconds"]
    paths = {path for row in samples for path in (row.get("disk") or {})}
    for path in sorted(paths):
        points = [row["disk"][path] for row in samples if (row.get("disk") or {}).get(path)]
        if not points:
            continue
        growth = points[-1]["used"] - points[0]["used"]
        summary["disk"][path] = {
            "start_bytes": points[0]["used"], "end_bytes": points[-1]["used"], "growth_bytes": growth,
            "growth_bytes_per_24h": growth * 24 * 3600 // elapsed if elapsed > 0 else None,
            "filesystem_free_min": min(point["filesystem_free"] for point in points)}
    return summary


def project_from(journal):
    with open(journal, encoding="utf-8") as handle:
        for line in handle:
            record = json.loads(line)
            if record.get("kind") == "configuration":
                return record["project"]
    raise ValueError("the journal has no configuration record")


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def main(arguments=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    sampling = commands.add_parser("sample")
    sampling.add_argument("--output", required=True)
    sampling.add_argument("--path", action="append", default=[])
    sampling.add_argument("--while-pid", type=int, required=True)
    sampling.add_argument("--interval", type=float, default=30)
    summary = commands.add_parser("summary")
    summary.add_argument("samples")
    which = summary.add_mutually_exclusive_group(required=True)
    which.add_argument("--journal")
    which.add_argument("--project")
    options = parser.parse_args(arguments)
    if options.command == "sample":
        with open(options.output, "x", encoding="utf-8", buffering=1) as output:
            while alive(options.while_pid):
                started = time.monotonic()
                output.write(json.dumps(sample(paths=options.path), sort_keys=True) + "\n")
                time.sleep(max(0.0, options.interval - (time.monotonic() - started)))
        return 0
    with open(options.samples, encoding="utf-8") as handle:
        samples = [json.loads(line) for line in handle if line.strip()]
    project = options.project or project_from(options.journal)
    json.dump(summarize(samples, project), sys.stdout, indent=1, sort_keys=True)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
