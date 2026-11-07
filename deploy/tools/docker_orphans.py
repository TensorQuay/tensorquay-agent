#!/usr/bin/env python3
"""Find, and on request remove, containers a killed Docker daemon left behind (rootless or not).

When the daemon is killed (the OOM killer, systemd-oomd), its containerd shims die with it but the containers' own
processes may survive in their cgroups. runc keeps each container's state under its root (tmpfs, so it outlives the
daemon), and when the daemon restarts and starts the same container again, runc refuses: "container with given ID
already exists". Recreating the container (`docker compose up -d --force-recreate`) gives it a new ID and works, but
the old processes keep running old code and holding memory.

    check (the default): list every runc state whose container Docker does not report as running, paused or
        restarting, as `orphan` (its process still runs: the same PID and start time runc recorded) or `stale` (its
        process is gone, or its PID now belongs to another process). Exit 1 if there is any, 0 if none. Read-only.
    clean --yes: stop each orphan through runc, gracefully (SIGTERM, then SIGKILL after a grace period: a SIGKILLed
        database is a crash) and delete its state, and delete each stale state. Just before acting on one it asks
        Docker again and re-reads the process's start time, so a container that has started since, or a reused PID, is
        never touched. `--exclude ID` (repeatable, a prefix is enough) leaves a state alone.

A container Docker reports as running is never listed or touched. Nothing else is changed: no image, volume, network
or Docker record.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Optional

LIVE_STATES = ("running", "paused", "restarting")
STOP_GRACE_SECONDS = 30
KILL_WAIT_SECONDS = 10


@dataclass(frozen=True)
class Found:
    kind: str  # "orphan" or "stale"
    id: str
    pid: int
    created: str
    command: str

    def line(self) -> str:
        return f"{self.kind:6} {self.id[:12]} pid={self.pid} created={self.created} cmd={self.command!r}"


def default_root() -> Path:
    """The runc root the Docker daemon uses: under the user's runtime directory when rootless, /run/docker otherwise."""
    if os.geteuid() == 0:
        return Path("/run/docker/runtime-runc/moby")
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return Path(runtime) / "docker" / "runtime-runc" / "moby"


def live_ids() -> set:
    """The full IDs of the containers Docker reports as running, paused or restarting."""
    args = ["docker", "ps", "--no-trunc", "-q"]
    for state in LIVE_STATES:
        args += ["--filter", f"status={state}"]
    result = subprocess.run(args, capture_output=True, text=True, timeout=60, check=True)
    return set(result.stdout.split())


def process_start(pid: int) -> Optional[int]:
    """A process's start time in clock ticks since boot (field 22 of /proc/<pid>/stat), or None when it is gone or a
    zombie (it has exited and only waits to be reaped; its command line is empty)."""
    try:
        return parse_start(Path(f"/proc/{pid}/stat").read_text())
    except OSError:
        return None


def parse_start(stat: str) -> Optional[int]:
    """Field 22 of a /proc/<pid>/stat line, or None when the state (field 3) is zombie or dead. The command name
    (field 2) is in parentheses and may hold spaces and parentheses itself, so fields are counted after its last
    closing one."""
    fields = stat[stat.rindex(")") + 2:].split()
    return None if fields[0] in ("Z", "X") else int(fields[19])


def command_of(pid: int) -> str:
    try:
        return Path(f"/proc/{pid}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace").strip()[:60]
    except OSError:
        return ""


def read_state(directory: Path) -> Optional[dict]:
    try:
        return json.loads((directory / "state.json").read_text())
    except (OSError, ValueError):
        return None


def classify(
    root: Path,
    live: set,
    start_of: Callable[[int], Optional[int]] = process_start,
    command: Callable[[int], str] = command_of,
) -> list:
    """Every runc state under `root` not belonging to a live container, as orphan or stale, sorted by ID."""
    found = []
    if not root.is_dir():
        return found
    for directory in sorted(root.iterdir()):
        if not directory.is_dir() or directory.name in live:
            continue
        state = read_state(directory)
        pid = int((state or {}).get("init_process_pid") or 0)
        recorded = (state or {}).get("init_process_start")
        alive = pid > 0 and recorded is not None and start_of(pid) == int(recorded)
        found.append(
            Found(
                kind="orphan" if alive else "stale",
                id=directory.name,
                pid=pid,
                created=str((state or {}).get("created", ""))[:19],
                command=command(pid) if alive else "",
            )
        )
    return found


def clean(
    root: Path,
    found: Iterable,
    runc: Callable[[list], None],
    live_now: Callable[[], set] = live_ids,
    start_of: Callable[[int], Optional[int]] = process_start,
    waits: tuple = (STOP_GRACE_SECONDS, KILL_WAIT_SECONDS),
) -> list:
    """Stops (SIGTERM, then SIGKILL) and deletes each orphan, deletes each stale state; returns what it did, one line
    each."""
    done = []
    for item in found:
        # Ask again just before acting: a container started since, or a PID reused since, is left alone.
        if item.id in live_now():
            done.append(f"skipped {item.id[:12]}: Docker reports it live now")
            continue
        state = read_state(root / item.id) or {}
        recorded = state.get("init_process_start")

        def running() -> bool:
            return item.pid > 0 and recorded is not None and start_of(item.pid) == int(recorded)

        was_running = running()
        for signal, wait in zip(("TERM", "KILL"), waits):
            if not running():
                break
            runc(["--root", str(root), "kill", item.id, signal])
            deadline = time.monotonic() + wait
            while running() and time.monotonic() < deadline:
                time.sleep(0.1)
        if running():
            done.append(f"failed  {item.id[:12]}: still running after SIGTERM and SIGKILL")
            continue
        runc(["--root", str(root), "delete", "--force", item.id])
        done.append(f"removed {item.id[:12]} ({'stopped ' + item.command if was_running else 'stale state'})")
    return done


def run_runc(args: list) -> None:
    subprocess.run(["runc", *args], capture_output=True, text=True, timeout=60, check=True)


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("action", nargs="?", choices=["check", "clean"], default="check")
    parser.add_argument("--yes", action="store_true", help="required by clean")
    parser.add_argument("--root", type=Path, default=None, help="the runc root (default: the Docker daemon's)")
    parser.add_argument("--exclude", action="append", default=[], metavar="ID", help="leave this state alone")
    args = parser.parse_args(argv)
    root = args.root or default_root()
    if args.action == "clean" and not args.yes:
        print("docker-orphans: clean changes running processes; pass --yes", file=sys.stderr)
        return 2
    found = [
        item for item in classify(root, live_ids())
        if not any(item.id.startswith(prefix) for prefix in args.exclude if prefix)
    ]
    for item in found:
        print(item.line())
    if args.action == "check":
        print(f"docker-orphans: {sum(f.kind == 'orphan' for f in found)} orphan, "
              f"{sum(f.kind == 'stale' for f in found)} stale under {root}")
        return 1 if found else 0
    for line in clean(root, found, run_runc):
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
