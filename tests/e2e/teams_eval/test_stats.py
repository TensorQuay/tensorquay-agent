"""The agent-teams evaluation's numbers without a run: Wilson and paired bootstrap intervals, per-task pairing, the
kinds, and the proposed bars, on synthetic attempts.

Run: python3 -m unittest tests/e2e/teams_eval/test_stats.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import stats  # noqa: E402


def attempt(task, kind, side, passed, score, tokens=1000, wall=60.0, failures=(), helpers=0):
    return stats.Attempt(task, kind, side, passed, score, tokens, wall, tuple(failures), helpers)


def pair(task, kind, single_scores, team_scores, team_wall=60.0, failures=(), helpers=2):
    out = [attempt(task, kind, "single", True, score) for score in single_scores]
    out += [attempt(task, kind, "team", True, score, wall=team_wall, failures=failures, helpers=helpers)
            for score in team_scores]
    return out


class Intervals(unittest.TestCase):
    def test_wilson_matches_its_textbook_values(self):
        low, high = stats.wilson(48, 50)
        self.assertAlmostEqual(low, 0.865, places=3)
        self.assertAlmostEqual(high, 0.989, places=3)
        self.assertEqual(stats.wilson(0, 0), (None, None))

    def test_the_bootstrap_is_seeded_and_brackets_the_mean(self):
        values = [1.0, 2.0, 0.5, 1.5, 3.0, -0.5, 2.0, 1.0]
        first = stats.paired_bootstrap(values)
        self.assertEqual(first, stats.paired_bootstrap(values), "a report is reproducible")
        self.assertLess(first[0], stats.mean(values))
        self.assertGreater(first[1], stats.mean(values))
        self.assertEqual(stats.paired_bootstrap([]), (None, None))
        self.assertEqual(stats.paired_bootstrap([2.0, 2.0, 2.0]), (2.0, 2.0))


class Pairing(unittest.TestCase):
    def test_each_task_pairs_its_sides_means_and_drops_a_task_with_one_side(self):
        records = pair("R1", "research", [4, 6, 8], [7, 8, 9]) + [attempt("C9", "code", "single", True, 5)]
        found = stats.differences(records, stats.score_metric)
        self.assertEqual(found, {"R1": 2.0}, "team 8 minus single 6; C9 has one side")

    def test_an_unjudged_attempt_is_left_out_of_its_task_mean(self):
        records = pair("R1", "research", [4, 6], [8, 8]) + [attempt("R1", "research", "single", True, None)]
        self.assertEqual(stats.differences(records, stats.score_metric), {"R1": 3.0})

    def test_a_task_without_a_check_counts_for_scores_not_pass_rates(self):
        records = [attempt("R7", "research", side, None, 6) for side in stats.SIDES]
        self.assertEqual(stats.differences(records, stats.passed_metric), {})
        self.assertEqual(stats.differences(records, stats.score_metric), {"R7": 0.0})

    def test_a_kind_filter_keeps_only_its_tasks(self):
        records = pair("R1", "research", [5], [7]) + pair("C1", "code", [5], [4])
        self.assertEqual(stats.differences(records, stats.score_metric, ("code",)), {"C1": -1.0})


class Bars(unittest.TestCase):
    def split_work(self, gain, team_wall=60.0, failures=()):
        records = []
        for index in range(10):
            kind = "research" if index < 5 else "report"
            records += pair(f"T{index}", kind, [5, 5, 5], [5 + gain] * 3, team_wall=team_wall, failures=failures)
        return records

    def test_a_clear_gain_at_little_cost_is_recommended_for_split_work(self):
        verdict = stats.bars(self.split_work(2))
        self.assertEqual(verdict["verdict"], "recommend for split work", verdict["reasons"])

    def test_no_gain_keeps_it_experimental_and_says_why(self):
        verdict = stats.bars(self.split_work(0))
        self.assertEqual(verdict["verdict"], "keep experimental")
        self.assertIn("the team's mean judged score is not higher", verdict["reasons"])

    def test_a_slow_team_keeps_it_experimental(self):
        verdict = stats.bars(self.split_work(2, team_wall=100.0))
        self.assertIn("the team costs more than 1.5x the wall time", verdict["reasons"])

    def test_a_frequent_team_only_failure_keeps_it_experimental(self):
        verdict = stats.bars(self.split_work(2, failures=("helper_never_reported",)))
        self.assertTrue(any("helper_never_reported" in reason for reason in verdict["reasons"]), verdict)

    def test_the_score_bar_is_paired_so_a_missing_attempt_does_not_tilt_it(self):
        # Unpaired, the team looks higher (5.0 against 3.5); paired per task it is +1 and -1, no gain.
        records = pair("T1", "research", [8], [9]) + pair("T2", "research", [2, 2, 2], [1])
        self.assertIn("the team's mean judged score is not higher", stats.bars(records)["reasons"])

    def test_the_wall_ratio_is_paired_per_task(self):
        records = [attempt("T1", "research", "single", True, 5, wall=60.0)]
        records += [attempt("T1", "research", "team", True, 5, wall=60.0) for _ in range(3)]
        self.assertEqual(stats.bars(records)["wall_ratio"], 1.0, "three team attempts of 60 s against one of 60 s")

    def test_unjudged_attempts_make_no_claim(self):
        records = [attempt("R1", "research", side, True, None) for side in stats.SIDES]
        self.assertIn("not judged yet", stats.bars(records)["reasons"])


class Analysis(unittest.TestCase):
    def test_the_lead_alone_share_counts_team_attempts_without_helpers(self):
        records = pair("R1", "research", [5], [6, 6], helpers=0) + pair("R2", "research", [5], [6, 6])
        self.assertEqual(stats.analyse(records)["lead_alone_share"], 0.5)

    def test_each_kind_and_split_work_are_compared_on_their_own(self):
        records = pair("R1", "research", [5], [7]) + pair("P1", "report", [5], [9]) + pair("C1", "code", [5], [3])
        found = stats.analyse(records)
        self.assertEqual(found["by_kind"]["code"]["score"]["mean"], -2.0)
        self.assertEqual(found["split"]["score"]["per_task"], {"P1": 4.0, "R1": 2.0})
        self.assertEqual(found["overall"]["score"]["tasks"], 3)


class Groups(unittest.TestCase):
    def test_short_dependent_and_long_are_each_summarised_and_compared_alone(self):
        records = (pair("R1", "research", [5], [7]) + pair("C1", "code", [5], [3])
                   + pair("D1", "dependent", [4], [8]) + pair("L1", "long", [6], [6]))
        found = stats.analyse(records)["by_group"]
        self.assertEqual(found["short"]["comparison"]["score"]["per_task"], {"C1": -2.0, "R1": 2.0})
        self.assertEqual(found["dependent"]["comparison"]["score"]["mean"], 4.0)
        self.assertEqual(found["long"]["sides"]["team"]["attempts"], 1)
        self.assertEqual(found["short"]["sides"]["single"]["attempts"], 2, "R1 and C1, not D1 or L1")
        self.assertEqual(found["short"]["sides"]["team"]["pass_interval"], stats.wilson(2, 2))

    def test_a_side_summary_gives_medians_per_attempt_and_of_the_passes(self):
        records = [attempt("D1", "dependent", "team", True, 5, tokens=100, wall=60.0),
                   attempt("D1", "dependent", "team", False, 5, tokens=400, wall=600.0),
                   attempt("D2", "dependent", "team", True, 5, tokens=300, wall=120.0),
                   attempt("R1", "research", "team", True, 5, tokens=9_000, wall=9_000.0)]
        summary = stats.side_summary(records, "team", ("dependent",))
        self.assertEqual(summary["median_tokens"], 300)
        self.assertEqual(summary["median_wall_secs"], 120.0)
        self.assertEqual(summary["median_wall_secs_passed"], 90.0, "60 and 120: the failed 600 is left out")
        self.assertEqual(summary["tokens"], 800, "the sum stays, over the kinds asked for")
        self.assertIsNone(stats.side_summary(records, "single")["median_tokens"])

    def test_the_token_ratio_is_paired_per_task_as_the_wall_ratio_is(self):
        records = [attempt("T1", "dependent", "single", True, 5, tokens=1000)]
        records += [attempt("T1", "dependent", "team", True, 5, tokens=2000) for _ in range(3)]
        records += [attempt("T2", "dependent", "team", True, 5, tokens=50_000)]
        self.assertEqual(stats.token_ratio(records), 2.0, "T2 has no single side, so it isn't paired")
        self.assertEqual(stats.analyse(records)["by_group"]["dependent"]["token_ratio"], 2.0)
        self.assertIsNone(stats.analyse(records)["by_group"]["long"]["token_ratio"])

    def test_the_team_measures_add_up_per_group(self):
        records = [stats.Attempt("D1", "dependent", "team", True, 5, 100, 60.0, (), 2, 1, 3, 1),
                   stats.Attempt("D2", "dependent", "team", True, 5, 100, 60.0, (), 3, 0, 2, 0)]
        summary = stats.side_summary(records, "team", ("dependent",))
        self.assertEqual((summary["take_conflicts"], summary["waits"], summary["yields"], summary["mean_helpers"]),
                         (1, 5, 1, 2.5))


if __name__ == "__main__":
    unittest.main()
