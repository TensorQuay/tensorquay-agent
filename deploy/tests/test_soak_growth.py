"""SOAK-001 (PRD M5): resource growth judged per gateway lifetime of at least 4 h."""
import json
import unittest
from pathlib import Path

import soak_growth
from soak_growth import judge, lifetimes, verdict

HOUR = 3600
FIXTURES = Path(__file__).parent / "fixtures" / "soak_growth"


def series(hours, value, step=30, start=0.0, metric="gateway_rss_bytes"):
    return [{"at": start + second, metric: value(second)} for second in range(0, int(hours * HOUR) + 1, step)]


class Lifetimes(unittest.TestCase):
    def test_restart_windows_split_the_samples_and_samples_inside_one_are_dropped(self):
        samples = series(3, lambda s: 1)
        parts = lifetimes(samples, [(HOUR, HOUR + 60), (2 * HOUR, 2 * HOUR + 60)])
        self.assertEqual([(part[0]["at"], part[-1]["at"]) for part in parts],
                         [(0, HOUR - 30), (HOUR + 60, 2 * HOUR - 30), (2 * HOUR + 60, 3 * HOUR)])

    def test_no_restart_is_one_lifetime(self):
        self.assertEqual(len(lifetimes(series(1, lambda s: 1), [])), 1)


class Judge(unittest.TestCase):
    def test_a_flat_lifetime_passes(self):
        result = judge(series(12, lambda s: 100_000_000), "gateway_rss_bytes")
        self.assertTrue(result["passed"])
        self.assertAlmostEqual(result["growth"], 0.0)
        self.assertAlmostEqual(result["projected_24h"], 0.0)

    def test_warm_up_growth_does_not_count(self):
        warm = judge(series(12, lambda s: 50_000_000 + min(s, HOUR) * 10_000), "gateway_rss_bytes")
        self.assertTrue(warm["passed"], warm)

    def test_a_slow_leak_fails_on_the_slope_even_when_the_hours_look_close(self):
        # 0.3 % per hour: the second hour and the last hour differ by ~3 %, but 24 h projects to ~7 %.
        leak = judge(series(12, lambda s: 100_000_000 * (1 + 0.003 * s / HOUR)), "gateway_rss_bytes")
        self.assertLess(leak["growth"], 0.05)
        self.assertGreater(leak["projected_24h"], 0.05)
        self.assertFalse(leak["passed"])

    def test_growth_between_the_second_and_the_last_hour_fails(self):
        step = judge(series(12, lambda s: 100 if s < 11 * HOUR else 110, metric="open_fds"), "open_fds")
        self.assertAlmostEqual(step["growth"], 0.10)
        self.assertFalse(step["passed"])

    def test_a_lifetime_under_four_hours_is_not_judged(self):
        self.assertIsNone(judge(series(3.9, lambda s: 1), "gateway_rss_bytes"))


class Verdict(unittest.TestCase):
    METRICS = ("gateway_rss_bytes", "open_fds", "database_connections")

    def samples(self, hours, value, start=0.0):
        return [{"at": start + s, "gateway_rss_bytes": value(s), "open_fds": 40, "database_connections": 10}
                for s in range(0, int(hours * HOUR) + 1, 30)]

    def test_the_24_hour_run_is_judged_on_its_long_lifetime(self):
        samples = self.samples(24, lambda s: 100_000_000)
        windows = [(t, t + 60) for t in range(2 * HOUR, 12 * HOUR + 1, 2 * HOUR)]
        result = verdict(samples, windows, self.METRICS, require_judged=True)
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["judged"]), 1)
        self.assertEqual(set(result["judged"][0]["metrics"]), set(self.METRICS))
        self.assertGreaterEqual(result["judged"][0]["hours"], 11.9)

    def test_a_24_hour_run_without_a_long_lifetime_fails(self):
        samples = self.samples(24, lambda s: 1)
        windows = [(t, t + 60) for t in range(1800, 24 * HOUR, 1800)]
        result = verdict(samples, windows, self.METRICS, require_judged=True)
        self.assertFalse(result["passed"])
        self.assertIn("no gateway lifetime of 4 h", result["reason"])

    def test_the_2_hour_run_is_not_judged_and_says_so(self):
        result = verdict(self.samples(2, lambda s: 1), [], self.METRICS, require_judged=False)
        self.assertTrue(result["passed"])
        self.assertEqual(result["judged"], [])
        self.assertIn("not judged", result["reason"])

    def test_one_failing_metric_fails_the_verdict(self):
        samples = self.samples(12, lambda s: 100_000_000 * (1 + 0.01 * s / HOUR))
        self.assertFalse(verdict(samples, [], self.METRICS, require_judged=True)["passed"])

    def test_a_24_hour_run_judges_the_connection_floor_on_its_12_hour_lifetime(self):
        windows = [(t, t + 60) for t in range(2 * HOUR, 12 * HOUR + 1, 2 * HOUR)]
        result = verdict(self.samples(24, lambda s: 100_000_000), windows, self.METRICS, require_judged=True,
                         require_floor=True)
        self.assertTrue(result["passed"], result)
        self.assertNotIn("not judged", result["reason"])

    def test_a_24_hour_run_whose_last_lifetime_is_under_11_hours_fails_on_the_unjudged_floor(self):
        # An unplanned restart at hour 17 leaves a 7 h last lifetime: D3 never runs, so the run can't pass on it.
        windows = [(t, t + 60) for t in (*range(2 * HOUR, 12 * HOUR + 1, 2 * HOUR), 17 * HOUR)]
        result = verdict(self.samples(24, lambda s: 100_000_000), windows, self.METRICS, require_judged=True,
                         require_floor=True)
        self.assertFalse(result["passed"])
        self.assertIn("connection floor (D3) not judged", result["reason"])

    def test_an_8_hour_run_passes_and_says_the_floor_was_not_judged(self):
        result = verdict(self.samples(8, lambda s: 100_000_000), [], self.METRICS, require_judged=True)
        self.assertTrue(result["passed"], result)
        self.assertIn("connection floor (D3) not judged", result["reason"])


def lifetime(hours=12, fds=lambda s: 50, connections=lambda s: 5 + (s // 30) % 11, rss=lambda s: 36_000_000):
    """A lifetime (12 h: the 24 h run's judged one) whose gateway connections swing 5-15 as a loaded pool does,
    measured as the soak's probe does: its own session counts one more (soak_chaos's DATABASE_QUERY before 9 Oct)."""
    return [{"at": float(s), "gateway_rss_bytes": rss(s), "database_connections": connections(s) + 1,
             "open_fds": fds(s) + connections(s)} for s in range(0, int(hours * HOUR) + 1, 30)]


def leak(samples, fds_per_hour=0.0, held_per_hour=0.0, pool=16):
    """Fds, or held connections (each also a socket, and stopping at the pool's maximum), leaked from the start."""
    leaked = []
    for sample in samples:
        hours = sample["at"] / HOUR
        gateway = sample["database_connections"] - soak_growth.PROBE
        held = min(pool, gateway + int(held_per_hour * hours))
        leaked.append(dict(sample, database_connections=held + soak_growth.PROBE,
                           open_fds=sample["open_fds"] + int(fds_per_hour * hours) + held - gateway))
    return leaked


class OpenFds(unittest.TestCase):
    """Every DB connection is a socket, so open_fds is judged without the gateway's: the fds other than its DB
    connections, by the 5 % rule (the c15 soaks: those stayed flat every hour while the pool's swings moved
    open_fds)."""

    def test_the_pools_swings_alone_pass(self):
        self.assertTrue(soak_growth.judge_fds(lifetime())["passed"])

    def test_one_fd_leaked_an_hour_fails(self):
        self.assertFalse(soak_growth.judge_fds(leak(lifetime(), fds_per_hour=1))["passed"])

    def test_one_fd_leaked_every_two_hours_fails(self):
        self.assertFalse(soak_growth.judge_fds(leak(lifetime(), fds_per_hour=0.5))["passed"])

    def test_a_held_connection_is_a_db_fd_not_another_one(self):
        self.assertTrue(soak_growth.judge_fds(leak(lifetime(), held_per_hour=1))["passed"])


class DatabaseConnections(unittest.TestCase):
    """The pool bounds the gateway's count (the probe's own session left out), so it's judged by the bound, the
    pool's last hour and, on the 24 h run's 12 h lifetime, the floor."""

    def judge(self, samples, listeners=0):
        return soak_growth.judge_connections(samples, pool=16, listeners=listeners)

    def test_a_pool_swinging_with_the_load_passes(self):
        result = self.judge(lifetime())
        self.assertTrue(result["passed"], result)
        self.assertEqual(result["ceiling"], 16)
        self.assertEqual(result["highest"], 15, "the probe's own session isn't the gateway's")

    def test_the_probes_own_session_is_not_counted(self):
        # The c15 soaks' 17: the pool at 16 plus the probe. Their 16: the pool at 15.
        self.assertEqual(soak_growth.gateway_connections({"database_connections": 17}), 16)
        self.assertEqual(soak_growth.gateway_connections({"database_connections": 30, "gateway_connections": 9}), 9)

    def test_a_connection_outside_the_pool_fails_and_a_listener_is_allowed_for(self):
        samples = lifetime()
        samples[-10] = dict(samples[-10], database_connections=18)  # the gateway at 17
        self.assertFalse(self.judge(samples)["passed"])
        self.assertTrue(self.judge(samples, listeners=1)["passed"], "agent teams on: the pool and its PgListener")

    def test_a_pool_at_its_maximum_through_the_last_hour_fails(self):
        end = 12 * HOUR
        samples = [dict(s, database_connections=17) if s["at"] >= end - HOUR else s for s in lifetime()]
        self.assertFalse(self.judge(samples)["passed"])

    def test_a_busy_last_hour_one_short_of_the_maximum_passes(self):
        # The rule's first draft counted the probe's session with the pool, so a pool at 15 read as pinned.
        end = 12 * HOUR
        samples = [dict(s, database_connections=16) if s["at"] >= end - HOUR else s for s in lifetime()]
        result = self.judge(samples)
        self.assertEqual(result["pinned_last_window"], 0.0)
        self.assertTrue(result["passed"], result)

    def test_one_held_connection_an_hour_fails_on_the_floor(self):
        result = self.judge(leak(lifetime(), held_per_hour=1))
        self.assertFalse(result["passed"])
        self.assertGreater(result["floor_last"] - result["floor_first"], soak_growth.FLOOR_RISE)

    def test_one_held_connection_every_two_hours_fails_on_the_floor(self):
        result = self.judge(leak(lifetime(), held_per_hour=0.5))
        self.assertGreater(result["floor_last"] - result["floor_first"], soak_growth.FLOOR_RISE)
        self.assertFalse(result["passed"])

    def test_the_floor_is_not_judged_under_11_hours_and_says_so(self):
        result = self.judge(lifetime(hours=7))
        self.assertFalse(result["floor_judged"])
        self.assertIsNone(result["floor_first"])
        self.assertTrue(result["passed"], "D1 and D2 still judge it")

    def test_a_lifetime_under_four_hours_is_not_judged(self):
        self.assertIsNone(self.judge(lifetime(hours=3.9)))

    def test_scaled_keywords_reach_every_rule_as_perf_021_uses_them(self):
        hour = lifetime(hours=1)
        rule = {"warm_up": 300, "window": 600, "min_lifetime": 3000, "horizon": HOUR}
        for metric in ("gateway_rss_bytes", "open_fds", "database_connections"):
            with self.subTest(metric=metric):
                self.assertIsNotNone(soak_growth.judge_metric(hour, metric, **rule))



def idle(samples, stale=lambda s: 0, age=lambda s: 4):
    """The samples as the probe reads them from 9 Oct 2026: the gateway's connections apart (the probe's own less),
    the oldest idle one's age and how many have been idle over STALE_SECONDS (soak_chaos's DATABASE_QUERY)."""
    return [dict(sample, gateway_connections=sample["database_connections"] - soak_growth.PROBE,
                 gateway_idle_age_seconds=age(int(sample["at"])), gateway_stale_connections=stale(int(sample["at"])))
            for sample in samples]


def held_from(samples, at, count=1):
    """`count` connections taken out of the pool at `at` and never used again: the count rises by them, and from
    STALE_SECONDS after `at` they read idle beyond it (their state_change only gets older)."""
    out = []
    for sample in samples:
        if sample["at"] < at:
            out.append(sample)
            continue
        old = sample["gateway_connections"]
        new = min(16, old + count)
        held = sample["at"] - at
        out.append(dict(sample, gateway_connections=new, database_connections=new + soak_growth.PROBE,
                        open_fds=sample["open_fds"] + new - old,
                        gateway_idle_age_seconds=max(sample["gateway_idle_age_seconds"], int(held)),
                        gateway_stale_connections=sample["gateway_stale_connections"]
                        + (count if held > soak_growth.STALE_SECONDS else 0)))
    return out


class IdleAge(unittest.TestCase):
    """D4 (9 Oct 2026, the blind spot): a pool connection is reused within seconds (sqlx's idle queue is FIFO and the
    admission pass queries every second), so none stays idle for long; a held one, taken and never used again, idles
    for good. So no gateway connection may be idle over STALE_SECONDS after warm-up, beyond its listeners."""

    def judge(self, samples, listeners=0):
        return soak_growth.judge_connections(samples, pool=16, listeners=listeners)

    # TEST: SOAK-001
    def test_a_pool_reusing_its_connections_passes_and_reports_the_oldest(self):
        result = self.judge(idle(lifetime(), age=lambda s: s % 17))
        self.assertTrue(result["idle_judged"])
        self.assertEqual(result["stale_highest"], 0)
        self.assertEqual(result["idle_age_highest"], 16)
        self.assertTrue(result["passed"], result)

    # TEST: SOAK-001
    def test_one_held_connection_the_floor_misses_fails_on_its_idle_age(self):
        samples = held_from(idle(lifetime()), at=6 * HOUR)
        without_d4 = [{k: v for k, v in s.items() if not k.startswith("gateway_idle") and k != "gateway_stale_connections"}
                      for s in samples]
        self.assertTrue(self.judge(without_d4)["passed"], "D1-D3 alone pass one held connection: the blind spot")
        result = self.judge(samples)
        self.assertEqual(result["stale_highest"], 1)
        self.assertGreater(result["idle_age_highest"], 5 * HOUR)
        self.assertFalse(result["passed"])

    # TEST: SOAK-001
    def test_a_held_connection_in_a_lifetime_under_11_hours_fails_though_the_floor_isnt_judged(self):
        result = self.judge(held_from(idle(lifetime(hours=7)), at=2 * HOUR))
        self.assertFalse(result["floor_judged"])
        self.assertFalse(result["passed"], "D4 judges every lifetime the rule judges")

    # TEST: SOAK-001
    def test_the_gateways_listeners_may_idle_and_one_more_fails(self):
        samples = idle(lifetime(), stale=lambda s: 1 if s >= HOUR else 0)
        self.assertFalse(self.judge(samples)["passed"], "teams off: no connection of the gateway idles for good")
        self.assertTrue(self.judge(samples, listeners=1)["passed"], "teams on: its PgListener idles for good")
        two = idle(lifetime(), stale=lambda s: 2 if s >= HOUR else 0)
        self.assertFalse(self.judge(two, listeners=1)["passed"])

    # TEST: SOAK-001
    def test_the_warm_up_is_left_out(self):
        result = self.judge(idle(lifetime(), stale=lambda s: 1 if s < HOUR else 0))
        self.assertEqual(result["stale_highest"], 0)
        self.assertTrue(result["passed"], result)

    # TEST: SOAK-001
    def test_samples_from_before_the_idle_age_dont_judge_it_and_the_verdict_says_so(self):
        result = self.judge(lifetime())
        self.assertFalse(result["idle_judged"])
        self.assertIsNone(result["stale_highest"])
        self.assertTrue(result["passed"], "D1-D3 still judge them")
        run = verdict(lifetime(), [], ("database_connections",), require_judged=True)
        self.assertTrue(run["passed"], run)
        self.assertIn("idle age (D4) not recorded", run["reason"])
        recorded = verdict(idle(lifetime()), [], ("database_connections",), require_judged=True)
        self.assertNotIn("D4", recorded["reason"])

def past(name):
    """The judged lifetime of a past 24 h soak's samples (times from the lifetime's start; only the three metrics;
    `database_connections` as the probe measured it then, its own session included)."""
    with (FIXTURES / name).open() as lines:
        return [json.loads(line) for line in lines]


def tiled(samples, hours=12):
    """A healthy `hours`-long lifetime from a past run's own hours: its first hour (the warm-up), then its settled
    hours in turn, repeated, each re-stamped. Real noise, no trend added."""
    start = samples[0]["at"]
    by_hour = {}
    for sample in samples:
        by_hour.setdefault(int((sample["at"] - start) // HOUR), []).append(sample)
    settled = [by_hour[hour] for hour in sorted(by_hour) if hour >= 1 and len(by_hour[hour]) >= 30]
    out = [dict(sample) for sample in by_hour[0]]
    for index in range(hours - 1):
        for sample in settled[index % len(settled)]:
            offset = (sample["at"] - start) % HOUR
            out.append(dict(sample, at=start + (index + 1) * HOUR + offset))
    return out


class PastRuns(unittest.TestCase):
    """The two finished 24 h soaks whose fds and DB failed the 5 % rule on noise (6 Oct: c14b-rc's soak 4, a 4.5 h
    judged lifetime; 8 Oct: c15's, 7.0 h, stopped at 19 h). Their RSS failures were real: c15's two heap leaks."""

    RUNS = ("soak-2026-10-06.jsonl", "soak-2026-10-08.jsonl")
    METRICS = ("gateway_rss_bytes", "open_fds", "database_connections")

    def test_fds_and_connections_pass_and_rss_still_fails(self):
        for name in self.RUNS:
            with self.subTest(run=name):
                result = verdict(past(name), [], self.METRICS, require_judged=True)
                metrics = result["judged"][0]["metrics"]
                self.assertTrue(metrics["open_fds"]["passed"], metrics["open_fds"])
                self.assertTrue(metrics["database_connections"]["passed"], metrics["database_connections"])
                self.assertFalse(metrics["database_connections"]["floor_judged"], "under 11 h")
                self.assertFalse(metrics["database_connections"]["idle_judged"], "recorded before the idle age")
                self.assertFalse(metrics["gateway_rss_bytes"]["passed"])
                self.assertFalse(result["passed"])
                self.assertEqual(result["reason"], "over its limit: gateway_rss_bytes; the connection floor (D3) "
                                                   "not judged in a lifetime under 11 h; the connections' idle age "
                                                   "(D4) not recorded in its samples (from before 9 Oct 2026)")

    def test_the_5_percent_rule_on_the_raw_counts_failed_them(self):
        for name in self.RUNS:
            with self.subTest(run=name):
                samples = past(name)
                self.assertFalse(judge(samples, "open_fds")["passed"])
                self.assertFalse(judge(samples, "database_connections")["passed"])

    def test_their_hours_tiled_to_12_h_pass_with_the_floors_margin(self):
        for name in self.RUNS:
            with self.subTest(run=name):
                result = soak_growth.judge_connections(tiled(past(name)))
                self.assertTrue(result["floor_judged"])
                self.assertLess(result["floor_last"] - result["floor_first"], soak_growth.FLOOR_RISE, "margin: one")
                self.assertTrue(result["passed"], result)

    def test_five_held_over_12_h_fail_on_their_real_noise(self):
        for name in self.RUNS:
            with self.subTest(run=name):
                held = leak(tiled(past(name)), held_per_hour=5 / 12)
                self.assertFalse(soak_growth.judge_connections(held)["passed"])

    def test_an_fd_leak_on_the_8_oct_lifetime_fails(self):
        self.assertFalse(soak_growth.judge_fds(leak(past("soak-2026-10-08.jsonl"), fds_per_hour=0.5))["passed"])


if __name__ == "__main__":
    unittest.main()
