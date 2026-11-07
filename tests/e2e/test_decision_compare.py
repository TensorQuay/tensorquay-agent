"""The E2E-DECISION-008 analysis without a run: pairing, reruns, harness exclusions, the exact McNemar test, Wilson and
bootstrap intervals, and the order effect, on synthetic attempts.

Run: python3 -m unittest tests/e2e/test_decision_compare.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import decision_compare as dc  # noqa: E402


def attempt(number, task, arm, round_, position, cls="pass", tokens=100, steps=5, wall=1000, rerun_of=None,
            latency=0, fallbacks=None, with_round=True):
    record = {"task_id": task, "attempt": number, "rerun_of": rerun_of, "wall_ms": wall, "steps": steps,
              "prompt_tokens": tokens, "completion_tokens": 0, "class": cls, "arm": arm, "pair_position": position,
              "decisions": {"by_kind": {}, "fallbacks": fallbacks or {}, "engine_calls": 1 if arm == "engine" else 0,
                            "engine_latency_ms": latency}}
    if with_round:
        record["round"] = round_
    return record


def abba(tasks=("t1", "t2"), rounds=2, **overrides):
    """A complete run: per task and round both arms, control first in odd rounds and the engine first in even ones."""
    records, number = [], 0
    for task in tasks:
        for round_ in range(1, rounds + 1):
            order = ["model", "engine"] if round_ % 2 else ["engine", "model"]
            for position, arm in enumerate(order, start=1):
                number += 1
                records.append(attempt(number, task, arm, round_, position, **overrides))
    return records


class PairingTests(unittest.TestCase):
    def test_pairs_by_task_and_round(self):
        pairs, excluded = dc.pairs(abba())
        self.assertEqual(len(pairs), 4)
        self.assertEqual(excluded, {})
        self.assertEqual({(pair.task, pair.round) for pair in pairs}, {("t1", 1), ("t1", 2), ("t2", 1), ("t2", 2)})
        # The engine went first in even rounds.
        self.assertEqual({pair.round: pair.engine_first for pair in pairs if pair.task == "t1"}, {1: False, 2: True})

    def test_a_rerun_replaces_the_attempt_it_reruns(self):
        records = abba(tasks=("t1",), rounds=1)
        records[1]["class"] = "harness.provider"
        records.append(dict(attempt(3, "t1", "engine", 1, 2, cls="model.wrong_work"), rerun_of=2))
        pairs, excluded = dc.pairs(records)
        self.assertEqual(len(pairs), 1)
        self.assertEqual(pairs[0].engine["attempt"], 3)
        self.assertEqual(excluded, {})

    def test_a_harness_failure_or_a_missing_arm_drops_the_pair_and_says_why(self):
        records = abba(tasks=("t1", "t2"), rounds=1)
        records[0]["class"] = "harness.timeout"
        records = [record for record in records if not (record["task_id"] == "t2" and record["arm"] == "engine")]
        pairs, excluded = dc.pairs(records)
        self.assertEqual(pairs, [])
        # Per arm too: an arm that fails the harness more often would bias the pairs that remain.
        self.assertEqual(excluded, {"harness failure": 1, "harness failure (model)": 1, "one arm only": 1})

    def test_older_files_without_round_pair_in_schedule_order(self):
        records = abba(with_round=False)
        pairs, _ = dc.pairs(records)
        self.assertEqual(sorted((pair.task, pair.round, pair.engine_first) for pair in pairs),
                         [("t1", 1, False), ("t1", 2, True), ("t2", 1, False), ("t2", 2, True)])

    def test_a_run_without_an_engine_arm_is_refused(self):
        with self.assertRaises(SystemExit):
            dc.analyse([attempt(1, "t1", "model", 1, 1)])


class StatisticsTests(unittest.TestCase):
    def test_exact_mcnemar(self):
        self.assertEqual(dc.mcnemar_exact(0, 0), 1.0)
        self.assertAlmostEqual(dc.mcnemar_exact(0, 6), 0.03125)
        self.assertAlmostEqual(dc.mcnemar_exact(6, 0), 0.03125)
        self.assertAlmostEqual(dc.mcnemar_exact(2, 8), 0.109375)
        self.assertEqual(dc.mcnemar_exact(5, 5), 1.0)

    def test_wilson(self):
        low, high = dc.wilson(8, 10)
        self.assertAlmostEqual(low, 0.4902, places=3)
        self.assertAlmostEqual(high, 0.9433, places=3)
        self.assertEqual(dc.wilson(0, 0), (None, None))

    def test_bootstrap_is_seeded_and_brackets_the_mean(self):
        values = [1.0, 2.0, 3.0, 4.0, 10.0]
        first = dc.bootstrap_mean(values)
        self.assertEqual(first, dc.bootstrap_mean(values))
        low, high = first
        self.assertLess(low, 4.0)
        self.assertGreater(high, 4.0)
        self.assertEqual(dc.bootstrap_mean([]), (None, None))
        self.assertEqual(dc.bootstrap_mean([7.0]), (7.0, 7.0))


class AnalysisTests(unittest.TestCase):
    def test_paired_differences_are_engine_minus_control(self):
        records = abba(tasks=("t1", "t2", "t3"), rounds=2)
        for record in records:
            if record["arm"] == "engine":
                record["prompt_tokens"], record["steps"], record["wall_ms"] = 80, 4, 1200
        report = dc.analyse(records)
        self.assertEqual(report["pairs"], 6)
        self.assertEqual(report["metrics"]["tokens"]["mean_difference"], -20)
        self.assertEqual(report["metrics"]["tokens"]["interval"], [-20, -20])
        self.assertEqual(report["metrics"]["steps"]["mean_difference"], -1)
        self.assertEqual(report["metrics"]["wall_ms"]["mean_difference"], 200)

    def test_completion_counts_the_discordant_pairs(self):
        records = abba(tasks=("t1", "t2", "t3"), rounds=2)
        engine = [record for record in records if record["arm"] == "engine"]
        for record in engine[:4]:
            record["class"] = "model.wrong_work"
        report = dc.analyse(records)["completion"]
        self.assertEqual((report["model"]["passed"], report["engine"]["passed"]), (6, 2))
        self.assertEqual((report["model_only"], report["engine_only"]), (4, 0))
        self.assertAlmostEqual(report["p_value"], 0.125)

    def test_the_order_effect_shows_when_the_second_attempt_is_cheaper(self):
        """A warm cache for whichever runs second: wall time drops at position 2, whatever the arm. The paired
        difference then depends on the order, and the treatment effect (the average over both orders) stays 0."""
        records = abba(tasks=("t1", "t2", "t3", "t4"), rounds=2)
        for record in records:
            record["wall_ms"] = 1000 if record["pair_position"] == 1 else 600
        report = dc.analyse(records)
        order = report["order"]["wall_ms"]
        # Engine minus control: the engine runs second (faster) when the control goes first, and first when it doesn't.
        self.assertEqual(order["engine_first"], 400)
        self.assertEqual(order["model_first"], -400)
        self.assertEqual(order["carryover"], 800)
        self.assertEqual(report["metrics"]["wall_ms"]["mean_difference"], 0)
        self.assertEqual(report["order"]["position"]["wall_ms"], {"first": 1000, "second": 600})

    def test_engine_share_and_fallbacks(self):
        records = abba(tasks=("t1",), rounds=2, latency=250, fallbacks={"timeout": 1})
        report = dc.analyse(records)["engine"]
        self.assertEqual(report["latency_share"], 0.25)
        self.assertEqual(report["fallbacks"], {"timeout": 2})
        self.assertEqual(report["engine_calls"], 2)

    def test_too_few_pairs_makes_no_claim(self):
        text = dc.markdown(dc.analyse(abba(tasks=("t1",), rounds=2)))
        self.assertIn("Too few pairs", text)
        many = dc.markdown(dc.analyse(abba(tasks=tuple("t%d" % n for n in range(6)), rounds=2)))
        self.assertNotIn("Too few pairs", many)

    def test_reads_a_run_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / "attempts.jsonl").write_text("".join(json.dumps(record) + "\n" for record in abba()))
            self.assertEqual(dc.analyse(dc.load(run))["pairs"], 4)



def varied(records, arm, metric, values):
    """Sets `metric` on the arm's attempts from `values`, in order (a spread, so a bootstrap interval has width)."""
    chosen = [record for record in records if record["arm"] == arm]
    for record, number in zip(chosen, values):
        record[metric] = number
    return records


TWELVE = tuple("t%d" % n for n in range(6))  # 6 tasks x 2 rounds: 12 pairs


class VerdictTests(unittest.TestCase):
    """ADR-0013's rule: a saving only where its interval is wholly below zero; a completion drop beyond its interval is
    a regression whatever the saving; below 10 pairs, no claim."""

    def test_below_ten_pairs_no_claim_whatever_the_numbers(self):
        records = varied(abba(tasks=("t1", "t2")), "engine", "prompt_tokens", [10, 11, 12, 13])
        self.assertEqual(dc.analyse(records)["verdict"]["claim"], "none")

    def test_tokens_lower_on_every_pair_is_a_saving(self):
        records = varied(abba(tasks=TWELVE), "engine", "prompt_tokens", [60 + n for n in range(12)])
        found = dc.analyse(records)["verdict"]
        self.assertEqual((found["claim"], found["savings"], found["regression"]), ("saving", ["tokens"], False))
        self.assertIn("**A saving:** tokens", dc.markdown(dc.analyse(records)))

    def test_a_completion_drop_beyond_its_interval_is_a_regression_whatever_the_saving(self):
        records = varied(abba(tasks=TWELVE), "engine", "prompt_tokens", [60 + n for n in range(12)])
        for record in [record for record in records if record["arm"] == "engine"][:8]:
            record["class"] = "model.wrong_work"
        found = dc.analyse(records)["verdict"]
        self.assertEqual((found["claim"], found["regression"]), ("regression", True))
        self.assertIn("tokens", found["savings"], "the saving is still reported, but it doesn't make the claim")

    def test_one_lost_pair_is_inside_the_interval_not_a_regression(self):
        records = abba(tasks=TWELVE)
        [record for record in records if record["arm"] == "engine"][0]["class"] = "model.wrong_work"
        self.assertEqual(dc.analyse(records)["verdict"]["claim"], "no_effect")

    def test_more_wall_time_with_the_engine_is_a_cost(self):
        records = varied(abba(tasks=TWELVE), "engine", "wall_ms", [1500 + 10 * n for n in range(12)])
        found = dc.analyse(records)["verdict"]
        self.assertEqual((found["claim"], found["costs"]), ("no_effect", ["wall_ms"]))


def frame(event):
    return json.dumps({"type": "event", "seq": 1, "event": event}) + "\n"


class RouteTests(unittest.TestCase):
    def test_counts_each_routes_steps_and_tokens_from_the_event_logs(self):
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            (run / "events").mkdir()
            records = abba(tasks=("t1",), rounds=2)
            engine = [record for record in records if record["arm"] == "engine"]
            engine[0]["conversation_id"] = "c1"
            engine[1]["conversation_id"] = "c2"  # its log is missing
            log = [frame({"type": "decision", "point": "before_step", "engine": "reference", "latency_ms": 3,
                          "choice": {"kind": "call_model", "route": "local", "tools": ["read_file"]}}),
                   frame({"type": "usage", "prompt_tokens": 100, "completion_tokens": 20}),
                   frame({"type": "decision", "point": "before_step", "engine": "reference", "latency_ms": 2,
                          "choice": {"kind": "call_model", "route": "cheap", "tools": ["read_file"]}}),
                   frame({"type": "decision", "point": "after_response", "engine": "reference", "latency_ms": 1,
                          "choice": {"kind": "select", "calls": []}}),
                   frame({"type": "usage", "prompt_tokens": 50, "completion_tokens": 5})]
            (run / "events" / "c1.jsonl").write_text("".join(log))
            (run / "attempts.jsonl").write_text("".join(json.dumps(record) + "\n" for record in records))
            report = dc.analyse(dc.load(run), run)
            self.assertEqual(report["routes"], {"chosen": {"local": 1, "cheap": 1},
                                                "tokens": {"local": 120, "cheap": 55}, "missing": 1})
            text = dc.markdown(report)
            self.assertIn("| cheap | 1 | 55 |", text)
            self.assertIn("1 engine attempt(s) had no event log", text)

    def test_without_the_run_directory_no_route_section(self):
        self.assertNotIn("routes", dc.analyse(abba()))


if __name__ == "__main__":
    unittest.main()
