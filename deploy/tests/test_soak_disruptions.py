"""SOAK-001…004: the seeded plan of gateway kills, database faults, sandbox kills and link cuts (H3)."""
import unittest

from soak_chaos import restart_times, schedule
from soak_disruptions import GLOBAL, KINDS, RECOVERY, backend_kinds, disruptions

DAY, HOUR = 86400, 3600


def windows(plan):
    return [(fault["at"], fault["at"] + fault.get("seconds", 0) + RECOVERY[fault["kind"]]) for fault in plan]


def overlaps(first, second):
    return first[0] < second[1] and second[0] < first[1]


class Plan(unittest.TestCase):
    def plan(self, seconds=DAY, rehearsal=False, seed=731):
        return disruptions(seed, seconds, rehearsal, 8, restart_times(seconds, rehearsal))

    def test_the_same_seed_gives_the_same_plan(self):
        self.assertEqual(self.plan(), self.plan())
        self.assertNotEqual(self.plan(), self.plan(seed=732))

    def test_every_kind_occurs_in_every_run_length(self):
        for seconds, rehearsal in ((DAY, False), (2 * HOUR, False), (300, True)):
            with self.subTest(seconds=seconds, rehearsal=rehearsal):
                self.assertEqual({fault["kind"] for fault in self.plan(seconds, rehearsal)}, set(KINDS))

    def test_the_24_hour_rates_follow_the_plan(self):
        counts = {kind: sum(fault["kind"] == kind for fault in self.plan()) for kind in KINDS}
        self.assertEqual(counts["gateway_kill"], 6)
        self.assertEqual(counts["db_restart"], 8)
        self.assertEqual(counts["db_outage"], 4)
        self.assertEqual(counts["db_crash"], 2)
        self.assertGreaterEqual(counts["sandbox_kill"], 90)
        self.assertGreaterEqual(counts["link_cut"], 40)
        self.assertGreaterEqual(counts["engine_stall"], 15)
        self.assertEqual(counts["disk_full"], 6)
        actions = {fault["action"] for fault in self.plan() if fault["kind"] == "engine_stall"}
        self.assertEqual(actions, {"hold", "error", "drop"})

    def test_gateway_kills_stay_in_hours_0_to_12_so_the_last_12_hours_are_one_lifetime(self):
        kills = [fault for fault in self.plan() if fault["kind"] == "gateway_kill"]
        self.assertTrue(all(fault["at"] + RECOVERY["gateway_kill"] < 12 * HOUR for fault in kills))

    def test_global_faults_never_overlap_each_other_or_a_restart(self):
        restarts = [(at - 120, at + 180) for at in restart_times(DAY, False)]
        global_windows = windows([fault for fault in self.plan() if fault["kind"] in GLOBAL])
        for index, window in enumerate(global_windows):
            self.assertFalse(any(overlaps(window, restart) for restart in restarts), window)
            self.assertFalse(any(overlaps(window, other) for other in global_windows[index + 1:]), window)

    def test_slot_faults_never_overlap_a_global_fault_or_each_other_in_one_slot(self):
        plan = self.plan()
        global_windows = windows([fault for fault in plan if fault["kind"] in GLOBAL])
        slot_faults = [fault for fault in plan if fault["kind"] not in GLOBAL]
        for fault in slot_faults:
            self.assertIn(fault["target"], range(8))
            window = windows([fault])[0]
            self.assertFalse(any(overlaps(window, other) for other in global_windows), fault)
            same = [other for other in slot_faults if other is not fault and other["target"] == fault["target"]]
            self.assertFalse(any(overlaps(window, other) for other in windows(same)), fault)

    def test_durations_are_bounded(self):
        for fault in self.plan():
            if fault["kind"] == "db_outage":
                self.assertTrue(60 <= fault["seconds"] <= 180)
            elif fault["kind"] == "link_cut":
                self.assertTrue(5 <= fault["seconds"] <= 60)
            elif fault["kind"] == "disk_full":
                self.assertTrue(60 <= fault["seconds"] <= 120)
            elif fault["kind"] == "engine_stall":
                self.assertTrue(60 <= fault["seconds"] <= 120)
                self.assertTrue(20 <= fault["hold"] <= min(90, fault["seconds"]))
            else:
                self.assertNotIn("seconds", fault)


class Backends(unittest.TestCase):
    def test_a_microvm_run_plans_only_the_kinds_that_mean_the_same_there(self):
        kinds = backend_kinds("firecracker")
        self.assertNotIn("link_cut", kinds)
        self.assertNotIn("disk_full", kinds)
        self.assertEqual(backend_kinds("docker"), KINDS)
        plan = disruptions(731, 8 * HOUR, False, 8, restart_times(8 * HOUR, False), kinds=kinds)
        self.assertEqual({fault["kind"] for fault in plan}, set(kinds))

    def test_the_8_hour_run_restarts_every_2_hours_until_hour_4_then_leaves_one_4_hour_lifetime(self):
        self.assertEqual(restart_times(8 * HOUR, False), [7200, 14400])
        kills = [fault for fault in disruptions(731, 8 * HOUR, False, 8, restart_times(8 * HOUR, False))
                 if fault["kind"] == "gateway_kill"]
        self.assertTrue(kills and all(fault["at"] + RECOVERY["gateway_kill"] < 4 * HOUR for fault in kills))


class WithPauses(unittest.TestCase):
    def test_pauses_keep_clear_of_every_disruption(self):
        restarts = restart_times(DAY, False)
        plan = disruptions(731, DAY, False, 8, restarts)
        faults = schedule(731, DAY, slots=8, restarts=restarts, disruptions=plan)
        global_windows = windows([fault for fault in plan if fault["kind"] in GLOBAL])
        for fault in faults:
            if fault["kind"] != "runtime_link":
                continue
            pause = (fault["at"], fault["at"] + fault["pause_seconds"] + 60)
            self.assertFalse(any(overlaps(pause, window) for window in global_windows), fault)
            same = windows([other for other in plan if other["kind"] not in GLOBAL
                            and other["target"] == fault["target"]])
            self.assertFalse(any(overlaps(pause, window) for window in same), fault)
        kinds = {fault["kind"] for fault in faults}
        self.assertTrue(set(KINDS) <= kinds, "the disruptions are part of the schedule")
        self.assertGreater(sum(fault["kind"] == "runtime_link" for fault in faults), 8 * 6)

    def test_without_disruptions_the_plan_is_the_old_one(self):
        self.assertEqual(schedule(731, 7200), schedule(731, 7200, disruptions=[]))


if __name__ == "__main__":
    unittest.main()
