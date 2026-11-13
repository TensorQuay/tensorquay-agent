"""SOAK-001…003's recovery verdicts, pure functions of times and counts: a conversation's recovery from a full disk or
a store stall, the store stalls' rate, the missed resource samples, and the full-disk probe's choice and verdict."""
import time

from soak_stream import DISK_RECOVERY_SECONDS


def disk_recovery(window, steps, completions, now, final=False, margin=DISK_RECOVERY_SECONDS):
    """A conversation that got `io` in a disk_full window recovers in two parts: a step completes after the free and
    within the margin (progress resumed promptly), and a run completes after the free before the soak ends (the work
    still finishes). A scripted run lasts several steps, longer than the margin (the c14b soak, 5 Oct 2026: a step's
    median 69 s, a run's p90 356 s), so the margin bounds the first step, not a whole run. `failed_step` once the
    margin passes without a step, `failed_run` when the soak ends with no run completed, `recovered` with both, else
    `pending`. A step or run completed before the free proves nothing."""
    end = window[1]
    if end == float("inf"):
        return "pending"
    stepped = any(end <= step <= end + margin for step in steps)
    if not stepped:
        return "failed_step" if now > end + margin else "pending"
    if any(completion >= end for completion in completions):
        return "recovered"
    return "failed_run" if final else "pending"


def io_recovery(window, completions, now, margin=DISK_RECOVERY_SECONDS):
    """A run must complete within the margin after `window` closes: `recovered` once one does, `failed` when the margin
    passes without one, else `pending`. A store_unavailable outside a database fault uses it (store_recoveries); a full
    disk uses disk_recovery. A completion before the window's end proves nothing."""
    end = window[1]
    if end == float("inf"):
        return "pending"
    if any(end <= completion <= end + margin for completion in completions):
        return "recovered"
    return "failed" if now > end + margin else "pending"


# The gateway's store may time out on a busy host outside every database fault (its clients get store_unavailable).
# Accepted as load, not a fault: the conversation must complete a run within this margin after the notice, the
# notices are counted in the result, and more than STORE_EVENTS_PER_HOUR stalls in any hour fail the run.
STORE_RECOVERY_SECONDS = 180
STORE_EVENTS_PER_HOUR = 5


def store_events(times, gap=10.0):
    """One stall reaches several clients within seconds: notices closer than `gap` to the event's first are one."""
    events = []
    for when in sorted(times):
        if not events or when - events[-1] > gap:
            events.append(when)
    return events


def too_many_store_events(events, per_hour=STORE_EVENTS_PER_HOUR):
    """Whether any rolling hour holds more than `per_hour` events."""
    events = sorted(events)
    return any(sum(1 for later in events[i:] if later - first < 3600) > per_hour for i, first in enumerate(events))


# Resource samples on a busy host: a sample may miss while the engine is loaded (builds, other work); the run fails
# only when the sampler has stopped working (no success for this long) or too few points remain to judge growth.
SAMPLE_STALL_SECONDS = 15 * 60


def missed_sample_problem(missed, attempts):
    """Why the missed resource samples leave growth unjudgeable (more than a tenth, and more than two), else None."""
    if missed > max(2, attempts // 10):
        return "missed {} of {} resource samples: growth can't be judged".format(missed, attempts)
    return None


def replacements_on_full_disk(times, windows, margin):
    """The replacement times inside a full-disk window or its margin: the gateway replaced those runtimes by design."""
    return sorted(when for when in times if any(start <= when <= end + margin for start, end in windows))


def probe_verdict(refusal):
    """The full-disk probe's message: a clean typed refusal (an HTTP error whose body names a code), or accepted. An
    accepted message must then complete within the margin after the free, which the recovery check enforces like any
    other conversation's; only an untyped refusal fails here."""
    if refusal is not None:
        return "refused: {} {}".format(refusal["status"], refusal["code"]) if refusal.get("code") else "failed"
    return "accepted"


def probe_slot(settled):
    """A conversation with no run in flight, so the probe's message starts a run on the full disk; None when every
    one is busy. A message into a busy conversation is answered `busy`, which the full-disk rule doesn't excuse."""
    return next((index for index, ready in enumerate(settled) if ready), None)


def wait_for_idle(settled, *, poll, seconds, clock=time.monotonic, sleep=time.sleep):
    """The first idle conversation within `seconds`, draining events between checks so a run's end is seen; None if
    none became idle in time."""
    deadline = clock() + seconds
    while True:
        index = probe_slot(settled())
        if index is not None or clock() >= deadline:
            return index
        poll()
        sleep(0.25)


def probe_outcome(probe):
    """A skipped probe (no conversation was idle while the disk was full) is recorded, not judged; a sent one is
    judged by its refusal."""
    return "skipped" if probe.get("skipped") else probe_verdict(probe["refusal"])
