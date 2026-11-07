"""DEP-U-001: docker_orphans finds what a killed Docker daemon left (a runc state per container), tells an orphan (its
process still runs) from a stale state, never lists or touches a container Docker reports live, and cleans only after
checking again: a container started since, or a reused PID, is left alone. Runs on a fake runc root; no Docker."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import docker_orphans  # noqa: E402


def state(root: Path, cid: str, pid: int, start: int) -> None:
    directory = root / cid
    directory.mkdir()
    (directory / "state.json").write_text(json.dumps(
        {"id": cid, "init_process_pid": pid, "init_process_start": start, "created": "2026-09-27T06:49:00Z"}))


class OrphanTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        state(self.root, "a" * 64, 100, 5000)   # live in Docker
        state(self.root, "b" * 64, 200, 6000)   # orphan: same process
        state(self.root, "c" * 64, 300, 7000)   # stale: PID reused
        state(self.root, "d" * 64, 400, 8000)   # stale: process gone
        (self.root / ("e" * 64)).mkdir()        # stale: no state file
        self.starts = {100: 5000, 200: 6000, 300: 1234}
        self.live = {"a" * 64}

    def tearDown(self):
        self.directory.cleanup()

    def start_of(self, pid):
        return self.starts.get(pid)

    def classified(self):
        return docker_orphans.classify(self.root, self.live, self.start_of, lambda pid: f"cmd-{pid}")

    def test_orphans_and_stale_states_are_found_and_live_containers_never_listed(self):
        found = {item.id[0]: item.kind for item in self.classified()}
        self.assertEqual(found, {"b": "orphan", "c": "stale", "d": "stale", "e": "stale"})
        orphan = next(item for item in self.classified() if item.kind == "orphan")
        self.assertIn("cmd-200", orphan.line())

    def test_clean_kills_orphans_deletes_stale_states_and_never_touches_the_live_one(self):
        calls = []

        def runc(args):
            calls.append(args[2:])
            if args[2] == "kill":
                self.starts.pop(200)  # the orphan stops on SIGTERM

        done = docker_orphans.clean(self.root, self.classified(), runc, lambda: self.live, self.start_of, (1, 1))
        self.assertEqual(calls[0], ["kill", "b" * 64, "TERM"], "a graceful stop first")
        self.assertNotIn(["kill", "b" * 64, "KILL"], calls, "no SIGKILL for an orphan that stopped")
        self.assertIn(["delete", "--force", "b" * 64], calls)
        for stale in "cde":
            self.assertIn(["delete", "--force", stale * 64], calls)
        self.assertFalse(any("a" * 64 in call for call in calls), "the live container is never touched")
        self.assertFalse(any(call[0] == "kill" and call[1] != "b" * 64 for call in calls), "only the orphan is killed")
        self.assertEqual(len(done), 4)

    def test_clean_checks_again_before_acting(self):
        found = self.classified()
        calls = []
        # Since the check: the orphan's container started in Docker again, and the stale PID 300 is no concern.
        started = self.live | {"b" * 64}
        done = docker_orphans.clean(self.root, found, lambda args: calls.append(args[2:]), lambda: started,
                                    self.start_of, (1, 1))
        self.assertFalse(any("b" * 64 in call for call in calls))
        self.assertIn(f"skipped {'b' * 12}: Docker reports it live now", done)

    def test_a_reused_pid_is_never_signalled(self):
        calls = []
        docker_orphans.clean(self.root, self.classified(), lambda args: calls.append(args[2:]), lambda: self.live,
                             lambda pid: {200: 6000, 300: 1234}.get(pid) if pid != 200 else None, (1, 1))
        # PID 300 now belongs to another process (a different start time): its state is deleted, nothing killed.
        self.assertFalse(any(call[0] == "kill" for call in calls), calls)
        self.assertIn(["delete", "--force", "c" * 64], calls)

    def test_an_orphan_that_survives_the_kill_is_reported_not_deleted(self):
        calls = []
        done = docker_orphans.clean(self.root, [f for f in self.classified() if f.kind == "orphan"],
                                    lambda args: calls.append(args[2:]), lambda: self.live, self.start_of, (0.2, 0.2))
        self.assertEqual(done, [f"failed  {'b' * 12}: still running after SIGTERM and SIGKILL"])
        self.assertEqual(calls, [["kill", "b" * 64, "TERM"], ["kill", "b" * 64, "KILL"]], "SIGKILL only after the grace")
        self.assertNotIn(["delete", "--force", "b" * 64], calls)

    def test_an_excluded_state_is_left_alone(self):
        original = docker_orphans.live_ids
        docker_orphans.live_ids = lambda: {"a" * 64}
        try:
            import io
            from contextlib import redirect_stdout
            out = io.StringIO()
            with redirect_stdout(out):
                docker_orphans.main(["check", "--root", str(self.root), "--exclude", "bbbb"])
            self.assertNotIn("b" * 12, out.getvalue())
            self.assertIn("c" * 12, out.getvalue())
        finally:
            docker_orphans.live_ids = original

    def test_clean_needs_yes_and_check_exits_1_when_anything_is_found(self):
        self.assertEqual(docker_orphans.main(["clean", "--root", str(self.root)]), 2)
        original = docker_orphans.live_ids
        docker_orphans.live_ids = lambda: {"a" * 64}
        try:
            with tempfile.TemporaryDirectory() as empty:
                self.assertEqual(docker_orphans.main(["check", "--root", empty]), 0)
            self.assertEqual(docker_orphans.main(["check", "--root", str(self.root)]), 1)
        finally:
            docker_orphans.live_ids = original

    def test_the_start_time_is_read_after_a_command_name_with_spaces(self):
        stat = "4242 (tqa (gate) way) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 19 20\n"
        self.assertEqual(docker_orphans.parse_start(stat), 987654)

    def test_a_zombie_counts_as_stopped(self):
        # Killed but not yet reaped: /proc still has its start time, but it runs nothing and holds no memory.
        stat = "4242 (postgres) Z 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 19 20\n"
        self.assertIsNone(docker_orphans.parse_start(stat))


if __name__ == "__main__":
    unittest.main()
