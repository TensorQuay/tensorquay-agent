"""SEC-010 on the microVM: the storm's command and the judgement over what the stormed guest reported (no KVM)."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pids_checks as checks  # noqa: E402


def output(max_=256, storm=356, current=256, limited=100, done=True, refused=None):
    refused = limited if refused is None else refused
    lines = ["noise", "pids max={} storm={} current={} limited={} refused={}".format(max_, storm, current, limited,
                                                                                  refused)]
    if done:
        lines.append("pids done")
    return "\n".join(lines) + "\n"


class Storm(unittest.TestCase):
    def test_the_storm_reads_its_limit_in_the_guest_and_goes_past_it_with_sleeps_only(self):
        self.assertIn('g = "/sys/fs/cgroup/sandbox"', checks.STORM)
        self.assertIn('limit = int(open(g + "/pids.max").read())', checks.STORM)
        self.assertIn("storm = limit + {}".format(checks.PAST_THE_LIMIT), checks.STORM)
        self.assertIn('os.posix_spawn("/bin/sleep", ["sleep", "{}"], {{}})'.format(checks.SLEEP_SECONDS), checks.STORM)
        self.assertGreaterEqual(checks.PAST_THE_LIMIT, 100)
        compile(checks.STORM, "storm", "exec")

    def test_the_storm_ends_inside_the_shell_tools_deadline(self):
        # The shell tool's hard limit is 30 s (tqa-tools shell_timeout), and a timed-out call returns no output, so
        # the sleeps plus spawning and reaping must end well inside it (the first candidate 6 run held them 60 s).
        self.assertEqual(checks.SHELL_TIMEOUT_SECONDS, 30)
        self.assertLessEqual(checks.SLEEP_SECONDS + 8, checks.SHELL_TIMEOUT_SECONDS)
        self.assertGreaterEqual(checks.SLEEP_SECONDS, 15, "long enough for a neighbour VM to start and answer")

    def test_only_a_refused_spawn_is_tolerated(self):
        self.assertIn("if error.errno != errno.EAGAIN:\n            raise", checks.STORM)


class Verdict(unittest.TestCase):
    def test_a_storm_stopped_at_the_configured_limit_passes(self):
        self.assertEqual(checks.problems(output(), configured=256), [])

    def test_a_guest_limit_other_than_the_configured_one_fails(self):
        self.assertIn("the guest's limit is 512, the gateway configured 256",
                      checks.problems(output(max_=512, storm=612), configured=256))

    def test_a_count_past_the_limit_or_a_limit_never_reached_fails(self):
        self.assertTrue(any("past the limit" in p for p in checks.problems(output(current=300), configured=256)))
        self.assertTrue(any("did not refuse" in p for p in checks.problems(output(limited=0), configured=256)))
        self.assertTrue(any("storm of 300" in p for p in checks.problems(output(storm=300), configured=256)))

    def test_every_spawn_past_the_limit_must_be_refused(self):
        # The storm is the limit plus PAST_THE_LIMIT, so at least that many spawns fail at the limit.
        self.assertTrue(any("refused" in p for p in checks.problems(output(refused=50), configured=256)))
        self.assertEqual(checks.problems(output(refused=checks.PAST_THE_LIMIT), configured=256), [])

    def test_a_missing_report_or_an_unfinished_storm_fails(self):
        self.assertEqual(checks.problems("noise\n", configured=256),
                         ["the stormed guest printed no pids report"])
        self.assertIn("the storm's processes were not all reaped", checks.problems(output(done=False), configured=256))


if __name__ == "__main__":
    unittest.main()
