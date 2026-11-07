"""T12 on the task-page stack (WEB-E2E-042): the browser can't make a sandbox fail, so this does it on the spec's
request. Each time the spec writes `kill-wanted` (the task's id) in the markers directory, the task's own sandbox
container is killed, `killed` is written, and once the gateway's replacement for that task is running, `replaced`.
The spec removes the markers before asking again. Started from `demo_task_page.verify_overlay`, before the stack comes
up; `verify_result` reads how it went."""

import os
from pathlib import Path
import re
import tempfile
import threading
import time

import compose

WANTED = "kill-wanted"
KILLED = "killed"
REPLACED = "replaced"
FAILED = "kill-failed"
# The task's id as the spec writes it: a UUID, so nothing else reaches the docker filter.
TASK = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

_killer = None


def scale(env):
    """The harness's deadline scale (crates/tqa-testkit/src/deadline.rs): an integer from 1 to 20, else 1."""
    text = env.get("TQA_TEST_DEADLINE_SCALE", "").strip()
    return int(text) if text.isdigit() and 1 <= int(text) <= 20 else 1


class Killer:
    # How long a replacement may take, before the deadline scale.
    replacement_seconds = 120

    def __init__(self, markers, env):
        self.markers = Path(markers)
        self.env = env
        self.stop = threading.Event()
        self.kills = 0
        self.outcome = None
        self.thread = threading.Thread(target=self.run, name="task-page-killer", daemon=True)

    def start(self):
        self.markers.mkdir(mode=0o700, exist_ok=True)
        self.thread.start()
        return self

    def sandboxes(self, task):
        """The task's own running sandbox containers on this stack."""
        return compose.command(["docker", "ps", "-q", "--no-trunc",
                                "--filter", "label=tqa.instance=" + self.env["TQA_SANDBOX_NETWORK"],
                                "--filter", "label=tqa.conversation=" + task],
                               self.env, capture=True).stdout.split()

    def run(self):
        # The whole browser run is the bound: the spec's own scaled wait fails first if a replacement never comes.
        deadline = time.monotonic() + 1800 * scale(self.env)
        wanted = self.markers / WANTED
        while True:
            while not wanted.exists():
                if self.stop.is_set() or time.monotonic() > deadline:
                    if self.kills == 0:
                        self.outcome = "the spec never wrote {}".format(WANTED)
                    return
                time.sleep(0.2)
            try:
                task = wanted.read_text().strip()
                if not TASK.fullmatch(task):
                    raise ValueError("not a task id: {!r}".format(task[:80]))
                before = self.sandboxes(task)
                if len(before) != 1:
                    raise RuntimeError("expected one running sandbox for the task, found {}".format(len(before)))
                compose.command(["docker", "kill", before[0]], self.env)
                wanted.unlink(missing_ok=True)
                self.kills += 1
                self.mark(KILLED, before[0])
                # The replacement: a new container for the same task, running (the loss schedule's longest wait is
                # 45 s; the spec asks twice, so 0 and then 5 s).
                limit = time.monotonic() + self.replacement_seconds * scale(self.env)
                while True:
                    after = [identity for identity in self.sandboxes(task) if identity not in before]
                    if after:
                        self.mark(REPLACED, after[0])
                        break
                    if time.monotonic() > limit:
                        raise RuntimeError("no replacement sandbox within {} s".format(self.replacement_seconds))
                    time.sleep(0.5)
                self.outcome = "killed {} time(s), each replaced".format(self.kills)
            except Exception as error:  # Reported to the spec, which fails naming it, and kept for the verdict.
                self.outcome = "the kill failed: {}".format(compose.redact(str(error), self.env))
                self.mark(FAILED, self.outcome)
                return

    def mark(self, name, text):
        """Writes the marker `name` whole or not at all: the spec waits for it to exist and then reads it."""
        partial = self.markers / (name + ".partial")
        partial.write_text(text)
        os.replace(partial, self.markers / name)

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=30)
        return self


def start(env):
    """Starts the killer and tells the browser run where its markers are (`TQA_TASK_PAGE_RECOVERY_MARKERS`)."""
    global _killer
    # Only on a stack run: the killer needs the stack's sandbox network, and the driver's own tests pass none.
    if "TQA_SANDBOX_NETWORK" not in env:
        return
    markers = Path(tempfile.mkdtemp(prefix="tqa-task-page-recovery-")) / "markers"
    env["TQA_TASK_PAGE_RECOVERY_MARKERS"] = str(markers)
    _killer = Killer(markers, env).start()


def verify_result():
    """The spec asked for its two kills, and each lost sandbox was replaced for the same task. Nothing to check when no
    killer was started (the driver's own tests); on a stack run the spec then fails waiting for its first kill."""
    if _killer is None:
        return
    killer = _killer.finish()
    assert killer.kills == 2 and (killer.outcome or "").endswith("each replaced"), \
        "Recovery case: {}".format(killer.outcome)
    print("Task page: the task's sandbox was killed twice mid-run and replaced each time", flush=True)
