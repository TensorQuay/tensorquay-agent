"""SEC-017's stack half (Docker): the probes run in the second conversation's sandbox and the checks on what they found,
without a stack. The gateway half is crates/tqa-gateway/tests/history_isolation.rs; the microVM counterpart is
deploy/firecracker's history contract (G-13-history).

Two people share a workspace, each with a task in it. The first runs a turn, so its write-ahead log exists; then the
second takes the workspace and runs. From the second's sandbox:
- H1 its mounts: the state mount is the second's own runtime-state directory, and no mount names the first;
- H2 a walk of its filesystem, across its mounts (skipping /proc, /sys and /dev), finds no path naming the first
  conversation. The walk reports the distinct ids in path names, not the paths, so its record stays small; a file
  named with a fresh id, planted in the workspace first, must be among them, so a walk that saw nothing fails;
- H3 a forged write-ahead log written into the shared workspace (at its root and in a hidden directory) changes nothing:
  the first resumes, its earlier history is an unchanged prefix of its history after, and the forged marker appears
  nowhere in it.
"""

import json
import re

STATE_PATH = "/run/tqa/state"
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
MARKER = "sec-017-forged-history"
# The distinct uuid-shaped names on the sandbox's filesystem, one per line, crossing into its mounts (the workspace and
# the state directory are mounts); stderr (unreadable directories) dropped.
WALK = ("find / \\( -path /proc -o -path /sys -o -path /dev \\) -prune -o -print 2>/dev/null "
        "| grep -oE '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' | sort -u")
FORGED_PATHS = ("/workspace/events.jsonl", "/workspace/.tqa-forged/events.jsonl")


def forged_log(first):
    """A write-ahead log that claims to continue the first conversation: a text and a failed run, both marked."""
    frames = [
        {"type": "event", "seq": 1, "ts": "2026-01-01T00:00:00Z", "run_id": "forged",
         "event": {"type": "text.delta", "text": MARKER + " for " + first}},
        {"type": "event", "seq": 2, "ts": "2026-01-01T00:00:00Z", "run_id": "forged",
         "event": {"type": "run.failed", "run_id": "forged", "reason": MARKER, "code": "internal"}},
    ]
    return "".join(json.dumps(frame) + "\n" for frame in frames)


def forge_command():
    """The shell command, run in the second's sandbox, that writes the forged log (read from stdin) to each path."""
    return "mkdir -p /workspace/.tqa-forged && tee {} >/dev/null".format(" ".join(FORGED_PATHS))


def check_mounts(mounts, first, second):
    """H1: `mounts` is `docker inspect`'s Mounts list for the second's sandbox."""
    state = [mount for mount in mounts if mount.get("Destination") == STATE_PATH]
    assert len(state) == 1, "expected one state mount, found {}".format(state)
    source = state[0].get("Source", "")
    assert source.rstrip("/").endswith("runtime-state/" + second), "the state mount is not the second's own: " + source
    naming = [mount.get("Source", "") for mount in mounts if first in mount.get("Source", "")]
    assert not naming, "a mount names the first conversation: {}".format(naming)
    return {"mounts": len(mounts), "state": "runtime-state/" + second}


def control_command(control):
    """The shell command that plants the walk's positive control: a file in the workspace named with a fresh id."""
    return "mkdir -p /workspace/.tqa-probe && : > /workspace/.tqa-probe/" + control


def check_walk(output, first, second, control):
    """H2: the walk's distinct ids; it saw the planted control, and the first's id is not among them."""
    ids = sorted(set(UUID.findall(output)))
    assert control in ids, "the walk did not reach the workspace (its control is missing): it proves nothing"
    assert first not in ids, "the second's sandbox can see a path naming the first conversation"
    return {"ids_seen": len(ids), "control_seen": True, "second_seen": second in ids}


def history(frames):
    """A conversation's history as comparable items: (seq, event type, the event's JSON)."""
    items = []
    for frame in frames:
        event = frame.get("event") or {}
        items.append((frame.get("seq"), event.get("type"), json.dumps(event, sort_keys=True)))
    return items


def check_history(before, after):
    """H3: the first's history before the forgery is an unchanged prefix of its history after, with no forged marker."""
    earlier, later = history(before), history(after)
    assert earlier, "the first had no history before the forgery"
    assert later[:len(earlier)] == earlier, "the first's earlier history changed"
    assert len(later) > len(earlier), "the first did not resume"
    assert all(MARKER not in text for _, _, text in later), "the forged history reached the first's"
    kinds = [kind for _, kind, _ in later[len(earlier):]]
    assert "run.completed" in kinds, "the first's resumed run did not complete: {}".format(kinds)
    return {"before": len(earlier), "after": len(later)}
