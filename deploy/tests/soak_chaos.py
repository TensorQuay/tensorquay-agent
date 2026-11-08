"""SOAK-002: seeded fault plans and one-shot helpers confined to an owned container."""
import random
import re

from soak_audit import require
from soak_disruptions import GLOBAL, RECOVERY
from soak_growth import STALE_SECONDS


def restart_times(seconds, rehearsal):
    """Gateway restarts: every 5 min in a rehearsal, every 30 min in the 2 h run (SOAK-002), and every 2 h through
    hour 12 of the 24 h run (SOAK-003), whose last 12 h are one gateway lifetime for the growth criterion (M5)."""
    if rehearsal:
        return list(range(300, int(seconds) + 1, 300))
    if seconds >= 24 * 3600:
        return list(range(7200, 12 * 3600 + 1, 7200))
    if seconds >= 8 * 3600:
        # The microVM run: hours 4–8 are one gateway lifetime.
        return list(range(7200, 4 * 3600 + 1, 7200))
    if seconds >= 4 * 3600:
        # The real-model run: a restart every hour.
        return list(range(3600, int(seconds) + 1, 3600))
    return list(range(1800, int(seconds) + 1, 1800))


def schedule(seed, seconds, rehearsal=False, slots=4, restarts=None, disruptions=()):
    """Seed pauses and shuffled targets over `slots` conversations with two clients each; avoid attributing a pause
    failure to a restart or a disruption (soak_disruptions): a pause ends before the next restart or global fault
    begins, never starts inside one's recovery, and keeps clear of its own slot's faults."""
    randomizer = random.Random(seed)
    faults, targets, at, available = [], [], 0, [0] * slots
    restarts = restart_times(seconds, rehearsal) if restarts is None else restarts
    spans = [(fault["at"], fault["at"] + fault.get("seconds", 0) + RECOVERY[fault["kind"]], fault)
             for fault in disruptions]
    barriers = sorted(set(restarts) | {start for start, _, fault in spans if fault["kind"] in GLOBAL})

    def blocked(at, duration, target):
        end = at + duration + 60
        return any(start < end and at < stop and (fault["kind"] in GLOBAL or fault["target"] == target)
                   for start, stop, fault in spans)
    while True:
        if not targets:
            targets = [("client_sse", index) for index in range(2 * slots)] + [("runtime_link", index)
                                                                                for index in range(slots)]
            randomizer.shuffle(targets)
        at += randomizer.randint(4, 6) if rehearsal else randomizer.randint(30, 120)
        if at > seconds:
            break
        kind, target = targets[-1]
        duration = randomizer.randint(121, 150) if kind == "runtime_link" else 0
        upcoming = [barrier for barrier in barriers if barrier >= at]
        remaining = upcoming[0] - at if upcoming else float("inf")
        if kind == "runtime_link" and (at <= available[target] or remaining <= duration + 60
                                       or blocked(at, duration, target)):
            kind, target = "client_sse", randomizer.randrange(2 * slots)
        else:
            targets.pop()
        fault = {"at": at, "kind": kind, "target": target}
        if kind == "runtime_link":
            fault["pause_seconds"] = duration
            available[target] = at + duration + 60
        faults.append(fault)
    faults += [{"at": at, "kind": "gateway_restart", "target": None} for at in restarts]
    faults += [dict(fault, target=fault.get("target")) for fault in disruptions]
    return sorted(faults, key=lambda fault: fault["at"])


def observer_arguments(image, container):
    """A pinned, unprivileged observer may read only the owned gateway's process namespace."""
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", image), "observer image must be pinned")
    require(re.fullmatch(r"[0-9a-f]{64}", container), "observer target must be an owned container ID")
    return ["docker", "run", "--rm", "--read-only", "--cap-drop", "ALL", "--security-opt",
            "no-new-privileges", "--pids-limit", "32", "--memory", "64m", "--cpus", "0.25",
            "--pid", "container:" + container, "--network", "none", "--user", "1000:1000",
            "--entrypoint", "python3", image]


def runtime_identity(record, network):
    """Compare the process container, its addresses and its opaque credential binding across a pause. A microVM's host
    container sits on the microVM network and may carry no credential label, so every network counts and the label is
    optional; `network` stays for the Docker sandbox network's name in the record."""
    networks = record["NetworkSettings"]["Networks"]
    return {"container": record["Id"], "address": networks.get(network, {}).get("IPAddress"),
            "addresses": {name: settings.get("IPAddress") for name, settings in sorted(networks.items())},
            "credential_binding": record["Config"]["Labels"].get("tqa.token_id")}


# The gateway's resident memory (KiB) and open descriptors, read in its PID namespace by the observer.
PROCESS_HELPER = ("import os; from pathlib import Path; print(next(line.split()[1] for line in "
                  "Path('/proc/1/status').read_text().splitlines() if line.startswith('VmRSS:')), "
                  "len(os.listdir('/proc/1/fd')))")
# The database's size, every backend on it (this probe and autovacuum workers included: the count before 9 Oct 2026),
# and every client backend but the probe's own: today, the gateway's (SOAK-001's rule judges it as the gateway's).
# Then, of those, the oldest idle one's age in seconds, and how many have been so for over STALE_SECONDS (9 Oct 2026): a
# pool's connection goes back into use, while one taken and never used again idles for good. Idle is any of
# Postgres's three idle states, 'idle in transaction (aborted)' included: a guard leaked on an error path holds that.
# A stack-readings poll (readings.py, application_name tqa-stack-readings) is never counted: soak stacks start no
# reader, and if one ever ran beside a soak its brief backend must not read as a gateway connection.
_READER = " and application_name <> 'tqa-stack-readings'"
_GATEWAY = ("from pg_stat_activity where datname = current_database() "
            "and backend_type = 'client backend' and pid <> pg_backend_pid()" + _READER)
_IDLE = " and state like 'idle%'"
DATABASE_QUERY = ("select pg_database_size(current_database()), "
                  "(select count(*) from pg_stat_activity where datname = current_database()" + _READER + "), "
                  f"(select count(*) {_GATEWAY}), "
                  f"(select coalesce(max(extract(epoch from now() - state_change)), 0)::int {_GATEWAY}{_IDLE}), "
                  f"(select count(*) {_GATEWAY}{_IDLE} and now() - state_change > interval '{STALE_SECONDS} seconds')")


def process_sample(text):
    """The observer's `<rss KiB> <open fds>` line; an empty or zero reading fails."""
    parts = text.split()
    require(len(parts) == 2 and all(part.isdigit() and int(part) > 0 for part in parts), "empty process measurement")
    return {"gateway_rss_bytes": int(parts[0]) * 1024, "open_fds": int(parts[1])}


def database_sample(text):
    """psql's `<size>|<connections>|<gateway connections>|<oldest idle age s>|<stale>`; the query's own connection
    makes the second zero impossible, while an idle pool may hold none (the third to fifth may be 0)."""
    parts = text.strip().split("|")
    require(len(parts) == 5 and all(part.isdigit() for part in parts) and int(parts[0]) > 0 and int(parts[1]) > 0,
            "empty database measurement")
    return {"database_bytes": int(parts[0]), "database_connections": int(parts[1]),
            "gateway_connections": int(parts[2]), "gateway_idle_age_seconds": int(parts[3]),
            "gateway_stale_connections": int(parts[4])}
