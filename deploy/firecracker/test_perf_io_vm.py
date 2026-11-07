"""TK-U-045: PERF-012's microVM driver without KVM or Docker: the scripted guest rounds, the host side's container, the
guest's answers read back, the run count waited for, and the verdict at the block image's 70 % target."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import perf_io_vm as vm  # noqa: E402

SIZES = {"size_mib": 64, "random_ops": 4096}
RUNTIME = "sha256:" + "a" * 64


def result(status, content):
    return {"event": {"type": "tool.result", "status": status, "content": content}}


def completed():
    return {"event": {"type": "run.completed"}}


class Scenario(unittest.TestCase):
    def test_the_bench_is_written_once_then_each_round_is_one_call_and_an_answer_per_phase(self):
        scenario = vm.scenario(3, SIZES, "print('bench')\n")
        responses = scenario["responses"]
        self.assertEqual(scenario["model"], "fixture")
        self.assertEqual(len(responses), 2 + 2 * 4 * 3)
        setup = responses[0]["events"][0]
        self.assertEqual((setup["type"], setup["name"]), ("tool_call", "shell"))
        self.assertIn("print('bench')\n", setup["arguments"]["command"])
        self.assertTrue(setup["arguments"]["command"].startswith("cat > " + vm.GUEST_BENCH + " <<'"))
        for index in range(3):
            for number, phase in enumerate(vm.PHASES):
                at = 2 + 2 * (4 * index + number)
                call, answer = responses[at], responses[at + 1]
                command = call["events"][0]["arguments"]["command"]
                self.assertIn("python3 " + vm.GUEST_BENCH + " " + vm.GUEST_DIRECTORY, command)
                self.assertIn("--size-mib 64 --random-ops 4096 --seed {} --phase {}".format(index, phase), command)
                self.assertEqual(answer["events"][0]["type"], "text")

    def test_a_bench_holding_the_heredocs_end_is_refused(self):
        with self.assertRaises(ValueError):
            vm.scenario(1, SIZES, "x\n" + vm.HEREDOC + "\ny\n")

    def test_the_guest_bench_lives_on_the_workspace_image_it_measures(self):
        self.assertTrue(vm.GUEST_BENCH.startswith("/workspace/"))
        self.assertTrue(vm.GUEST_DIRECTORY.startswith("/workspace/"))


class Heredoc(unittest.TestCase):
    def test_the_setup_call_writes_the_real_bench_byte_for_byte(self):
        source = vm.BENCH.read_text()
        with tempfile.TemporaryDirectory() as root:
            target = Path(root) / "bench.py"
            command = vm.scenario(1, SIZES, source)["responses"][0]["events"][0]["arguments"]["command"]
            subprocess.run(["sh", "-c", command.replace(vm.GUEST_BENCH, str(target), 1)], check=True, timeout=30)
            self.assertEqual(target.read_text(), source)
            # A source that ends in a blank line, or with no newline, comes through as written (with one newline).
            for odd, written in (("a\n\n", "a\n\n"), ("b", "b\n")):
                command = vm.scenario(1, SIZES, odd)["responses"][0]["events"][0]["arguments"]["command"]
                subprocess.run(["sh", "-c", command.replace(vm.GUEST_BENCH, str(target), 1)], check=True, timeout=30)
                self.assertEqual(target.read_text(), written)

    def test_a_failed_setup_is_refused(self):
        vm.setup_done([result("ok", ""), completed()])
        for stored in ([result("error", "No space left on device")], [], [result("ok", ""), result("ok", "")]):
            with self.subTest(stored=stored), self.assertRaisesRegex(AssertionError, "not written"):
                vm.setup_done(stored)


class FakeStack:
    """The public API as the driver uses it: each message's run ends at once with the guest's answer."""

    def __init__(self, directory):
        self.env, self.directory, self.stored, self.messages = {}, directory, [], []

    def sign_in(self):
        pass

    def request(self, path, body=None):
        if path == "/api/v1/conversations":
            return {"id": "c-1"}
        self.messages.append(body["text"])
        content = "" if len(self.messages) == 1 else json.dumps(PHASE_ANSWER)
        for frame in (result("ok", content), completed()):
            self.stored.append(dict(frame, seq=len(self.stored) + 1))
        return {}

    def page(self, after):
        """At most two events after `after`, as a short replay page would answer."""
        return [frame for frame in self.stored if frame["seq"] > after][:2]


GUEST = {"seq_write_mbps": 700.0, "seq_read_mbps": 1400.0, "rand_write_ops": 3500.0, "rand_read_ops": 14000.0}
# One guest phase's answer (each phase reads only its own keys): the measures with their seconds.
PHASE_ANSWER = dict(GUEST, seq_write_seconds=0.05, rand_write_seconds=0.5, seq_read_seconds=0.05,
                    rand_read_seconds=0.3, python="3.13.5")
FLUSH_SECONDS = 0.05


class Measure(unittest.TestCase):
    def test_warmups_are_dropped_and_each_round_has_both_sides_in_alternating_order(self):
        # Pages of two events: the run count needs every page read on from the last seq seen.
        with tempfile.TemporaryDirectory() as root:
            demo = FakeStack(Path(root) / "installation")
            sides = []
            host = json.dumps(dict(GUEST, python="3.13.5"))

            flushes = []

            def host_run(arguments, **_):
                if "-c" in arguments:
                    flushes.append(arguments)
                    return subprocess.CompletedProcess(arguments, 0, json.dumps({"flush_seconds": FLUSH_SECONDS}), "")
                sides.append(("host", arguments[-1]))
                return subprocess.CompletedProcess(arguments, 0, host, "")

            options = type("Options", (), {"warmups": 1, "rounds": 2})()
            with patch.object(vm.stack, "events", lambda stack, conversation, after=0: stack.page(after)), \
                    patch.object(vm.stack, "image_id", lambda reference, env: RUNTIME), \
                    patch.object(vm.stack.compose, "required_image", lambda env, name: "tqa-runtime:test"), \
                    patch.object(vm.subprocess, "run", host_run):
                runtime, directory, rounds = vm.measure(demo, options, SIZES)
            self.assertEqual(runtime, RUNTIME)
            self.assertEqual(directory, Path(root) / "perf-io-host")
            self.assertEqual(demo.messages, ["Set up the bench"] + ["Round {} {}".format(index, phase)
                                                                    for index in range(3) for phase in vm.PHASES])
            self.assertEqual(sides, [("host", "0"), ("host", "1"), ("host", "2")])
            self.assertEqual(len(flushes), 3 * 3, "after both writes and the sequential read of every round")
            self.assertTrue(all("source={}/storage,target=/storage".format(demo.directory) in " ".join(arguments)
                                for arguments in flushes))
            self.assertEqual(len(rounds), 2, "the warm-up is dropped")
            # Each write is charged the host flush after it: 64 MiB in 0.05 + 0.05 s, 4096 writes in 0.5 + 0.05 s.
            self.assertEqual(rounds[0]["guest"], {
                "seq_write_mbps": 671.1, "rand_write_ops": 7447.3, "seq_read_mbps": 1400.0, "rand_read_ops": 14000.0,
                "host_flush_seconds": {"seq-write": 0.05, "rand-write": 0.05}, "python": "3.13.5"})
            self.assertEqual(rounds[0]["host"], dict(GUEST, python="3.13.5"))


class Rounds(unittest.TestCase):
    def test_sides_alternate_host_first_on_even_rounds(self):
        self.assertEqual([vm.order(index) for index in range(3)],
                         [("host", "guest"), ("guest", "host"), ("host", "guest")])

    def test_the_guest_answer_is_the_last_json_line_of_the_runs_last_result(self):
        answer = {"seq_write_mbps": 410.2, "seq_read_mbps": 900.0, "rand_write_ops": 3000.0,
                  "rand_read_ops": 9000.0, "python": "3.13.5"}
        stored = [result("ok", "earlier"), completed(),
                  result("ok", "noise on stdout\n" + json.dumps(answer) + "\n"), completed()]
        self.assertEqual(vm.guest_answer(stored), answer)

    def test_a_failed_or_unparsable_guest_round_fails_with_its_reason(self):
        for stored, reason in (([result("error", "timeout: tool deadline elapsed")], "timeout"),
                               ([result("ok", "no json here")], "no JSON"),
                               ([completed()], "no tool result")):
            with self.subTest(reason=reason), self.assertRaisesRegex(AssertionError, reason):
                vm.guest_answer(stored)

    def test_the_wait_counts_completed_runs_and_fails_on_a_failed_one(self):
        self.assertEqual(vm.ended_runs([completed(), result("ok", "x"), completed()]), 2)
        with self.assertRaisesRegex(AssertionError, "run failed"):
            vm.ended_runs([completed(), {"event": {"type": "run.failed", "code": "internal"}}])


class HostSide(unittest.TestCase):
    def test_the_host_round_runs_the_guests_interpreter_on_a_bind_mount_of_the_host_disk(self):
        arguments = vm.host_arguments(RUNTIME, Path("/srv/x/host"), Path("/repo/bench.py"), SIZES, 7)
        self.assertEqual(arguments[:3], ["docker", "run", "--rm"])
        for flag in ("--read-only", "--network", "--user", "--entrypoint"):
            self.assertIn(flag, arguments)
        self.assertEqual(arguments[arguments.index("--network") + 1], "none")
        self.assertEqual(arguments[arguments.index("--user") + 1], "1000:1000")
        self.assertIn("type=bind,source=/srv/x/host,target=/host", arguments)
        self.assertIn("type=bind,source=/repo/bench.py,target=/bench/perf_io_bench.py,readonly", arguments)
        tail = arguments[arguments.index(RUNTIME):]
        self.assertEqual(tail, [RUNTIME, "/bench/perf_io_bench.py", "/host", "--size-mib", "64",
                                "--random-ops", "4096", "--seed", "7"])
        self.assertEqual(arguments[arguments.index("--entrypoint") + 1], "python3")


class Verdict(unittest.TestCase):
    def rounds(self, guest_share):
        host = {"seq_write_mbps": 1000.0, "seq_read_mbps": 2000.0, "rand_write_ops": 5000.0,
                "rand_read_ops": 20000.0, "python": "3.13.5"}
        guest = {name: value * guest_share if name != "python" else value for name, value in host.items()}
        return [{"host": host, "guest": guest}]

    def test_every_ratio_must_reach_seventy_percent(self):
        summary = vm.summarize(self.rounds(0.70))
        self.assertEqual(summary["seq_write_ratio"], 0.7)
        self.assertEqual(vm.verdict(summary), "pass")
        self.assertEqual(vm.verdict(vm.summarize(self.rounds(0.69))), "fail")

    def test_both_sides_must_run_the_same_interpreter(self):
        rounds = self.rounds(1.0)
        rounds[0]["guest"]["python"] = "3.12.1"
        with self.assertRaises(ValueError):
            vm.same_interpreter(rounds)
        self.assertEqual(vm.same_interpreter(self.rounds(1.0)), "3.13.5")


if __name__ == "__main__":
    unittest.main()
