"""WEB-E2E-042's sandbox killer (demo_task_page_recovery.py), with Docker mocked: it kills only the named task's one
sandbox, reports the replacement, refuses anything but a task id, and says when no replacement came."""

from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

import demo_task_page_recovery as recovery

TASK = "01a11659-8e25-7654-8f88-57a661328c41"
ENV = {"TQA_SANDBOX_NETWORK": "tqa-sandboxes-test", "TQA_TEST_DEADLINE_SCALE": "1"}


def wait_for(path, seconds=5):
    limit = time.monotonic() + seconds
    while not path.exists():
        if time.monotonic() > limit:
            raise AssertionError("no {}".format(path.name))
        time.sleep(0.05)
    return path.read_text()


class Docker:
    """Answers `docker ps` with the task's sandboxes: the original until it's killed, then its replacement."""

    def __init__(self, replace=True):
        self.calls = []
        self.killed = False
        self.replace = replace

    def __call__(self, args, env, capture=False, **_):
        self.calls.append(args)
        if args[:2] == ["docker", "kill"]:
            self.killed = True
            return Mock(stdout="")
        if not self.killed:
            return Mock(stdout="original\n")
        return Mock(stdout="replacement\n" if self.replace else "")


class KillerTests(unittest.TestCase):
    def run_killer(self, docker, request):
        markers = Path(tempfile.mkdtemp()) / "markers"
        with patch.object(recovery.compose, "command", side_effect=docker):
            killer = recovery.Killer(markers, ENV).start()
            (markers / recovery.WANTED).write_text(request)
            yield markers, killer
            killer.finish()

    def test_a_kill_names_the_task_and_reports_its_replacement(self):
        docker = Docker()
        for markers, killer in self.run_killer(docker, TASK):
            self.assertEqual(wait_for(markers / recovery.KILLED), "original")
            self.assertEqual(wait_for(markers / recovery.REPLACED), "replacement")
            self.assertFalse((markers / recovery.WANTED).exists())
        self.assertEqual(killer.kills, 1)
        self.assertEqual(killer.outcome, "killed 1 time(s), each replaced")
        listing = docker.calls[0]
        self.assertIn("label=tqa.conversation=" + TASK, listing)
        self.assertIn("label=tqa.instance=tqa-sandboxes-test", listing)
        self.assertIn(["docker", "kill", "original"], docker.calls)

    def test_anything_but_a_task_id_is_refused_before_docker(self):
        docker = Docker()
        for markers, killer in self.run_killer(docker, TASK + " --filter label=x"):
            self.assertIn("not a task id", wait_for(markers / recovery.FAILED))
        self.assertEqual(docker.calls, [])
        self.assertEqual(killer.kills, 0)

    def test_no_replacement_is_a_failure(self):
        docker = Docker(replace=False)
        with patch.object(recovery.Killer, "replacement_seconds", 0):
            for markers, killer in self.run_killer(docker, TASK):
                self.assertIn("no replacement", wait_for(markers / recovery.FAILED))
        self.assertTrue(docker.killed)

    def test_the_driver_check_wants_two_kills(self):
        with patch.object(recovery, "_killer", Mock(finish=Mock(return_value=Mock(
                kills=1, outcome="killed 1 time(s), each replaced")))):
            with self.assertRaises(AssertionError):
                recovery.verify_result()
        with patch.object(recovery, "_killer", Mock(finish=Mock(return_value=Mock(
                kills=2, outcome="killed 2 time(s), each replaced")))):
            recovery.verify_result()
        with patch.object(recovery, "_killer", None):
            recovery.verify_result()

    def test_start_needs_the_stack_network(self):
        env = {}
        with patch.object(recovery, "_killer", None):
            recovery.start(env)
            self.assertIsNone(recovery._killer)
        self.assertNotIn("TQA_TASK_PAGE_RECOVERY_MARKERS", env)


if __name__ == "__main__":
    unittest.main()
