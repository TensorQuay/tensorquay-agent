"""TEST-PLAN §15.1: the PERF harnesses' own arithmetic, result records and paired comparison, without Docker."""

import json
import sys
import subprocess
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import perf_common
import perf_concurrency
import perf_io
import perf_io_bench
import perf_start


def result(identity="PERF-010", utc="2026-09-26T08:00:00Z", kernel="Linux 7.0.0", **summary):
    environment = {"kernel": kernel, "cpu": "cpu", "docker": "29.0.0", "utc": utc}
    return {"id": identity, "environment": environment, "summary": summary}


def complete_environment():
    return {field: "value" for field in perf_common.REQUIRED}


class PercentileTests(unittest.TestCase):
    def test_nearest_rank_over_fifty_samples(self):
        samples = list(range(50, 0, -1))
        self.assertEqual(perf_common.percentile(samples, 50), 25)
        self.assertEqual(perf_common.percentile(samples, 99), 50)
        self.assertEqual(perf_common.percentile(samples, 95), 48)

    def test_no_samples_is_an_error(self):
        with self.assertRaises(ValueError):
            perf_common.percentile([], 50)


class ResultTests(unittest.TestCase):
    def test_a_result_without_its_environment_is_refused(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(perf_common, "RESULTS", Path(directory)):
            for field in perf_common.REQUIRED:
                environment = complete_environment()
                del environment[field]
                with self.assertRaisesRegex(ValueError, field):
                    perf_common.write("PERF-010", environment, {}, [1], {"p50_ms": 1}, {}, "pass")
            self.assertEqual(list(Path(directory).rglob("*.json")), [])

    def test_a_result_is_written_under_its_date_with_the_after_readings(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(perf_common, "RESULTS", Path(directory)):
            path = perf_common.write("PERF-010", complete_environment(), {"samples": 1}, [1.5], {"p50_ms": 1.5},
                                     {"p50_ms_max": 3000}, "pass")
            record = json.loads(path.read_text())
            self.assertEqual(path.parent.parent, Path(directory))
            self.assertRegex(path.name, r"^perf-010-\d{6}\.json$")
            self.assertEqual(record["verdict"], "pass")
            self.assertIn("load_after", record["environment"])
            self.assertIn("io_pressure_after", record["environment"])


class CompareTests(unittest.TestCase):
    def test_a_different_host_is_not_a_pair(self):
        with self.assertRaisesRegex(ValueError, "kernel"):
            perf_common.compare(result(p50_ms=100), result(kernel="Linux 7.1.0", p50_ms=100))

    def test_runs_more_than_a_day_apart_are_not_a_pair(self):
        with self.assertRaisesRegex(ValueError, "24 h"):
            perf_common.compare(result(p50_ms=100), result(utc="2026-09-27T08:00:01Z", p50_ms=100))

    def test_different_measurements_are_not_a_pair(self):
        with self.assertRaisesRegex(ValueError, "different measurements"):
            perf_common.compare(result(p50_ms=100), result(identity="PERF-020", p50_ms=100))

    def test_more_than_ten_percent_worse_is_flagged(self):
        worse = perf_common.compare(result(p50_ms=100, p99_ms=200), result(p50_ms=111, p99_ms=218))
        self.assertEqual(len(worse), 1)
        self.assertTrue(worse[0].startswith("p50_ms: 100 -> 111"))

    def test_nine_percent_worse_is_not_flagged(self):
        self.assertEqual(perf_common.compare(result(p50_ms=100), result(p50_ms=109)), [])

    def test_throughput_is_worse_when_it_falls(self):
        worse = perf_common.compare(result(seq_read_mbps=1000, sandbox_ratio=0.9),
                                    result(seq_read_mbps=850, sandbox_ratio=1.2))
        self.assertEqual([line.split(":")[0] for line in worse], ["seq_read_mbps"])


class DockerTimeTests(unittest.TestCase):
    def test_nanosecond_and_whole_second_instants(self):
        self.assertAlmostEqual(perf_start.docker_time("2026-09-26T08:00:01.123456789Z")
                               - perf_start.docker_time("2026-09-26T08:00:01Z"), 0.123456, places=6)
        self.assertAlmostEqual(perf_start.docker_time("2026-09-26T08:00:01.5Z")
                               - perf_start.docker_time("2026-09-26T08:00:01Z"), 0.5, places=6)

    def test_an_unexpected_format_is_refused(self):
        with self.assertRaises(Exception):
            perf_start.docker_time("2026-09-26 08:00:01")


class ConcurrencyTests(unittest.TestCase):
    def test_a_missing_sequence_number_is_a_gap(self):
        self.assertEqual(perf_concurrency.gaps([1, 2, 3]), [])
        # Queued with room: below max_live by count (rows before plus the messages sent before it), yet queued.
        self.assertEqual(perf_concurrency.queued_with_room(0, 16, [False] * 16), 0)
        self.assertEqual(perf_concurrency.queued_with_room(0, 16, [True] + [False] * 15), 1)
        # 14 rows left from the last run: only the first two messages had room; the rest waited for the old ones.
        self.assertEqual(perf_concurrency.queued_with_room(14, 16, [False, True] + [True] * 14), 1)
        self.assertEqual(perf_concurrency.gaps([1, 2, 4]), ["position 3 has seq 4"])
        self.assertEqual(perf_concurrency.gaps([1, 1, 2]), ["position 2 has seq 1", "position 3 has seq 2"])
        self.assertEqual(perf_concurrency.gaps([2, 3]), ["position 1 has seq 2", "position 2 has seq 3"])

    def test_cpu_percent_is_of_the_two_cpu_allocation(self):
        self.assertAlmostEqual(perf_concurrency.cpu_percent((10.0, 0), (11.0, 1_400_000)), 70.0)
        self.assertAlmostEqual(perf_concurrency.cpu_percent((10.0, 0), (12.0, 1_000_000)), 25.0)
        with self.assertRaises(Exception):
            perf_concurrency.cpu_percent((10.0, 0), (10.0, 1))

    def test_each_run_is_twenty_steps_ending_without_a_tool_call(self):
        responses = perf_concurrency.scenario()["responses"]
        self.assertEqual(len(responses), 20)
        calls = [sum(event["type"] == "tool_call" for event in reply["events"]) for reply in responses]
        self.assertEqual(calls, [1] * 19 + [0])
        self.assertTrue(all(reply["delay_ms"] == 10 for reply in responses))

    def test_the_worst_run_decides(self):
        run = {"cpu_p95_percent": 50.0, "cpu_max_percent": 90.0, "cpu_mean_percent": 30.0,
               "rss_peak_bytes": 100, "events": 200}
        worst, result = perf_concurrency.verdict([run, dict(run, cpu_p95_percent=70.0)])
        self.assertEqual((worst["cpu_p95_percent"], result), (70.0, "fail"))
        worst, result = perf_concurrency.verdict([run, dict(run, rss_peak_bytes=1_500_000_000)])
        self.assertEqual(result, "fail")
        self.assertEqual(perf_concurrency.verdict([run, run])[1], "pass")

    def test_the_sandbox_budget_admits_every_conversation_whatever_the_stack_wrote(self):
        # The stack's budget follows the soak's rule (twice its slots), so PERF-020 raises whatever line is there.
        toml = "[sandbox]\nmax_live={}\nnetwork=\"n\"\n"
        for written in (4, 8):
            self.assertEqual(perf_concurrency.sandbox_budget(toml.format(written), 16), toml.format(16))
        # A larger budget is never lowered.
        self.assertEqual(perf_concurrency.sandbox_budget(toml.format(32), 16), toml.format(32))

    def test_the_sandbox_budget_needs_exactly_one_max_live_line(self):
        for text in ("[sandbox]\nnetwork=\"n\"\n", "[sandbox]\nmax_live=4\nmax_live=8\n"):
            with self.assertRaises(perf_concurrency.AuditError):
                perf_concurrency.sandbox_budget(text, 16)


class WorkspaceIoTests(unittest.TestCase):
    def test_the_sandbox_has_the_docker_backends_options(self):
        arguments = perf_io.sandbox_arguments("tqa-perf-io-x", "sha256:" + "0" * 64, "/tmp/w")
        for option in (["--read-only"], ["--cap-drop", "ALL"], ["--security-opt", "no-new-privileges"],
                       ["--user", "1000:1000"], ["--memory", "512m"], ["--network", "none"]):
            self.assertTrue(any(arguments[index:index + len(option)] == option for index in range(len(arguments))),
                            option)
        self.assertIn("type=bind,source=/tmp/w,target=/workspace", arguments)

    def test_ratios_are_of_the_p50s_and_every_one_must_reach_eighty_percent(self):
        host = {"seq_write_mbps": 100.0, "seq_read_mbps": 200.0, "rand_write_ops": 1000.0, "rand_read_ops": 500.0}
        rounds = [{"host": host, "sandbox": dict(host, seq_write_mbps=value)} for value in (70.0, 80.0, 90.0)]
        summary = perf_io.summarize(rounds)
        self.assertEqual(summary["seq_write_ratio"], 0.8)
        self.assertEqual(summary["rand_read_ratio"], 1.0)
        self.assertEqual(perf_io.verdict(summary), "pass")
        self.assertEqual(perf_io.verdict(dict(summary, rand_write_ratio=0.79)), "fail")

    def test_both_sides_must_run_the_same_interpreter(self):
        host = {"seq_write_mbps": 1.0, "seq_read_mbps": 1.0, "rand_write_ops": 1.0, "rand_read_ops": 1.0}
        same = [{"host": dict(host, python="3.12.3"), "sandbox": dict(host, python="3.12.3")}]
        perf_io.same_interpreter(same)
        for other in ("3.13.5", None):
            mixed = [{"host": dict(host, python="3.12.3"), "sandbox": dict(host, python=other)}]
            with self.assertRaises(Exception):
                perf_io.same_interpreter(mixed)

    def test_cpu_lists_parse_like_the_kernel_writes_them(self):
        self.assertEqual(perf_io.cpu_list("0-7"), [0, 1, 2, 3, 4, 5, 6, 7])
        self.assertEqual(perf_io.cpu_list("0-1,4,8-9\n"), [0, 1, 4, 8, 9])
        self.assertEqual(perf_io.cpu_list(""), [])

    def test_auto_pins_to_the_last_performance_core_of_a_hybrid_cpu(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "cpu_core").mkdir()
            (Path(root) / "cpu_core" / "cpus").write_text("0-7\n")
            (Path(root) / "cpu_atom").mkdir()
            (Path(root) / "cpu_atom" / "cpus").write_text("8-23\n")
            self.assertEqual(perf_io.performance_core(Path(root)), 7)

    def test_without_core_types_auto_takes_the_fastest_core_or_none(self):
        with tempfile.TemporaryDirectory() as root:
            for cpu, khz in ((0, 4700000), (1, 5400000), (2, 5400000), (3, 4700000)):
                freq = Path(root) / "system" / "cpu" / f"cpu{cpu}" / "cpufreq"
                freq.mkdir(parents=True)
                (freq / "cpuinfo_max_freq").write_text(f"{khz}\n")
            self.assertEqual(perf_io.performance_core(Path(root)), 2)
        with tempfile.TemporaryDirectory() as root:
            self.assertIsNone(perf_io.performance_core(Path(root)))

    def test_pin_choice_defaults_to_auto_and_accepts_none_or_a_number(self):
        self.assertEqual(perf_io.pin_choice("none"), None)
        self.assertEqual(perf_io.pin_choice("5"), 5)
        with patch.object(perf_io, "performance_core", return_value=7):
            self.assertEqual(perf_io.pin_choice("auto"), 7)
        with self.assertRaises(ValueError):
            perf_io.pin_choice("fast")

    def test_random_measures_default_to_a_few_seconds_of_operations(self):
        self.assertGreaterEqual(perf_io.DEFAULT_RANDOM_OPS, 16384)

    def test_a_pinned_run_puts_both_sides_on_the_same_cpu(self):
        arguments = perf_io.sandbox_arguments("tqa-perf-io-x", "sha256:" + "0" * 64, "/tmp/w", cpu=2)
        self.assertIn("--cpuset-cpus", arguments)
        self.assertEqual(arguments[arguments.index("--cpuset-cpus") + 1], "2")
        self.assertEqual(perf_io.host_command(["python3", "bench.py"], cpu=2), ["taskset", "-c", "2", "python3", "bench.py"])
        self.assertEqual(perf_io.host_command(["python3", "bench.py"], cpu=None), ["python3", "bench.py"])
        self.assertNotIn("--cpuset-cpus", perf_io.sandbox_arguments("x", "sha256:" + "0" * 64, "/tmp/w"))

    def test_the_host_interpreter_reaches_the_sandbox_read_only(self):
        arguments = perf_io.sandbox_arguments("tqa-perf-io-x", "sha256:" + "0" * 64, "/tmp/w")
        mounts = [arguments[index + 1] for index, value in enumerate(arguments) if value == "--mount"]
        interpreter = [mount for mount in mounts if "/opt/hostpy" in mount]
        self.assertTrue(interpreter)
        self.assertTrue(all(mount.endswith(",readonly") for mount in interpreter), interpreter)
        writable = [mount for mount in mounts if not mount.endswith(",readonly")]
        self.assertEqual(writable, ["type=bind,source=/tmp/w,target=/workspace"])

    def test_the_benchmark_reports_every_measure_and_leaves_nothing_behind(self):
        with tempfile.TemporaryDirectory() as directory:
            output = subprocess.run([sys.executable, perf_io_bench.__file__, directory, "--size-mib", "2",
                                     "--random-ops", "16"], capture_output=True, text=True, check=True).stdout
            result = json.loads(output)
            seconds = tuple(measure.rsplit("_", 1)[0] + "_seconds" for measure in perf_io.MEASURES)
            self.assertEqual(sorted(result), sorted(perf_io.MEASURES + seconds + ("python",)))
            self.assertEqual(result.pop("python"), sys.version.split()[0])
            self.assertTrue(all(value > 0 for value in result.values()))
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_single_phases_keep_the_file_until_the_last_and_report_their_own_measure(self):
        """PERF-012's microVM driver runs the phases one by one, acting on the host between them."""
        with tempfile.TemporaryDirectory() as directory:
            seen = {}
            for phase, measure in (("seq-write", "seq_write_mbps"), ("rand-write", "rand_write_ops"),
                                   ("seq-read", "seq_read_mbps"), ("rand-read", "rand_read_ops")):
                output = subprocess.run([sys.executable, perf_io_bench.__file__, directory, "--size-mib", "2",
                                         "--random-ops", "16", "--phase", phase],
                                        capture_output=True, text=True, check=True).stdout
                result = json.loads(output)
                self.assertGreater(result[measure], 0, phase)
                self.assertGreater(result[measure.rsplit("_", 1)[0] + "_seconds"], 0, phase)
                seen[phase] = sorted(Path(directory).iterdir())
            self.assertEqual([len(seen[phase]) for phase in ("seq-write", "rand-write", "seq-read")], [1, 1, 1])
            self.assertEqual(seen["rand-read"], [], "the last phase removes the file")



class MicroVmStartTests(unittest.TestCase):
    """PERF-011 from the person's side: a first message's start on the microVM backend, split by stage."""

    def test_the_backend_is_docker_unless_firecracker_is_asked_for(self):
        self.assertEqual(perf_start.arguments([]).backend, "docker")
        self.assertEqual(perf_start.arguments(["--backend", "firecracker"]).backend, "firecracker")
        self.assertEqual(perf_start.arguments([]).samples, 50)
        with self.assertRaises(SystemExit):
            perf_start.arguments(["--backend", "vmware"])
        with self.assertRaises(SystemExit):
            perf_start.arguments(["--samples", "0"])

    def test_a_busy_engine_is_asked_for(self):
        self.assertFalse(perf_start.arguments([]).busy_engine)
        self.assertTrue(perf_start.arguments(["--backend", "firecracker", "--busy-engine"]).busy_engine)

    def test_a_busy_engine_is_refused_for_docker_starts(self):
        # PERF-010 never keeps the engine busy: a result read as "busy" must have been.
        for argv in (["--busy-engine"], ["--backend", "docker", "--busy-engine"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit), patch("sys.stderr"):
                perf_start.arguments(argv)

    def test_the_stage_a_frame_marks(self):
        ready = {"type": "session.ready", "sandbox": {"state": "ready"}}
        self.assertEqual(perf_start.stage({"type": "session.ready", "sandbox": {"state": "creating"}}), "creating")
        self.assertEqual(perf_start.stage(ready), "ready")
        self.assertEqual(perf_start.stage({"type": "event", "event": {"type": "run.started"}}), "run.started")
        self.assertIsNone(perf_start.stage({"type": "session.ready", "sandbox": {"state": "idle"}}))
        self.assertIsNone(perf_start.stage({"type": "event", "event": {"type": "text.delta"}}))

    def test_ready_is_the_runtime_registering_its_session_in_the_vm_host_log(self):
        log = (
            "2026-10-01T02:46:51.365038298Z [anonymous-instance:fc_vcpu 0] Received a VcpuEvent::Resume message\n"
            '2026-10-01T02:46:52.500000000Z {"timestamp":"2026-10-01T02:46:52.49Z","level":"INFO",'
            '"fields":{"message":"runtime session registered"}}\n'
            '2026-10-01T02:46:53.000000000Z {"fields":{"message":"runtime session registered"}}\n')
        self.assertAlmostEqual(perf_start.registered_at(log) - perf_start.docker_time("2026-10-01T02:46:52Z"), 0.5,
                               places=6)
        with self.assertRaises(AssertionError):
            perf_start.registered_at("2026-10-01T02:46:51Z booting\n")

    def test_a_sample_streams_after_the_tasks_last_event_so_history_is_not_replayed(self):
        # A returning task's stream from 0 replays the last run's run.started, which is not this start.
        self.assertEqual(perf_start.last_seq([]), 0)
        self.assertEqual(perf_start.last_seq([{"seq": 3}, {"seq": 9}, {"seq": 7}]), 9)

    def test_the_four_stages_add_up_to_the_total(self):
        split = perf_start.vm_split(sent=100.0, created=100.4, running=100.5, ready=101.7, started=101.9)
        self.assertEqual(list(split), ["accepted_to_vm_host_created_ms", "vm_host_create_to_start_ms",
                                       "vm_host_start_to_ready_ms", "ready_to_run_started_ms"])
        self.assertAlmostEqual(sum(split.values()), 1900.0, places=6)
        self.assertAlmostEqual(split["vm_host_start_to_ready_ms"], 1200.0, places=6)

    def test_an_upload_start_ends_at_the_uploads_answer(self):
        split = perf_start.vm_split(sent=100.0, created=100.4, running=100.5, ready=101.7, started=101.9,
                                    end="upload_answered")
        self.assertEqual(list(split)[-1], "ready_to_upload_answered_ms")
        self.assertAlmostEqual(split["ready_to_upload_answered_ms"], 200.0, places=6)

    def test_out_of_order_stamps_are_refused(self):
        with self.assertRaises(AssertionError):
            perf_start.vm_split(sent=100.0, created=99.0, running=100.5, ready=101.7, started=101.9)

    def test_the_summary_keeps_both_targets(self):
        totals = [1800.0] * 25 + [2600.0] * 25
        splits = [{"a_ms": 100.0, "b_ms": 200.0}] * 50
        summary, verdict = perf_start.vm_summary(totals, splits)
        self.assertEqual((summary["p50_ms"], summary["p99_ms"]), (1800.0, 2600.0))
        self.assertEqual((summary["a_p50_ms"], summary["b_p50_ms"]), (100.0, 200.0))
        self.assertEqual((verdict, summary["owner_3s_met"]), ("pass", True))
        summary, verdict = perf_start.vm_summary([2600.0] * 50, splits)
        self.assertEqual((verdict, summary["owner_3s_met"]), ("fail", True))
        summary, verdict = perf_start.vm_summary([4964.0] * 50, splits)
        self.assertEqual((verdict, summary["owner_3s_met"]), ("fail", False))


class DockerReturningStartTests(unittest.TestCase):
    """The returning task on Docker, as a person sees it: the same events as the microVM's, its own container times."""

    def test_the_stack_runs_with_the_microvms_idle_window(self):
        text = perf_start.idle_window("[runtime]\nbackend='docker'\n[sandbox]\nmax_live=4\n[web]\n")
        self.assertEqual(text, "[runtime]\nbackend='docker'\n[sandbox]\nidle_seconds=10\nmax_live=4\n[web]\n")
        self.assertEqual(perf_start.IDLE_SECONDS, perf_start.VM_IDLE_SECONDS)
        with self.assertRaises(AssertionError):
            perf_start.idle_window("[runtime]\n")

    def test_the_split_adds_up_to_the_total(self):
        split = perf_start.docker_split(sent_wall=100.0, created=100.3, running=100.4, started=100.9)
        self.assertEqual(list(split), ["to_container_created_ms", "create_to_start_ms", "start_to_run_started_ms"])
        self.assertAlmostEqual(sum(split.values()), 900.0, places=6)

    def test_a_refused_upload_says_why(self):
        # The c14b Gate 9 run (5 Oct 2026): PERF-011's upload got a 409 whose body was never logged.
        import io
        import urllib.error

        def refuse(request, timeout):
            raise urllib.error.HTTPError(request.full_url, 409, "Conflict", {},
                                         io.BytesIO(b'{"error":{"code":"sandbox_busy"}}'))
        stack = SimpleNamespace(origin="http://gw", client=SimpleNamespace(open=refuse),
                                request=lambda path, body=None: {"id": "c1"})
        with self.assertRaisesRegex(Exception, 'HTTP 409 .*sandbox_busy'):
            perf_start.vm_upload(stack)

    def test_a_returning_sample_waits_for_the_idle_removal_then_times_the_next_message_on_a_new_container(self):
        steps = []
        stack = SimpleNamespace(runtimes={"c1": "old-container"},
                                request=lambda path, body=None: steps.append(("create", body["title"])) or {"id": "c1"},
                                delete=lambda conversation: steps.append(("delete", conversation)))

        def started(_stack, conversation, text):
            steps.append(("message", text, dict(stack.runtimes)))
            return 1000.0, 50.0, {"monotonic": 50.8, "run.started": 1000.8}

        def gone(_stack, conversation, seconds):
            steps.append(("gone", seconds))

        with patch.object(perf_start, "message_started", side_effect=started), \
                patch.object(perf_start, "vm_gone", side_effect=gone), \
                patch.object(perf_start, "container_times", return_value=(1000.2, 1000.3)):
            total, split = perf_start.returning_sample(stack)
        self.assertEqual(steps, [("create", "perf-010-returning"), ("message", "start", {"c1": "old-container"}),
                                 ("gone", perf_start.IDLE_SECONDS + perf_start.RETIRE_SECONDS),
                                 ("message", "again", {}), ("delete", "c1"), ("gone", 60)])
        self.assertAlmostEqual(total, 800.0, places=6)
        self.assertAlmostEqual(split["to_container_created_ms"], 200.0, places=6)
        self.assertAlmostEqual(split["start_to_run_started_ms"], 500.0, places=6)


if __name__ == "__main__":
    unittest.main()
