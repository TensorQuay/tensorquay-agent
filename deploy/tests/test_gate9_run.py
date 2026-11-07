"""The Gate 9 run's own mechanics that need no Docker or build: a bounded harness, the baseline's harness files, a dry
run and a candidate-only run's exit."""
import contextlib
import json
import io
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import gate9_run


class ReadyPopen(subprocess.Popen):
    """The harness process, with `bounded`'s clock started only once the child says it's ready (its signal handling
    in place). A loaded host can take longer than the whole time limit to start Python, and a SIGTERM before the
    handler exists kills the child outright, which a time-based test can't tell from the case it's checking."""

    ready = None  # the file the child writes once its handler is installed
    started = []  # each process, for its return code

    def wait(self, timeout=None):
        if timeout is not None and not getattr(self, "_readied", False):
            self._readied = True
            ReadyPopen.started.append(self)
            # Waits on the child's own event, not on a time: until it is ready, or has already exited.
            while not self.ready.exists() and self.poll() is None:
                time.sleep(0.01)
        return super().wait(timeout)


class Bounded(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        ReadyPopen.ready = self.directory / "ready"
        ReadyPopen.started = []
        popen = patch.object(gate9_run.subprocess, "Popen", ReadyPopen)
        popen.start()
        self.addCleanup(popen.stop)

    def harness(self, handler, startup=0):
        """A child that waits `startup` seconds (a slow start), installs `handler` for SIGTERM, says it's ready, then
        sleeps past any limit."""
        return [sys.executable, "-c", (
            "import signal, sys, time\n"
            "time.sleep({startup})\n"
            "{handler}\n"
            "open({ready!r}, 'w').write('ready')\n"
            "time.sleep(60)\n").format(startup=startup, handler=handler, ready=str(ReadyPopen.ready))]

    def test_a_harness_that_finishes_returns_its_exit_code(self):
        self.assertEqual(gate9_run.bounded([sys.executable, "-c", "raise SystemExit(3)"], None, None, 10), 3)

    def test_a_harness_past_its_time_gets_sigterm_to_tear_down_first(self):
        marker = self.directory / "tore-down"
        handler = ("def stop(*_):\n    open({!r}, 'w').write('yes'); sys.exit(0)\n"
                   "signal.signal(signal.SIGTERM, stop)").format(str(marker))
        # A start slower than the 1 s limit, as on a loaded host: the limit counts from the child's readiness.
        self.assertEqual(gate9_run.bounded(self.harness(handler, startup=1.5), None, None, 1), "timeout after 1 s")
        self.assertTrue(marker.is_file(), "the harness had its chance to tear down")
        self.assertEqual(ReadyPopen.started[0].returncode, 0, "it ended through its own handler")

    def test_a_harness_that_ignores_sigterm_is_killed(self):
        with patch.object(gate9_run, "TEARDOWN_SECONDS", 1):
            self.assertEqual(gate9_run.bounded(self.harness("signal.signal(signal.SIGTERM, signal.SIG_IGN)", startup=1.5),
                                               None, None, 1), "timeout after 1 s")
        self.assertEqual(ReadyPopen.started[0].returncode, -signal.SIGKILL, "killed, not ended by the SIGTERM")


class LayOver(unittest.TestCase):
    def test_the_baseline_gets_the_candidates_rust_harness_and_its_perf_module(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            baseline, candidate = root / "baseline", root / "candidate"
            for tree, text in ((baseline, "old"), (candidate, "new")):
                for relative in ("crates/tqa-core/tests/performance/report.rs", "crates/tqa-core/tests/performance.rs",
                                 "crates/tqa-testkit/src/perf.rs", "crates/tqa-testkit/src/perf_tests.rs"):
                    (tree / relative).parent.mkdir(parents=True, exist_ok=True)
                    (tree / relative).write_text(text)
            (baseline / "crates/tqa-core/tests/performance/gone.rs").write_text("old only")
            (baseline / "crates/tqa-testkit/src/lib.rs").write_text("pub mod time;\n")
            gate9_run.lay_over(baseline, candidate, ["PERF-001", "PERF-010"])
            self.assertEqual((baseline / "crates/tqa-core/tests/performance/report.rs").read_text(), "new")
            self.assertFalse((baseline / "crates/tqa-core/tests/performance/gone.rs").exists(),
                             "the harness directory is the candidate's, not a mix")
            self.assertEqual((baseline / "crates/tqa-testkit/src/lib.rs").read_text(), "pub mod time;\npub mod perf;\n")


class LayOverDependencies(unittest.TestCase):
    BASELINE = ('[package]\nname = "tqa-testkit"\n\n[dependencies]\nserde_json = "~1.0"\nfutures = "~0.3"\n\n'
                '[dev-dependencies]\ntokio-test = "~0.4"\n')
    CANDIDATE = ('[package]\nname = "tqa-testkit"\n\n[dependencies]\nserde_json = "~1.1"\nsha2 = "~0.11"\n'
                 'futures = "~0.3"\n\n[dev-dependencies]\ntokio-test = "~0.4"\ntempfile = "~3.27"\n')

    def lay(self, baseline_text, candidate_text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            baseline, candidate = root / "baseline", root / "candidate"
            for tree, text in ((baseline, "old"), (candidate, "new")):
                for relative in ("crates/tqa-core/tests/performance.rs", "crates/tqa-core/tests/performance/report.rs",
                                 "crates/tqa-testkit/src/perf.rs", "crates/tqa-testkit/src/perf_tests.rs"):
                    (tree / relative).parent.mkdir(parents=True, exist_ok=True)
                    (tree / relative).write_text(text)
            (baseline / "crates/tqa-testkit/src/lib.rs").write_text("pub mod time;\n")
            (baseline / "crates/tqa-testkit/Cargo.toml").write_text(baseline_text)
            (candidate / "crates/tqa-testkit/Cargo.toml").write_text(candidate_text)
            gate9_run.lay_over(baseline, candidate, ["PERF-001"])
            return (baseline / "crates/tqa-testkit/Cargo.toml").read_text()

    def test_the_baseline_testkit_gains_the_dependencies_the_candidates_perf_kit_needs(self):
        # 13h's testkit has no sha2; the candidate's perf.rs uses it, so without it the baseline doesn't build.
        laid = self.lay(self.BASELINE, self.CANDIDATE)
        self.assertEqual(laid, '[package]\nname = "tqa-testkit"\n\n[dependencies]\nserde_json = "~1.0"\n'
                               'futures = "~0.3"\nsha2 = "~0.11"\n\n[dev-dependencies]\ntokio-test = "~0.4"\n')

    def test_a_baseline_dependency_keeps_its_version_and_nothing_is_added_twice(self):
        laid = self.lay(self.BASELINE, self.CANDIDATE)
        self.assertIn('serde_json = "~1.0"', laid)
        self.assertEqual(laid.count("sha2"), 1)
        self.assertEqual(self.lay(laid, self.CANDIDATE), laid, "laying over twice changes nothing")
        self.assertNotIn("tempfile", laid, "only [dependencies]: the perf kit is library code")

    def test_a_baseline_that_already_builds_is_left_as_it_is(self):
        self.assertEqual(self.lay(self.CANDIDATE, self.CANDIDATE), self.CANDIDATE)


class KeepRaw(unittest.TestCase):
    def test_raw_files_are_kept_beside_the_kept_result_and_a_changed_one_fails_its_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results, keep = root / "results", root / "keep"
            results.mkdir()
            keep.mkdir()
            (results / "P-x.lag.json").write_bytes(b"[1]")
            (results / "P-x.gone.json").write_bytes(b"[2]")
            entries = [{"file": "P-x.lag.json", "sha256": gate9_run.hashlib.sha256(b"[1]").hexdigest()},
                       {"file": "P-x.gone.json", "sha256": "0" * 64},
                       {"file": "P-x.missing.json", "sha256": "0" * 64}]
            with patch.object(gate9_run, "ROOT", root):
                kept = gate9_run.keep_raw(results / "P-x.json", keep / "PERF-002-1-candidate-P-x.json",
                                          {"id": "PERF-002", "result": {"raw_files": entries}})
            self.assertEqual([(entry["path"], entry["matches"]) for entry in kept],
                             [("keep/PERF-002-1-candidate-P-x.lag.json", True),
                              ("keep/PERF-002-1-candidate-P-x.gone.json", False),
                              ("keep/PERF-002-1-candidate-P-x.missing.json", False)])
            self.assertEqual((keep / "PERF-002-1-candidate-P-x.lag.json").read_bytes(), b"[1]")


class BaselineBuiltins(unittest.TestCase):
    def test_only_baseline_jobs_take_the_engines_tags_for_their_built_ins(self):
        runner = SimpleNamespace(tags={"baseline": {}, "candidate": {}}, work=Path("/w"))
        for side, expected in (("baseline", "1"), ("candidate", None)):
            job = SimpleNamespace(side=side, identifier="PERF-020")
            env = gate9_run.Runner.environment(runner, job)
            self.assertEqual(env.get("TQA_UNPINNED_BUILTINS"), expected, side)


IMAGE = "sha256:" + "2" * 64
TAGS = "".join("{}={}\n".format(name, IMAGE) for name in ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_RUNTIME_IMAGE",
                                                          "TQA_FAKE_MODEL_IMAGE", "TQA_WORKSTATION_IMAGE"))


class DryRun(unittest.TestCase):
    def run_dry(self, *arguments, env=None):
        """A dry run over a clean checkout: every git and docker call recorded, none touching the host."""
        with tempfile.TemporaryDirectory() as directory:
            tags = Path(directory) / "tags"
            tags.write_text(TAGS)
            calls, output = [], io.StringIO()
            with patch.object(gate9_run, "git", side_effect=lambda *a, **_: calls.append(("git",) + a) or ""), \
                    patch.object(gate9_run.subprocess, "run", side_effect=lambda c, **_: calls.append(tuple(c))), \
                    patch.object(gate9_run, "checkout") as checkout, \
                    patch.object(gate9_run.tempfile, "mkdtemp") as mkdtemp, \
                    patch.object(gate9_run, "bounded") as bounded, \
                    patch.dict(os.environ, env or {}, clear=False), contextlib.redirect_stdout(output):
                gate9_run.main(["--candidate-tags", str(tags), "--dry-run"] + list(arguments))
            for started in (checkout, mkdtemp, bounded):
                started.assert_not_called()
            return calls, output.getvalue()

    def test_a_dry_run_checks_the_tree_and_images_prints_each_command_and_starts_nothing(self):
        calls, output = self.run_dry("--ids", "PERF-010", "--candidate-only")
        self.assertIn(("git", "status", "--porcelain"), calls)
        self.assertIn(("docker", "image", "inspect", IMAGE), calls)
        self.assertIn("Gate 9 dry run: 1 jobs, nothing started", output)
        self.assertIn("PERF-010 candidate round 1: python3 -B deploy/tests/perf_start.py | in the candidate (HEAD) | "
                      "images TQA_GATEWAY_IMAGE=sha256:222222222222", output)

    def test_a_dry_run_of_a_rust_number_names_its_build_directory_and_needs_its_database(self):
        with self.assertRaisesRegex(SystemExit, "step a needs DATABASE_URL"):
            self.run_dry("--ids", "PERF-002", "--candidate-only", env={"DATABASE_URL": "", "TQA_DATABASE_URL": ""})
        _, output = self.run_dry("--ids", "PERF-002", "--candidate-only",
                                 env={"DATABASE_URL": "postgres://t", "TQA_DATABASE_URL": "postgres://t",
                                      "CARGO_TARGET_DIR": "/t"})
        self.assertIn("PERF-002 candidate round 1: cargo nextest run --release -p tqa-gateway --test "
                      "perf_event_pipeline", output)
        self.assertIn("| CARGO_TARGET_DIR /t/candidate", output)

    def test_a_compared_number_without_candidate_only_still_needs_the_previous_release(self):
        with self.assertRaisesRegex(SystemExit, "needs --baseline-tags and --baseline-commit"):
            self.run_dry("--ids", "PERF-010")


class CandidateOnlyExit(unittest.TestCase):
    def test_a_reproduction_that_passed_exits_zero_and_a_failed_one_does_not(self):
        for reasons, code in (({"PERF-010": gate9_run.plan.REPRODUCTION}, None),
                              ({"PERF-010": "a candidate result's verdict is fail"}, 1)):
            runner = SimpleNamespace(trees={}, records=[], check=lambda jobs: None, prepare=lambda jobs: None,
                                     run=lambda job: {}, keep_record=lambda record: None,
                                     index=lambda: (Path("i.json"), [], reasons, []))
            options = gate9_run.arguments(["--candidate-tags", "t", "--ids", "PERF-010", "--candidate-only"])
            with self.subTest(reasons=reasons), patch.object(gate9_run, "Runner", return_value=runner), \
                    patch.dict(os.environ, {"TQA_DOCKER_TESTS": "1"}), contextlib.redirect_stdout(io.StringIO()):
                if code is None:
                    gate9_run.measure(options)
                else:
                    with self.assertRaises(SystemExit) as stopped:
                        gate9_run.measure(options)
                    self.assertEqual(stopped.exception.code, code)


class HarnessesTearDownOnSigterm(unittest.TestCase):
    def test_each_stack_harness_the_run_bounds_installs_the_sigterm_teardown_first(self):
        # bounded() sends SIGTERM past a job's time and expects the harness to remove its stack (a stopped PERF-010
        # once left a whole stack running); each installs the handler before anything else, even a refusal.
        import compose
        import perf_concurrency
        import perf_idle
        import perf_start
        for module in (perf_start, perf_concurrency, perf_idle):
            with self.subTest(module.__name__), patch.object(compose, "teardown_on_signals") as installed, \
                    patch.dict(os.environ, {}, clear=True), patch.object(sys, "argv", ["harness"]), \
                    self.assertRaises(SystemExit):
                module.main()
            installed.assert_called_once_with()


class Refusals(unittest.TestCase):
    def test_a_refusal_is_one_plain_line_not_a_traceback(self):
        with self.assertRaises(SystemExit) as stopped:
            gate9_run.main(["--candidate-tags", "x", "--steps", "d"])
        self.assertEqual(str(stopped.exception), "Gate 9 run refused: step d needs KVM: pass --kvm once the KVM run "
                                                 "is agreed")

    def test_a_run_names_steps_or_ids(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            gate9_run.main(["--candidate-tags", "x"])


if __name__ == "__main__":
    unittest.main()


class IndexOnly(unittest.TestCase):
    """The c14b run (5 Oct 2026) lost its job records when its index failed; --index-only rebuilds from kept files."""

    def test_records_are_rebuilt_from_kept_files_with_their_load_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            kept = Path(directory)
            for round_, side in ((1, "baseline"), (2, "candidate")):
                name = "PERF-020-{}-{}-perf-020-1.json".format(round_, side)
                (kept / name).write_text(json.dumps({"id": "PERF-020", "verdict": "pass", "summary": {}}))
            records = gate9_run.rebuilt(kept)
            self.assertEqual([(r["id"], r["round"], r["side"]) for r in records],
                             [("PERF-020", 1, "baseline"), ("PERF-020", 2, "candidate")])
            self.assertTrue(all(r["disturbed"] is None and r["results"][0]["verdict"] == "pass" for r in records))
            self.assertEqual(gate9_run.rebuilt(kept, {"PERF-001"}), [], "only the IDs named")

    def test_a_raw_companion_that_changed_fails_its_result(self):
        with tempfile.TemporaryDirectory() as directory:
            kept = Path(directory)
            lag = kept / "PERF-002-1-candidate-PERF-002-2026.lag.json"
            lag.write_text("[1, 2]")
            result = {"id": "PERF-002", "verdict": "pass", "raw": [{"file": "PERF-002-2026.lag.json",
                                                                 "sha256": "0" * 64}]}
            (kept / "PERF-002-1-candidate-PERF-002-2026.json").write_text(json.dumps(result))
            with patch.object(gate9_run.plan, "raw_files", return_value=result["raw"]):
                records = gate9_run.rebuilt(kept)
            self.assertEqual(records[0]["results"][0]["verdict"], "fail")

    def test_a_kept_jobs_file_is_read_as_written(self):
        with tempfile.TemporaryDirectory() as directory:
            kept = Path(directory)
            record = {"id": "PERF-001", "side": "candidate", "round": 2, "step": "a", "exit": 0, "disturbed": False,
                      "max_load": 3.0, "results": [{"path": "tests/perf/results/2026-10-05/gate9-t/x.json",
                                                    "sha256": "a" * 64,
                                                    "id": "PERF-001", "verdict": "pass", "raw": []}]}
            (kept / "jobs.jsonl").write_text(json.dumps(record) + "\n")
            records = gate9_run.rebuilt(kept)
            self.assertEqual(records[0]["disturbed"], False)
            self.assertEqual(records[0]["results"][0]["path"], str(kept.resolve() / "x.json"), "read beside jobs.jsonl")



class IndexedBaselines(unittest.TestCase):
    """The c14b retake (5 Oct 2026): 13h failed PERF-002's offered rate and one PERF-020 baseline run was disturbed."""

    def index(self, records, files, *notes):
        with tempfile.TemporaryDirectory() as directory:
            kept = Path(directory)
            for name, data in files.items():
                (kept / name).write_text(json.dumps(data))
            (kept / "jobs.jsonl").write_text("".join(json.dumps(record) + "\n" for record in records))
            tags = kept / "tags.txt"
            tags.write_text("TQA_GATEWAY_IMAGE=sha256:" + "a" * 64 + "\n")
            argv = ["--candidate-tags", str(tags), "--index-only", str(kept), "--out", str(kept / "out")]
            for note in notes:
                argv += ["--note", note]
            with contextlib.redirect_stdout(io.StringIO()) as printed:
                gate9_run.measure(gate9_run.arguments(argv))
            index = json.loads(next((kept / "out").glob("*.json")).read_text())
            return index, printed.getvalue()

    @staticmethod
    def job(identifier, side, round_, data, disturbed=False):
        name = "{}-{}-{}-r.json".format(identifier, round_, side)
        record = {"id": identifier, "side": side, "round": round_, "step": "a", "exit": 0, "disturbed": disturbed,
                  "max_load": 26.5 if disturbed else 3.0,
                  "results": [{"path": name, "sha256": "0" * 64, "id": identifier, "verdict": data["verdict"],
                               "raw": []}]}
        return record, name, data

    def build(self, jobs, *notes):
        records, files = [], {}
        for record, name, data in jobs:
            records.append(record)
            files[name] = data
        return self.index(records, files, *notes)

    def test_a_disturbed_baseline_run_is_left_out_and_the_rest_compare_indicatively(self):
        def run(cpu):
            return {"id": "PERF-020", "verdict": "pass", "summary": {"cpu_p95_percent": cpu, "rss_peak_bytes": 1e7}}
        index, _ = self.build([self.job("PERF-020", "baseline", 1, run(90.0), disturbed=True),
                               self.job("PERF-020", "candidate", 2, run(20.0)),
                               self.job("PERF-020", "candidate", 3, run(20.0)),
                               self.job("PERF-020", "baseline", 4, run(22.0))])
        compared = index["comparisons"]["PERF-020"]
        self.assertEqual(compared["cpu_p95_percent"]["baseline"], 22.0, "round 1's 90 % is left out")
        self.assertEqual(compared["excluded_baseline"],
                         "indicative: baseline round 1 disturbed, load up to 26.5 left out")

    def test_a_baseline_that_failed_every_run_is_not_measurable_and_a_note_is_kept(self):
        def run(verdict):
            return {"id": "PERF-002", "verdict": verdict, "result": {"lag_ms": {"p50": 1.0, "p99": 2.0}}}
        note = "PERF-002=baseline not measurable: candidate-only"
        index, printed = self.build([self.job("PERF-002", "baseline", 1, run("fail")),
                                     self.job("PERF-002", "candidate", 2, run("pass")),
                                     self.job("PERF-002", "candidate", 3, run("pass")),
                                     self.job("PERF-002", "baseline", 4, run("fail"))], note)
        self.assertEqual(index["comparisons"]["PERF-002"],
                         {"compared": False, "reason": "baseline not measurable: round 1 failed; round 4 failed"})
        self.assertEqual(index["notes"], {"PERF-002": "baseline not measurable: candidate-only"})
        self.assertIn("note: PERF-002: baseline not measurable: candidate-only", printed)
