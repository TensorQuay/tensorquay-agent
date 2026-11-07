"""The Gate 9 measurement run's own rules, without Docker, KVM or a build: the jobs and their ABBA order, the tag
files, the quiet host and a disturbed result, the paired comparison and what may be recorded as gate evidence."""
import re
import unittest
from pathlib import Path

import gate9_plan as plan
from soak_audit import AuditError

IMAGE = "sha256:" + "1" * 64
STACK = {key: IMAGE for key in ("TQA_GATEWAY_IMAGE", "TQA_WEB_IMAGE", "TQA_RUNTIME_IMAGE", "TQA_FAKE_MODEL_IMAGE",
                                 "TQA_WORKSTATION_IMAGE")}


class Jobs(unittest.TestCase):
    def test_compared_measurements_run_abba_and_the_rest_once_on_the_candidate(self):
        jobs = plan.jobs(["a", "c"])
        self.assertEqual([(job.identifier, job.side) for job in jobs[:4]],
                         [("PERF-001", "baseline"), ("PERF-001", "candidate"), ("PERF-001", "candidate"),
                          ("PERF-001", "baseline")])
        self.assertEqual([job.round for job in jobs[:4]], [1, 2, 3, 4])
        self.assertEqual([job.identifier for job in jobs[4:12:4]], ["PERF-002", "PERF-003"])
        self.assertEqual([(job.identifier, job.side) for job in jobs[12:]], [("PERF-021", "candidate")])

    def test_steps_run_in_their_fixed_order_whatever_order_they_are_given(self):
        self.assertEqual([job.step for job in plan.jobs(["c", "b"])][0], "b")

    def test_every_compared_id_is_one_the_lead_named(self):
        self.assertEqual(plan.COMPARED, {"PERF-001", "PERF-002", "PERF-003", "PERF-010", "PERF-011", "PERF-020"})

    def test_an_unknown_step_and_the_kvm_step_without_its_flag_are_refused(self):
        with self.assertRaisesRegex(AuditError, "unknown step x"):
            plan.jobs(["x"])
        with self.assertRaisesRegex(AuditError, "step d needs KVM: pass --kvm"):
            plan.jobs(["d"])
        self.assertEqual({job.identifier for job in plan.jobs(["d"], kvm=True)}, {"PERF-011"})

    def test_the_soak_runs_once_and_is_not_compared(self):
        self.assertEqual([(job.identifier, job.side) for job in plan.jobs(["e"])], [("SOAK", "candidate")])


class OneNumber(unittest.TestCase):
    def test_ids_run_alone_in_step_order_with_their_abba(self):
        jobs = plan.jobs([], ids=["PERF-020", "PERF-003"])
        self.assertEqual([(job.step, job.identifier, job.side) for job in jobs],
                         [("a", "PERF-003", side) for side in plan.ABBA] + [("b", "PERF-020", side)
                                                                           for side in plan.ABBA])

    def test_ids_add_to_the_steps_named(self):
        self.assertEqual([job.identifier for job in plan.jobs(["c"], ids=["PERF-010"], candidate_only=True)],
                         ["PERF-010", "PERF-021"])

    def test_a_candidate_only_run_measures_each_compared_id_once_on_the_candidate(self):
        jobs = plan.jobs(["a"], candidate_only=True)
        self.assertEqual([(job.identifier, job.side, job.round) for job in jobs],
                         [(identifier, "candidate", 1) for identifier in ("PERF-001", "PERF-002", "PERF-003")])

    def test_an_unknown_id_is_refused_and_perf_011_alone_still_needs_kvm(self):
        with self.assertRaisesRegex(AuditError, "unknown ID PERF-099: the IDs are PERF-001, PERF-002"):
            plan.jobs([], ids=["PERF-099"])
        with self.assertRaisesRegex(AuditError, "step d needs KVM"):
            plan.jobs([], ids=["PERF-011"], candidate_only=True)


class RawData(unittest.TestCase):
    def test_raw_data_beside_a_result_is_not_taken_for_a_result(self):
        after = {"2026-10-02/PERF-002-2026-10-02T06-00-00Z.json", "2026-10-02/PERF-002-2026-10-02T06-00-00Z.lag.json",
                 "2026-10-02/perf-010-060000.json", "2026-10-02/perf-010-logs-060000/gateway.json"}
        self.assertEqual(plan.new_files(set(), after), ["2026-10-02/PERF-002-2026-10-02T06-00-00Z.json",
                                                        "2026-10-02/perf-010-060000.json"])

    def test_a_result_names_its_raw_files_by_plain_name_and_hash(self):
        entry = {"name": "lag", "file": "PERF-002-x.lag.json", "sha256": "a" * 64}
        self.assertEqual(plan.raw_files({"id": "PERF-002", "result": {"raw_files": [entry]}}), [entry])
        self.assertEqual(plan.raw_files({"id": "PERF-010", "summary": {}}), [])
        for bad in ({"file": "../x.json", "sha256": "a"}, {"file": "x.json"}, "x.json"):
            with self.subTest(bad=bad), self.assertRaisesRegex(AuditError, "PERF-002 names a raw file"):
                plan.raw_files({"id": "PERF-002", "result": {"raw_files": [bad]}})


class Tags(unittest.TestCase):
    def test_a_tag_file_is_image_ids_by_name_with_comments(self):
        text = "# candidate X\n\nTQA_GATEWAY_IMAGE={}\nTQA_WEB_IMAGE={}\n".format(IMAGE, IMAGE)
        self.assertEqual(plan.parse_tags(text), {"TQA_GATEWAY_IMAGE": IMAGE, "TQA_WEB_IMAGE": IMAGE})

    def test_a_line_that_is_not_an_image_id_is_refused_with_its_number(self):
        for text in ("TQA_GATEWAY_IMAGE=tqa-gateway:local\n", "junk\n", "TQA_GATEWAY_IMAGE={0}\nTQA_GATEWAY_IMAGE={0}\n"
                     .format(IMAGE)):
            with self.subTest(text=text), self.assertRaisesRegex(AuditError, "line [12]"):
                plan.parse_tags(text)

    def test_each_side_needs_the_images_its_steps_use(self):
        plan.check_tags(STACK, ["a", "b", "c"], "candidate")
        with self.assertRaisesRegex(AuditError, "the candidate tag file lacks TQA_VMM_IMAGE"):
            plan.check_tags(STACK, ["d"], "candidate")
        with self.assertRaisesRegex(AuditError, "the baseline tag file lacks TQA_GATEWAY_IMAGE"):
            plan.check_tags({}, ["b"], "baseline")
        plan.check_tags({}, ["a"], "baseline")  # step a builds from source
        with self.assertRaisesRegex(AuditError, "lacks TQA_WORKSTATION_IMAGE"):
            plan.check_tags({key: value for key, value in STACK.items() if key != "TQA_WORKSTATION_IMAGE"}, ["e"],
                            "candidate")

    def test_every_side_a_stack_step_runs_needs_the_workstation(self):
        # A gateway that takes pinned built-ins needs the Workstation image; without it the first stack job fails late.
        # Step c (PERF-021) is never compared, so its baseline runs nothing and needs no images.
        without = {key: value for key, value in STACK.items() if key != "TQA_WORKSTATION_IMAGE"}
        for step, side in (("b", "candidate"), ("b", "baseline"), ("c", "candidate"), ("d", "candidate"),
                           ("d", "baseline")):
            with self.subTest(step=step, side=side), \
                    self.assertRaisesRegex(AuditError, "the {} tag file lacks TQA_WORKSTATION_IMAGE".format(side)):
                plan.check_tags(dict(without, TQA_VMM_IMAGE=IMAGE), [step], side)

    def test_the_baseline_needs_no_soak_images(self):
        plan.check_tags(STACK, ["b", "e"], "baseline")


class Load(unittest.TestCase):
    def test_quiet_is_the_one_minute_load_under_ten(self):
        self.assertTrue(plan.quiet("9.99 30.0 40.0 2/300 1"))
        self.assertFalse(plan.quiet("10.0 1.0 1.0 2/300 1"))

    def test_a_run_is_disturbed_when_the_load_rose_above_ten_during_it(self):
        self.assertEqual(plan.disturbed([2.0, 9.9, 3.0]), (False, 9.9))
        self.assertEqual(plan.disturbed([2.0, 10.5, 3.0]), (True, 10.5))
        with self.assertRaises(AuditError):
            plan.disturbed([])


def result(identifier, verdict="pass", **result):
    return {"id": identifier, "verdict": verdict, "result": result, "environment": {"commit": "c"}}


class Metrics(unittest.TestCase):
    def test_each_compared_id_reads_its_own_figures(self):
        self.assertEqual(plan.metrics(result("PERF-001", variants={"journal": {"p50_ms": 0.2, "p99_ms": 0.9}})),
                         {"journal_p50_ms": 0.2, "journal_p99_ms": 0.9})
        self.assertEqual(plan.metrics(result("PERF-002", lag_ms={"p50": 1.0, "p99": 7.0})),
                         {"lag_p50_ms": 1.0, "lag_p99_ms": 7.0})
        three = result("PERF-003", added_total_ms={"p50": 0.4}, added_first_token_ms={"p50": 0.3})
        self.assertEqual(plan.metrics(three), {"added_total_p50_ms": 0.4, "added_first_token_p50_ms": 0.3})
        python = {"id": "PERF-020", "verdict": "pass", "summary": {"cpu_p95_percent": 22.2, "rss_peak_bytes": 9}}
        self.assertEqual(plan.metrics(python), {"cpu_p95_percent": 22.2, "rss_peak_bytes": 9})
        for identifier in ("PERF-010", "PERF-011-new-task"):
            self.assertEqual(plan.metrics({"id": identifier, "summary": {"p50_ms": 900, "p99_ms": 1200}}),
                             {"p50_ms": 900, "p99_ms": 1200})

    def test_a_result_without_a_figure_is_refused_by_name(self):
        with self.assertRaisesRegex(AuditError, "PERF-002 result lacks result.lag_ms.p99"):
            plan.metrics(result("PERF-002", lag_ms={"p50": 1.0}))


class Comparison(unittest.TestCase):
    def pair(self, identifier, baseline, candidate, **extra):
        return [dict(result(identifier, lag_ms={"p50": a, "p99": a}), **extra) for a in baseline], \
               [dict(result(identifier, lag_ms={"p50": b, "p99": b}), **extra) for b in candidate]

    def test_abba_compares_the_means_of_each_side(self):
        before, after = self.pair("PERF-002", (10.0, 12.0), (11.0, 11.0))
        figures = plan.compare("PERF-002", before, after)["lag_p50_ms"]
        self.assertEqual((figures["baseline"], figures["candidate"], figures["regression"]), (11.0, 11.0, False))
        self.assertAlmostEqual(figures["baseline_drift"], 2.0 / 11.0)

    def test_a_negative_added_baseline_is_clamped_to_zero_and_judged_on_the_floor(self):
        # 13h's PERF-003 total added p50 was -31.9 ms in the c14b run, -30.6 ms in its retake (Gate 9 on c14b-rc).
        def run(total):
            return result("PERF-003", added_total_ms={"p50": total}, added_first_token_ms={"p50": 1.0})
        before = [run(-31.0), run(-32.8)]
        figure = plan.compare("PERF-003", before, [run(0.6), run(0.8)])["added_total_p50_ms"]
        self.assertEqual((figure["baseline"], figure["regression"]), (0.0, False))
        self.assertEqual(figure["note"], "baseline added is -31.9 ms, at or under 0: not comparable as a percentage; "
                                         "clamped to 0 and judged by the absolute floor")
        figure = plan.compare("PERF-003", before, [run(1.4), run(1.6)])["added_total_p50_ms"]
        self.assertTrue(figure["regression"], "1.5 ms is over the 1 ms floor")
        self.assertNotIn("note", plan.compare("PERF-003", [run(2.0)] * 2, [run(2.0)] * 2)["added_total_p50_ms"])

    def test_a_regression_is_over_ten_percent_and_over_the_noise_floor(self):
        before, after = self.pair("PERF-002", (10.0, 10.0), (11.5, 11.5))
        self.assertTrue(plan.compare("PERF-002", before, after)["lag_p50_ms"]["regression"])
        before, after = self.pair("PERF-002", (0.1, 0.1), (0.2, 0.2))
        self.assertFalse(plan.compare("PERF-002", before, after)["lag_p50_ms"]["regression"],
                         "doubling 0.1 ms is under the 0.5 ms floor")

    def test_perf_020_compares_on_its_figures_own_floors(self):
        # The c14b Gate 9 run (5 Oct 2026) failed its index here: the ID's floor was looked up even when each figure
        # had its own, and PERF-020 has none of its own.
        def run(cpu, rss):
            return {"id": "PERF-020", "verdict": "pass", "summary": {"cpu_p95_percent": cpu, "rss_peak_bytes": rss}}
        figures = plan.compare("PERF-020", [run(20.0, 12e6), run(22.0, 12e6)], [run(21.0, 12e6), run(21.0, 13e6)])
        self.assertEqual(figures["cpu_p95_percent"]["candidate"], 21.0)
        self.assertFalse(figures["rss_peak_bytes"]["regression"], "0.5 MB is under the 10 MB floor")

    def test_an_added_time_near_zero_is_judged_on_the_floor_alone(self):
        before = [result("PERF-003", added_total_ms={"p50": -0.1}, added_first_token_ms={"p50": 0.0})] * 2
        after = [result("PERF-003", added_total_ms={"p50": 0.3}, added_first_token_ms={"p50": 2.0})] * 2
        figures = plan.compare("PERF-003", before, after)
        self.assertFalse(figures["added_total_p50_ms"]["regression"])
        self.assertTrue(figures["added_first_token_p50_ms"]["regression"])

    def test_a_side_without_two_results_is_not_compared_and_says_why(self):
        before, after = self.pair("PERF-002", (10.0,), (11.0, 11.0))
        self.assertEqual(plan.compare("PERF-002", before, after),
                         {"compared": False, "reason": "1 baseline and 2 candidate results; ABBA needs 2 of each"})

    def test_perf_011_compares_each_of_its_four_parts_separately(self):
        def part(name, value):
            return {"id": "PERF-011-" + name, "verdict": "pass", "summary": {"p50_ms": value, "p99_ms": value}}
        before = [part("first-start", 3000), part("new-task", 1000), part("returning-task", 400),
                  part("upload-new-task", 1100)] * 2
        after = [part("first-start", 3000), part("new-task", 1000), part("returning-task", 600),
                 part("upload-new-task", 1100)] * 2
        figures = plan.compare("PERF-011", before, after)
        self.assertEqual(sorted(name.split(".")[0] for name in figures if name.endswith("p50_ms")),
                         ["first-start", "new-task", "returning-task", "upload-new-task"])
        self.assertFalse(figures["new-task.p50_ms"]["regression"])
        self.assertTrue(figures["returning-task.p50_ms"]["regression"])

    def test_perf_010_compares_the_fresh_sandbox_and_the_returning_task(self):
        def part(identifier, value):
            return {"id": identifier, "verdict": "pass", "summary": {"p50_ms": value, "p99_ms": value}}
        before = [part("PERF-010", 700), part("PERF-010-returning-task", 500)] * 2
        after = [part("PERF-010", 700), part("PERF-010-returning-task", 800)] * 2
        figures = plan.compare("PERF-010", before, after)
        self.assertFalse(figures["p50_ms"]["regression"])
        self.assertTrue(figures["returning-task.p50_ms"]["regression"])

    def test_the_parts_counted_are_the_results_the_start_harness_writes(self):
        source = (Path(__file__).resolve().parent / "perf_start.py").read_text()
        for identifier in ("PERF-010", "PERF-011"):
            written = set(re.findall(r'"({}(?:-[a-z-]+)?)"'.format(identifier), source)) - {identifier + "-"}
            if identifier == "PERF-011":
                written.discard("PERF-011")  # the harness's messages name the ID; its results all have a part
            self.assertEqual(len(written), plan.PARTS[identifier], sorted(written))


def job(identifier, side="candidate", round_=1, files=(), disturbed=False, exit_code=0):
    return {"id": identifier, "side": side, "round": round_, "exit": exit_code, "disturbed": disturbed,
            "results": [dict(path="r/{}.json".format(name), sha256=name[0] * 64, id=name, verdict=verdict)
                        for name, verdict in files]}


class PlanStatesTheRule(unittest.TestCase):
    def test_the_test_plan_states_the_ten_percent_rule_and_every_noise_floor_the_run_uses(self):
        plan_text = (Path(__file__).resolve().parents[2] / "docs" / "TEST-PLAN.md").read_text()
        rule = plan_text[plan_text.index("3. Paired A/B for regressions"):plan_text.index("4. A result without")]
        rule = " ".join(rule.split())
        self.assertEqual(plan.REGRESSION, 0.10)
        self.assertIn("more than 10 % worse", rule)
        for stated in ("PERF-001 0.05 ms", "PERF-002 0.5 ms", "PERF-003 1 ms", "PERF-010/011 50 ms",
                       "PERF-020 2 percentage points of CPU", "10 MB of RSS"):
            self.assertIn(stated, rule)
        self.assertEqual(plan.FLOORS, {"PERF-001": 0.05, "PERF-002": 0.5, "PERF-003": 1.0, "PERF-010": 50.0,
                                       "PERF-011": 50.0, "cpu_p95_percent": 2.0, "rss_peak_bytes": 10_000_000})


class Recorded(unittest.TestCase):
    def test_an_id_is_recorded_only_when_every_candidate_run_passed_undisturbed(self):
        jobs = [job("PERF-020", round_=2, files=[("PERF-020", "pass")]),
                job("PERF-020", round_=3, files=[("PERF-020", "pass")]),
                job("PERF-020", "baseline", 1, files=[("PERF-020", "fail")])]
        self.assertEqual(plan.recorded(jobs), (["PERF-020=r/PERF-020.json@" + "P" * 64], {}))

    def test_a_disturbed_failed_or_crashed_candidate_run_is_not_recorded_and_says_why(self):
        cases = {
            "disturbed": ([job("PERF-021", files=[("PERF-021", "pass")], disturbed=True)],
                          "a candidate run was disturbed (load over 10)"),
            "failed": ([job("PERF-021", files=[("PERF-021", "fail")])], "a candidate result's verdict is fail"),
            "crashed": ([job("PERF-021", exit_code=1)], "a candidate run exited 1 without a result"),
            # Rebuilt from kept results (--index-only): the load is unknown, so it's never taken as undisturbed.
            "load lost": ([job("PERF-021", files=[("PERF-021", "pass")], disturbed=None)],
                          "a candidate run's load record was lost, so it can't be shown undisturbed"),
        }
        for name, (jobs, reason) in cases.items():
            with self.subTest(name):
                self.assertEqual(plan.recorded(jobs), ([], {"PERF-021": reason}))

    def test_a_candidate_only_run_records_no_compared_id_and_says_it_is_a_reproduction(self):
        jobs = [job("PERF-010", files=[("PERF-010", "pass")]), job("PERF-021", files=[("PERF-021", "pass")])]
        self.assertEqual(plan.recorded(jobs, candidate_only=True),
                         (["PERF-021=r/PERF-021.json@" + "P" * 64], {"PERF-010": plan.REPRODUCTION}))

    def test_a_failed_candidate_only_run_gives_its_failure_not_the_reproduction_reason(self):
        self.assertEqual(plan.recorded([job("PERF-010", files=[("PERF-010", "fail")])], candidate_only=True),
                         ([], {"PERF-010": "a candidate result's verdict is fail"}))

    def test_one_soak_result_stands_for_soak_001_to_004(self):
        soak = job("SOAK", files=[("SOAK", "pass")])
        entries, _ = plan.recorded([soak])
        self.assertEqual(entries, ["SOAK-00{}=r/SOAK.json@{}".format(n, "S" * 64) for n in range(1, 5)])


class SoakVerdict(unittest.TestCase):
    def test_a_soak_passes_only_as_gate_evidence(self):
        self.assertEqual(plan.soak_verdict({"status": "passed", "gate_evidence": True}), "pass")
        self.assertEqual(plan.soak_verdict({"status": "passed", "gate_evidence": False}), "fail")
        self.assertEqual(plan.soak_verdict({"status": "failed", "gate_evidence": True}), "fail")


class Commands(unittest.TestCase):
    def test_a_rust_measurement_runs_its_one_ignored_test_in_release(self):
        self.assertEqual(plan.rust_command("PERF-002", build=False), [
            "cargo", "nextest", "run", "--release", "-p", "tqa-gateway", "--test", "perf_event_pipeline",
            "--run-ignored", "all", "--no-capture", "-E", "test(=perf_002_event_pipeline_throughput)"])
        self.assertIn("--no-run", plan.rust_command("PERF-001", build=True))

    def test_a_stack_measurement_runs_its_harness_and_the_soak_its_gate_settings(self):
        self.assertEqual(plan.python_command("PERF-011", None), ["python3", "-B", "deploy/tests/perf_start.py",
                                                                 "--backend", "firecracker"])
        self.assertEqual(plan.python_command("SOAK", "/w/soak"), [
            "python3", "-B", "deploy/tests/soak.py", "--hours", "24", "--conversations", "8", "--seed", "731",
            "--output", "/w/soak"])

    def test_the_baseline_gets_the_candidates_harness_files_over_its_own_code(self):
        paths = plan.overlay(["PERF-001", "PERF-003"])
        self.assertIn("crates/tqa-core/tests/performance", paths)
        self.assertIn("crates/tqa-gateway/tests/perf_relay.rs", paths)
        self.assertIn("crates/tqa-testkit/src/perf.rs", paths)
        self.assertNotIn("crates/tqa-gateway/tests/perf_event_pipeline.rs", paths)
        self.assertEqual(plan.overlay(["PERF-010"]), [], "stack harnesses run from the candidate with baseline images")

    def test_the_testkit_gains_its_perf_module_once(self):
        self.assertEqual(plan.with_perf_module("pub mod time;\n"), "pub mod time;\npub mod perf;\n")
        self.assertEqual(plan.with_perf_module("pub mod perf;\npub mod time;\n"), "pub mod perf;\npub mod time;\n")

    def test_new_result_files_are_the_ones_a_job_added(self):
        before = {"2026-10-01/a.json"}
        after = before | {"2026-10-01/b.json", "2026-10-01/perf-020-logs-1/x.json", "2026-10-01/gate.log"}
        self.assertEqual(plan.new_files(before, after), ["2026-10-01/b.json"])


if __name__ == "__main__":
    unittest.main()
