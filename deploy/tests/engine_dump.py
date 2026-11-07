"""A snapshot of the local container engine when a step stalls: the goroutine stacks of this user's own rootless dockerd
and containerd (SIGUSR1: dockerd writes a file, containerd logs them), summarised by what the goroutines wait on, and the
host's pressure. Only this user's own daemons are signalled; nothing is changed.
"""

import collections
import glob
import os
import re
import signal
import subprocess
import time
from pathlib import Path

DUMP_WAIT_SECONDS = 2
TOP_FRAMES = 12
# `goroutine 123 [sync.Mutex.Lock, 2 minutes]:` and the frame line after it.
HEADER = re.compile(r"^goroutine \d+ \[([^,\]]+)(?:, ([^\]]+))?\]:$")


def own_pids(name):
    done = subprocess.run(["pgrep", "-u", str(os.getuid()), "-x", name], capture_output=True, text=True, timeout=10,
                          check=False)
    return [int(pid) for pid in done.stdout.split()]


# Goroutines that only hold an idle connection open; a stall is never these, and there are hundreds of them.
IDLE = (("IO wait", "internal/poll.runtime_pollWait"), ("select", "net/http.(*persistConn).writeLoop"),
        ("IO wait", "net/http.(*persistConn).readLoop"), ("select", "net/http.(*persistConn).readLoop"))


def minutes(wait):
    match = re.match(r"(\d+) (minutes|seconds)", wait or "")
    if not match:
        return 0.0
    return int(match.group(1)) / (1 if match.group(2) == "minutes" else 60)


def summary(stacks):
    """The goroutines by wait state and top frame, most frequent first, with the longest wait in each group; idle
    connections are counted apart. A stall shows as many goroutines waiting on one lock or call."""
    counts, longest, idle, lines = collections.Counter(), {}, 0, stacks.splitlines()
    for index, line in enumerate(lines):
        match = HEADER.match(line.strip())
        if not match or index + 1 >= len(lines):
            continue
        frame = lines[index + 1].strip().split("(0x")[0]
        state = match.group(1)
        if (state, frame) in IDLE:
            idle += 1
            continue
        key = "[{}] {}".format(state, frame)
        counts[key] += 1
        if minutes(match.group(2)) >= minutes(longest.get(key)):
            longest[key] = match.group(2)
    report = ["{:5d} {}{}".format(count, key, " (longest {})".format(longest[key]) if longest.get(key) else "")
              for key, count in counts.most_common(TOP_FRAMES)]
    return report + ["{:5d} idle connections (not listed)".format(idle)]


def pressure():
    lines = []
    for resource in ("cpu", "io", "memory"):
        try:
            text = Path("/proc/pressure", resource).read_text()
        except OSError:
            continue
        lines += ["{} {}".format(resource, line) for line in text.splitlines()]
    return lines


def snapshot(reason):
    """Signals this user's dockerd and containerd for their stacks and returns a readable report."""
    report = ["Engine snapshot ({}) at {}".format(reason, time.strftime("%H:%M:%S"))]
    before = time.time()
    for name in ("dockerd", "containerd"):
        for pid in own_pids(name):
            try:
                os.kill(pid, signal.SIGUSR1)
                report.append("{} {}: stack dump requested".format(name, pid))
            except OSError as error:
                report.append("{} {}: {}".format(name, pid, error))
    time.sleep(DUMP_WAIT_SECONDS)
    dumps = [path for path in glob.glob("/run/user/{}/docker/goroutine-stacks-*.log".format(os.getuid()))
             if os.path.getmtime(path) >= before - 1]
    if dumps:
        newest = max(dumps, key=os.path.getmtime)
        report.append("dockerd stacks: " + newest)
        report += summary(Path(newest).read_text(errors="replace"))
    else:
        report.append("dockerd stacks: no new dump file")
    report.append("containerd stacks: in its journal (journalctl --user -u docker) at the time above")
    return "\n".join(report + pressure())
