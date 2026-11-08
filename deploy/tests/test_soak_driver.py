"""SOAK-001…003: rehearsal labels, run lengths, conversation generations, finite drains and receipt owners."""
import json
import time
import types
from pathlib import Path
import queue
import tempfile
import unittest
from unittest import mock

import soak
import soak_stack
from soak_disruptions import backend_kinds
from soak import (check_rotate_after, covered, disk_recovery, io_recovery, journal_cap, link_witnessed, log_windows,
                  needs_generation, planned_kinds, probe_outcome, probe_slot, probe_verdict, replacement_times,
                  replacements_on_full_disk, run_seconds, store_events, too_many_store_events, wait_for_idle,
                  slot_action, check_own_threads, MAX_DRIVER_THREADS)
from soak_scenario import RESPONSES
from soak_audit import AuditError, ReceiptLog
import soak_audit
from soak_stream import SseClient


class Immediate:
    """An executor that runs each sample at once, so a test sees its outcome on the next measure()."""

    def submit(self, function):
        import concurrent.futures
        future = concurrent.futures.Future()
        try:
            future.set_result(function())
        except BaseException as error:  # noqa: BLE001 - the sample's own failure is what the test records
            future.set_exception(error)
        return future


class DriverTests(unittest.TestCase):
    def test_rehearsal_cannot_be_misreported_as_two_hour_evidence(self):
        self.assertEqual(run_seconds(2, False), 7200)
        self.assertEqual(run_seconds(2, True), 300)
        for hours in (0, -1, float("nan"), float("inf"), 0.5, 3, 25):
            with self.subTest(hours=hours), self.assertRaises(AuditError):
                run_seconds(hours, False)

    def test_the_24_hour_run_is_gate_evidence_and_its_rehearsal_is_not(self):
        self.assertEqual(run_seconds(24, False), 86400)
        self.assertEqual(run_seconds(24, True), 300)

    def test_the_8_hour_microvm_run_is_gate_evidence(self):
        self.assertEqual(run_seconds(8, False), 8 * 3600)

    def test_on_a_microvm_a_link_pause_is_witnessed_without_the_runtime_log(self):
        self.assertTrue(link_witnessed("firecracker", gateway_closed=True, runtime_logged=False, snapshots_grew=True))
        self.assertFalse(link_witnessed("firecracker", gateway_closed=True, runtime_logged=False, snapshots_grew=False))
        self.assertFalse(link_witnessed("firecracker", gateway_closed=False, runtime_logged=True, snapshots_grew=True))
        self.assertTrue(link_witnessed("docker", gateway_closed=True, runtime_logged=True, snapshots_grew=True))
        self.assertFalse(link_witnessed("docker", gateway_closed=True, runtime_logged=False, snapshots_grew=True))

    def link_cut_driver(self, frames_after_restore):
        """A driver mid link_cut: a 31 s cut (shorter than the gateway's 90 s link liveness bound, so the link survives
        it and the runtime logs no disconnect), with a slow run in flight; its clients take no new snapshot."""
        clock, wall = [1000.0], [5000.0]
        driver = soak.Soak.__new__(soak.Soak)
        receipts = [types.SimpleNamespace(last_seq=5479, last_ts=4999.0) for _ in range(2)]
        driver.clients = [types.SimpleNamespace(snapshots=66, receipts=receipt) for receipt in receipts]
        driver.conversations = ["conversation"]
        driver.stack = types.SimpleNamespace(restore_link=lambda conversation: {"n": "192.0.2.43"},
                                             logs=lambda container, since: "runtime session registered\n")
        driver.logged = []
        driver.log = lambda kind, **fields: driver.logged.append((kind, fields))
        fault = {"kind": "link_cut", "target": 0, "restored": None, "resume_at": 1031.0, "deadline": 1031.0 + 180,
                 "timestamp": 4969.0, "cut": {"container": "c", "networks": {"n": "192.0.2.43"}}}
        driver.pending = [fault]
        patches = (mock.patch.object(soak.time, "monotonic", lambda: clock[0]),
                   mock.patch.object(soak.time, "time", lambda: wall[0]))

        def advance(seconds):
            clock[0] += seconds
            wall[0] += seconds
            if frames_after_restore and fault["restored"] is not None:
                for receipt in receipts:  # the run keeps streaming about one fragment a second
                    receipt.last_seq += int(seconds)
                    receipt.last_ts = wall[0]
        return driver, fault, patches, advance

    def test_a_short_cut_with_a_slow_run_is_witnessed_by_the_frames_after_the_restore(self):
        # R2 on candidate11: a 31 s cut; the model call was retried and the run kept streaming, but no disconnect was
        # logged and no snapshot taken, so the fault went unwitnessed. Frames past the cursor, stamped after the restore,
        # came from the runtime over the restored link.
        driver, fault, (monotonic, wall), advance = self.link_cut_driver(frames_after_restore=True)
        with monotonic, wall:
            advance(31)
            driver.witnesses()  # restores the link; nothing new yet
            self.assertEqual(driver.pending, [fault])
            self.assertEqual(fault["cursor"], 5479)
            advance(5)
            driver.witnesses()
        self.assertEqual(driver.pending, [])
        self.assertIn(("fault_witness", {"fault_kind": "link_cut", "target": 0}), driver.logged)

    def test_a_cut_after_which_no_frame_arrives_still_fails_at_its_deadline(self):
        driver, fault, (monotonic, wall), advance = self.link_cut_driver(frames_after_restore=False)
        with monotonic, wall:
            advance(31)
            driver.witnesses()
            advance(179)
            driver.witnesses()
            self.assertEqual(driver.pending, [fault], "still waiting inside the deadline")
            advance(2)
            with self.assertRaisesRegex(AuditError, "no reconnect/replay witness"):
                driver.witnesses()

    def test_a_witness_window_does_not_run_while_the_harness_is_blocked_in_its_own_engine_command(self):
        # R2 on candidate 13e, at 73 min: the harness's own `docker kill` took 83.4 s on a saturated disk, and a pending
        # link fault's witness deadline ran out meanwhile, though nothing could be witnessed while the loop was blocked.
        # The time the harness spends in a slow engine command is added to every pending fault's deadline.
        driver, fault, (monotonic, wall), advance = self.link_cut_driver(frames_after_restore=False)
        with monotonic, wall:
            advance(31)
            driver.witnesses()  # the link is restored; its witness window (180 s from the resume) runs
            advance(100)
            driver.witnesses()
            advance(200)  # blocked 200 s in the harness's own kill: more than the 80 s left of the window
            driver.record_slow({"command": "kill", "seconds": 200.0, "attempts": 2, "io_full": 74.0})
            driver.witnesses()
            self.assertEqual(driver.pending, [fault], "still waiting: the blocked time doesn't count")
            advance(79)
            driver.witnesses()
            self.assertEqual(driver.pending, [fault], "the rest of its own window")
            advance(2)
            with self.assertRaisesRegex(AuditError, "no reconnect/replay witness"):
                driver.witnesses()

    def test_every_pending_fault_gets_the_blocked_time_and_a_fault_with_none_pending_nothing(self):
        driver = soak.Soak.__new__(soak.Soak)
        driver.logged = []
        driver.log = lambda kind, **fields: driver.logged.append((kind, fields))
        first = {"kind": "client_sse", "target": 0, "deadline": 1060.0}
        second = {"kind": "runtime_link", "target": 1, "deadline": 1200.0, "resume_at": 1140.0}
        driver.pending = [first, second]
        driver.record_slow({"command": "stop", "seconds": 83.4, "attempts": 1, "io_full": 74.0})
        self.assertEqual((first["deadline"], second["deadline"]), (1060.0 + 83.4, 1200.0 + 83.4))
        self.assertEqual(second["resume_at"], 1140.0, "only the witness deadline moves, never the fault's own window")
        driver.pending = []
        driver.record_slow({"command": "kill", "seconds": 5.0, "attempts": 1, "io_full": 10.0})
        self.assertEqual([kind for kind, _ in driver.logged], ["engine_slow", "engine_slow"])

    def test_frames_beyond_the_cursor_count_only_when_stamped_after_the_restore(self):
        from soak import progressed
        receipts = [types.SimpleNamespace(last_seq=10, last_ts=99.0)]
        self.assertFalse(progressed(receipts, cursor=5, restored_at=100.0), "replayed from before the restore")
        self.assertFalse(progressed([types.SimpleNamespace(last_seq=5, last_ts=120.0)], cursor=5, restored_at=100.0))
        self.assertTrue(progressed([types.SimpleNamespace(last_seq=6, last_ts=100.5)], cursor=5, restored_at=100.0))

    def stall_driver(self, affected_after, replaced=True):
        """A driver injecting an engine stall; conversation 1 is idle, conversation 0 is busy."""
        clock = [100.0]
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations = 2, ["busy", "idle"]
        driver.audits = [types.SimpleNamespace(active="r-1", started=3, kill_windows=[], fault=lambda *a: None),
                         types.SimpleNamespace(active=None, started=2, kill_windows=[], fault=lambda *a: None)]
        driver.sent = [3, 2]
        driver.clients = [types.SimpleNamespace(receipts=types.SimpleNamespace(last_seq=1)) for _ in range(4)]
        proxy = types.SimpleNamespace(affected=5, stall=lambda action, hold, seconds: None)
        killed = []
        driver.stack = types.SimpleNamespace(proxy=proxy, paused={}, kill_sandbox=lambda c: killed.append(c) or "old-id",
                                             replacement=lambda c, old: ("new-id" if replaced else None))
        driver.started, driver.pending, driver.faults, driver.logged = 0.0, [], [], []
        driver.log = lambda kind, **fields: driver.logged.append((kind, fields))
        driver.poll = lambda: None
        driver.gateway_ready = lambda: True
        monotonic = mock.patch.object(soak.time, "monotonic", lambda: clock[0])

        def after_window():
            clock[0] += 121
            proxy.affected = 5 + affected_after
        return driver, killed, monotonic, after_window, clock

    def test_an_engine_stall_retires_an_idle_conversations_sandbox_so_a_replacement_meets_it(self):
        driver, killed, monotonic, after_window, _ = self.stall_driver(affected_after=2)
        with monotonic:
            driver.inject({"kind": "engine_stall", "target": None, "action": "error", "hold": 50, "seconds": 120})
            self.assertEqual(killed, ["idle"], "the idle conversation, never the busy one")
            self.assertEqual(driver.pending[0]["retired"], {"slot": 1, "killed": "old-id"})
            self.assertEqual(len(driver.audits[1].kill_windows), 1, "a resumed run there is excused as after a kill")
            after_window()
            driver.witnesses()
        self.assertEqual(driver.pending, [])
        self.assertEqual((driver.faults[0]["affected_calls"], driver.faults[0]["replacement"]), (2, "new-id"))

    def test_an_engine_stall_that_met_no_call_fails_at_its_deadline(self):
        driver, _, monotonic, after_window, clock = self.stall_driver(affected_after=0)
        with monotonic:
            driver.inject({"kind": "engine_stall", "target": None, "action": "hold", "hold": 15, "seconds": 120})
            after_window()
            driver.witnesses()
            self.assertEqual(len(driver.pending), 1, "not witnessed: nothing went through the stalled engine")
            clock[0] += soak.WITNESS_SECONDS
            with self.assertRaisesRegex(AuditError, "no reconnect/replay witness"):
                driver.witnesses()

    def test_the_journal_cap_grows_with_the_run(self):
        self.assertEqual(journal_cap(2), 16 * 1024 * 1024)
        self.assertEqual(journal_cap(24), 192 * 1024 * 1024)

    def test_a_conversation_is_replaced_before_its_script_or_message_cap_runs_out(self):
        self.assertFalse(needs_generation(0, 0))
        self.assertFalse(needs_generation(RESPONSES - 7, 44))
        self.assertTrue(needs_generation(RESPONSES - 6, 0))
        self.assertTrue(needs_generation(0, 45))

    def test_a_rehearsal_can_rotate_early_to_prove_the_generation_path(self):
        self.assertFalse(needs_generation(5, 2, rotate_after=6))
        self.assertTrue(needs_generation(6, 2, rotate_after=6))
        self.assertTrue(needs_generation(0, 45, rotate_after=6))

    def test_early_rotation_is_refused_outside_a_rehearsal(self):
        with self.assertRaisesRegex(AuditError, "rehearsal"):
            check_rotate_after(6, rehearsal=False)
        with self.assertRaises(AuditError):
            check_rotate_after(0, rehearsal=True)
        check_rotate_after(None, rehearsal=False)
        check_rotate_after(6, rehearsal=True)

    def test_a_slot_that_owes_a_completion_after_a_fault_gets_one_more_message_before_it_rotates(self):
        self.assertEqual(slot_action(rotate=True, owes_completion=True, pending_fault=False), "submit")
        self.assertEqual(slot_action(rotate=True, owes_completion=False, pending_fault=False), "rotate")
        self.assertEqual(slot_action(rotate=True, owes_completion=False, pending_fault=True), "wait")
        self.assertEqual(slot_action(rotate=True, owes_completion=True, pending_fault=True), "submit")
        self.assertEqual(slot_action(rotate=False, owes_completion=False, pending_fault=False), "submit")

    def test_no_slot_rotates_while_a_global_fault_awaits_its_witness(self):
        # The 16y soak (21 Oct 2026, ~/.cache/r2-soak-24h-20261021-0006): slot 6 rotated 19 s into a 96 s engine
        # stall (action drop, +79193); its new conversation's first create met the stalled engine and failed.
        driver = soak.Soak.__new__(soak.Soak)
        driver.audits = [types.SimpleNamespace(started=40, active=None, needs_completion=lambda: False,
                                               disk_resumes=[]) for _ in range(2)]
        driver.sent, driver.conversations = [40, 40], ["c-0", "c-1"]
        driver.clients = [types.SimpleNamespace(io_times=[]) for _ in range(4)]
        driver.stack, driver.recovered = types.SimpleNamespace(paused=set()), set()
        driver.progress = [types.SimpleNamespace(completed=RESPONSES) for _ in range(2)]
        driver.options = types.SimpleNamespace(rotate_after=None)
        driver.sending_paused = lambda: False
        rotated = []
        driver.rotate, driver.submit = rotated.append, lambda index: self.fail("nothing owes a message")
        for kind in ("engine_stall", "db_outage", "disk_full"):  # the global faults that can be pending
            driver.pending = [{"kind": kind, "target": None}]
            driver.replenish(draining=False)
            self.assertEqual(rotated, [], "a {} awaiting its witness holds every slot".format(kind))
        driver.pending = [{"kind": "sandbox_kill", "target": 1}]
        driver.replenish(draining=False)
        self.assertEqual(rotated, [0], "a slot's own fault holds only that slot")
        driver.pending = []
        driver.replenish(draining=False)
        self.assertEqual(rotated, [0, 0, 1])

    def test_a_full_disk_recovers_with_a_step_within_the_margin_and_a_run_before_the_soak_ends(self):
        # The c14b soak's slot 5 (5 Oct 2026, ~/.cache/r2-soak-24h-20261005-0932): the disk freed at 20:41:08; run
        # r-223 had completed at 20:40:39, before the free; r-483 started at the free, its steps completing at 20:42:37
        # and 20:43:46 at the fake model's 69 s pace, so no whole run fits in 180 s.
        at = soak_audit.timestamp_seconds
        window = [at("2026-10-05T12:39:28.414Z"), at("2026-10-05T12:41:08.414Z")]
        steps = [at("2026-10-05T12:40:39.728Z"), at("2026-10-05T12:42:37.847Z"), at("2026-10-05T12:43:46.931Z")]
        completions = [at("2026-10-05T12:40:39.729Z")]
        margin_passed = window[1] + 181
        self.assertEqual(disk_recovery(window, steps, completions, margin_passed), "pending",
                         "r-483's timeline: a step 89 s after the free, its run still running")
        self.assertEqual(disk_recovery(window, steps, completions + [at("2026-10-05T12:44:56.000Z")], margin_passed),
                         "recovered")
        # Under the old rule the same timeline failed at the margin.
        self.assertEqual(io_recovery(window, completions, margin_passed), "failed")

    def test_a_full_disk_without_a_step_within_the_margin_fails(self):
        window = [100.0, 150.0]
        self.assertEqual(disk_recovery(window, [], [], now=150.0 + 180), "pending")
        self.assertEqual(disk_recovery(window, [], [], now=150.0 + 181), "failed_step")
        self.assertEqual(disk_recovery(window, [150.0 + 181], [150.0 + 400], now=150.0 + 401), "failed_step",
                         "a step after the margin is too late, whatever follows")
        self.assertEqual(disk_recovery([100.0, float("inf")], [], [], now=9999.0), "pending", "the disk is still full")

    def test_a_full_disk_with_a_step_but_no_run_completing_before_the_end_fails(self):
        window = [100.0, 150.0]
        self.assertEqual(disk_recovery(window, [200.0], [], now=9999.0), "pending")
        self.assertEqual(disk_recovery(window, [200.0], [], now=9999.0, final=True), "failed_run")
        self.assertEqual(disk_recovery(window, [200.0], [5000.0], now=9999.0, final=True), "recovered",
                         "the run may complete long after the margin, before the end")

    def test_a_step_or_run_completed_before_the_free_proves_nothing(self):
        window = [100.0, 150.0]
        self.assertEqual(disk_recovery(window, [140.0], [140.0], now=150.0 + 181), "failed_step")
        self.assertEqual(disk_recovery(window, [140.0, 160.0], [140.0], now=150.0 + 181, final=True), "failed_run")

    def test_the_audit_records_when_each_step_completed(self):
        audit = soak_audit.RunAudit()
        audit.record({"type": "event", "seq": 1, "ts": "2026-10-05T12:00:00Z",
                      "event": {"type": "run.started", "run_id": "r-1"}, "run_id": "r-1"})
        audit.record({"type": "event", "seq": 2, "ts": "2026-10-05T12:01:09Z",
                      "event": {"type": "step.completed", "step": 1, "duration_ms": 69000}, "run_id": "r-1"})
        self.assertEqual(audit.step_times, [soak_audit.timestamp_seconds("2026-10-05T12:01:09Z")])

    def test_a_store_unavailable_must_complete_a_run_within_the_margin(self):
        window = [100.0, 150.0]
        self.assertEqual(io_recovery(window, completions=[], now=120.0), "pending", "the storage is still full")
        self.assertEqual(io_recovery([100.0, float("inf")], completions=[], now=9999.0), "pending")
        self.assertEqual(io_recovery(window, completions=[140.0], now=200.0), "pending",
                         "a completion before the free does not prove recovery")
        self.assertEqual(io_recovery(window, completions=[140.0, 200.0], now=210.0), "recovered")
        self.assertEqual(io_recovery(window, completions=[], now=150.0 + 180), "pending")
        self.assertEqual(io_recovery(window, completions=[], now=150.0 + 181), "failed")
        self.assertEqual(io_recovery(window, completions=[150.0 + 181], now=150.0 + 182), "failed")

    def test_the_full_disk_probe_passes_on_a_typed_refusal_or_an_accepted_message(self):
        # Accepted means the message was taken on a full disk; its run completing within the margin after the free is
        # then the recovery condition, checked like any other conversation's.
        self.assertEqual(probe_verdict(refusal={"status": 507, "code": "insufficient_storage"}),
                         "refused: 507 insufficient_storage")
        self.assertEqual(probe_verdict(refusal=None), "accepted")
        self.assertEqual(probe_verdict(refusal={"status": 500, "code": None}), "failed", "an untyped refusal")

    def test_the_probe_picks_a_settled_conversation_and_never_a_busy_one(self):
        # A message into a conversation with a run in flight is answered `busy`, which the full-disk rule doesn't
        # excuse (the candidate 6 rehearsal): with none idle there is no slot, not a fallback.
        self.assertEqual(probe_slot([False, True, True]), 1)
        self.assertIsNone(probe_slot([False, False]))

    def test_the_probe_waits_a_bounded_time_for_an_idle_conversation(self):
        clock = [0.0]
        states = iter([[False, False], [False, False], [False, True]])
        polled = []
        index = wait_for_idle(lambda: next(states), poll=lambda: polled.append(clock[0]), seconds=10,
                              clock=lambda: clock[0], sleep=lambda s: clock.__setitem__(0, clock[0] + s))
        self.assertEqual(index, 1)
        self.assertEqual(len(polled), 2, "events are drained between checks, so a run's end is seen")

    def test_with_none_idle_in_time_the_probe_is_skipped_not_sent(self):
        clock = [0.0]
        index = wait_for_idle(lambda: [False] * 8, poll=lambda: None, seconds=10, clock=lambda: clock[0],
                              sleep=lambda s: clock.__setitem__(0, clock[0] + s))
        self.assertIsNone(index)
        self.assertLessEqual(clock[0], 10.5)
        self.assertEqual(probe_outcome({"skipped": "no conversation was idle", "refusal": None}), "skipped")
        self.assertEqual(probe_outcome({"refusal": None}), "accepted")
        self.assertEqual(probe_outcome({"refusal": {"status": 500, "code": None}}), "failed")

    def test_only_replacements_inside_a_full_disk_window_and_its_margin_count(self):
        windows = [[100.0, 115.0], [900.0, float("inf")]]
        times = [99.0, 103.0, 115.0 + 180, 115.0 + 181, 950.0]
        self.assertEqual(replacements_on_full_disk(times, windows, margin=180), [103.0, 295.0, 950.0])
        self.assertEqual(replacements_on_full_disk([50.0], [], margin=180), [])

    def test_a_runtime_replaced_on_the_full_disk_is_forgotten_but_not_one_replaced_outside_it(self):
        # The candidate 6 rehearsal: three runtimes replaced during the full disk were still remembered, so the next
        # gateway restart's check called their replacements unexpected.
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations = 2, ["c0", "c1"]
        driver.disk_windows = [[100.0, 115.0]]
        driver.audits = [types.SimpleNamespace(disk_resumes=[104.0]), types.SimpleNamespace(disk_resumes=[])]
        idle = types.SimpleNamespace(recovery=types.SimpleNamespace(replaced_at=[]))
        outside = types.SimpleNamespace(recovery=types.SimpleNamespace(replaced_at=[500.0]))
        driver.clients = [idle, idle, outside, idle]
        driver.stack = types.SimpleNamespace(runtimes={"c0": "old-0", "c1": "old-1"})
        driver.forgotten = [0, 0]
        driver.logged = []
        driver.log = lambda kind, **fields: driver.logged.append((kind, fields))
        driver.forget_replaced_runtimes()
        self.assertEqual(driver.stack.runtimes, {"c1": "old-1"})
        driver.stack.runtimes["c0"] = "new-0"
        driver.forget_replaced_runtimes()
        self.assertEqual(driver.stack.runtimes, {"c0": "new-0", "c1": "old-1"}, "each replacement is forgotten once")
        self.assertEqual(driver.logged[0][0], "runtime_replaced_on_full_disk")

    def test_a_new_conversation_s_full_disk_replacement_is_forgotten_after_the_slot_rotates(self):
        # The c14b-rc soak rerun: slot 6 was replaced on one full disk, rotated, and replaced on the next; the count
        # kept from the old conversation hid the new replacement, so the next check called it unexpected.
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations, driver.generation = 1, ["c0-g0"], [0]
        driver.disk_windows, driver.windows, driver.database_windows = [[100.0, 115.0]], [], []
        driver.audits, driver.progress, driver.sent = [types.SimpleNamespace(disk_resumes=[104.0])], [None], [0]
        idle = types.SimpleNamespace(recovery=types.SimpleNamespace(replaced_at=[]))
        driver.clients = [idle, idle]
        driver.forgotten = [0]
        with tempfile.TemporaryDirectory() as output:
            driver.stack = types.SimpleNamespace(runtimes={"c0-g0": "old-0"}, request=lambda path, body: {"id": "c0-g1"},
                                                 output=Path(output), origin="http://gateway", cookie="")
            driver.options = types.SimpleNamespace(model="scripted")
            driver.submit, driver.events = (lambda index: None), queue.Queue()
            driver.logged = []
            driver.log = lambda kind, **fields: driver.logged.append(kind)
            driver.forget_replaced_runtimes()
            self.assertEqual(driver.stack.runtimes, {})
            with mock.patch.object(soak, "SseClient") as client:
                client.return_value.recovery.replaced_at = []
                driver.open_conversation(0)
        self.assertEqual(driver.conversations, ["c0-g1"])
        driver.stack.runtimes["c0-g1"] = "first-1"
        driver.disk_windows.append([900.0, 915.0])
        driver.audits[0].disk_resumes.append(904.0)
        driver.forget_replaced_runtimes()
        self.assertEqual(driver.stack.runtimes, {}, "the new conversation's replacement is forgotten")
        self.assertEqual(driver.logged.count("runtime_replaced_on_full_disk"), 2)

    def measuring_driver(self, outcomes, clock):
        import subprocess
        driver = soak.Soak.__new__(soak.Soak)
        driver.samples, driver.missed_samples, driver.logged = [], 0, []
        driver.clients, driver.audits = [], []
        driver.clock = lambda: clock[0]
        driver.last_sample_ok = clock[0]
        driver.log = lambda kind, **fields: driver.logged.append(kind)
        results = iter(outcomes)

        def resources():
            outcome = next(results)
            if outcome is None:
                raise subprocess.TimeoutExpired(["docker", "run"], 90)
            return outcome
        driver.stack = types.SimpleNamespace(resources=resources)
        driver.sampler, driver.pending_sample = Immediate(), None
        return driver

    def test_a_slow_sample_never_blocks_the_loop_that_drains_client_events(self):
        # Clients fill a 512-event queue at about 13 events a second: a sample that held the main loop for 90 s would
        # overflow it. The sample runs on its own worker; measure() only collects and starts one.
        import concurrent.futures
        release = __import__("threading").Event()
        driver = soak.Soak.__new__(soak.Soak)
        driver.samples, driver.missed_samples, driver.logged = [], 0, []
        driver.clients, driver.audits = [], []
        driver.clock, driver.last_sample_ok = (lambda: 0.0), 0.0
        driver.log = lambda kind, **fields: driver.logged.append(kind)
        # The sample is held 30 s: a measure() that waited for it would take that long, so 5 s tells the two apart.
        driver.stack = types.SimpleNamespace(resources=lambda: (release.wait(30), {"at": 1})[1])
        driver.sampler, driver.pending_sample = concurrent.futures.ThreadPoolExecutor(max_workers=1), None
        self.addCleanup(driver.sampler.shutdown, wait=False)
        started = time.monotonic()
        driver.measure()
        driver.measure()
        self.assertLess(time.monotonic() - started, 5.0, "measure() returned while the sample still ran")
        self.assertEqual(driver.samples, [])
        release.set()
        driver.pending_sample.result(timeout=5)
        driver.measure()
        self.assertEqual(driver.samples[0], {"at": 1}, "collected on the next measure()")

    def test_the_drivers_own_threads_are_bounded_and_a_leak_stops_the_run_in_plain_words(self):
        # R2 leaked a thread per engine connection until the user's process limit was near and other builds failed.
        check_own_threads(MAX_DRIVER_THREADS)
        with self.assertRaisesRegex(AuditError, "the soak driver has 1001 threads; its bound is 1000: a leak in the "
                                                "harness, stopped before it exhausts this user's process limit"):
            check_own_threads(MAX_DRIVER_THREADS + 1)

    def test_every_poll_checks_the_drivers_own_threads(self):
        driver = soak.Soak.__new__(soak.Soak)
        driver.clients, driver.events = [], queue.Queue()
        with mock.patch.object(soak.threading, "active_count", return_value=MAX_DRIVER_THREADS + 1):
            with self.assertRaisesRegex(AuditError, "a leak in the harness"):
                driver.poll()

    def test_each_resource_sample_records_the_drivers_threads(self):
        clock = [0.0]
        driver = self.measuring_driver([{"at": 1}], clock)
        fields = []
        driver.log = lambda kind, **logged: fields.append((kind, logged))
        with mock.patch.object(soak.threading, "active_count", return_value=42):
            driver.measure()
        self.assertEqual([logged["driver_threads"] for kind, logged in fields if kind == "resources"], [42])

    def test_a_slow_harness_engine_command_is_logged_as_a_measurement(self):
        # R2's second run: a harness `docker kill` on a saturated engine is measured, never the soak's failure.
        driver = soak.Soak.__new__(soak.Soak)
        driver.pending = []
        driver.logged = []
        driver.log = lambda kind, **fields: driver.logged.append((kind, fields))
        driver.record_slow({"command": "kill", "seconds": 212.4, "attempts": 2, "io_full": 71.5})
        self.assertEqual(driver.logged, [("engine_slow", {"command": "kill", "seconds": 212.4, "attempts": 2,
                                                          "io_full": 71.5})])

    def test_a_missed_resource_sample_is_recorded_and_the_run_goes_on(self):
        # R1 on candidate 6: helper containers were slow to start on a busy host; a measurement must not
        # fail the soak, only a sampler that has stopped working.
        clock = [0.0]
        driver = self.measuring_driver([{"at": 1}, None, None, None, None, {"at": 6}], clock)
        for _ in range(6):
            clock[0] += 60
            driver.measure()
        self.assertEqual([sample["at"] for sample in driver.samples], [1, 6])
        self.assertEqual(driver.missed_samples, 4)
        self.assertEqual(driver.logged.count("resources_missed"), 4)

    def test_no_successful_sample_for_fifteen_minutes_fails_the_soak(self):
        clock = [0.0]
        driver = self.measuring_driver([{"at": 1}] + [None] * 40, clock)
        clock[0] += 30
        driver.measure()
        with self.assertRaisesRegex(AuditError, "no resource sample succeeded for 15 minutes"):
            for _ in range(40):
                clock[0] += 30
                driver.measure()
        self.assertGreaterEqual(clock[0] - 30, soak.SAMPLE_STALL_SECONDS)

    def test_more_than_a_tenth_missed_cannot_judge_growth(self):
        self.assertIsNone(soak.missed_sample_problem(missed=24, attempts=240))
        self.assertIn("25 of 240", soak.missed_sample_problem(missed=25, attempts=240))
        self.assertIsNone(soak.missed_sample_problem(missed=2, attempts=5), "a short run may miss a couple")

    def test_the_resource_helpers_wait_long_enough_for_a_loaded_engine(self):
        self.assertGreaterEqual(soak_stack.RESOURCE_COMMAND_SECONDS, 90)

    def test_a_conversations_control_frames_are_summed_over_its_clients(self):
        clients = [types.SimpleNamespace(control_frames={"message.queue": 2}),
                   types.SimpleNamespace(control_frames={"message.queue": 1, "future.notice": 1})]
        self.assertEqual(soak.control_frames(clients), {"message.queue": 3, "future.notice": 1})
        self.assertEqual(soak.control_frames([]), {})

    def test_one_stall_that_reaches_several_clients_is_one_event(self):
        self.assertEqual(store_events([100.0, 102.5, 103.0, 500.0, 509.0, 800.0]), [100.0, 500.0, 800.0])
        self.assertEqual(store_events([]), [])

    def test_more_than_five_stall_events_in_an_hour_fail(self):
        self.assertFalse(too_many_store_events([0.0, 600.0, 1200.0, 1800.0, 2400.0]))
        self.assertTrue(too_many_store_events([0.0, 600.0, 1200.0, 1800.0, 2400.0, 3000.0]))
        self.assertFalse(too_many_store_events([0.0, 700.0, 1400.0, 2100.0, 2800.0, 3700.0]), "a rolling hour")

    def store_driver(self, store_times, completed):
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations = 1, ["c0"]
        driver.clients = [types.SimpleNamespace(store_times=store_times),
                          types.SimpleNamespace(store_times=list(store_times))]
        driver.audits = [types.SimpleNamespace(completed_times=completed)]
        driver.store_recovered, driver.logged = set(), []
        driver.log = lambda kind, **fields: driver.logged.append(kind)
        return driver

    def test_a_conversation_that_got_the_notice_must_complete_a_run_within_180_s(self):
        driver = self.store_driver([1000.0], [1100.0])
        with mock.patch.object(soak.time, "time", return_value=1150.0):
            driver.store_recoveries()
        self.assertIn("store_recovered", driver.logged)
        driver = self.store_driver([1000.0], [])
        with mock.patch.object(soak.time, "time", return_value=1100.0):
            driver.store_recoveries()
        with mock.patch.object(soak.time, "time", return_value=1181.0), \
                self.assertRaisesRegex(AuditError, "store_unavailable.*did not complete a run within 180 s"):
            driver.store_recoveries()

    def client_drop_driver(self, database_windows):
        driver = soak.Soak.__new__(soak.Soak)
        driver.clients = [types.SimpleNamespace(connections=1, snapshots=1)]
        driver.pending = [{"kind": "client_sse", "target": 0, "connections": 1, "snapshots": 1,
                           "deadline": time.monotonic() - 1}]
        driver.database_windows, driver.logged = database_windows, []
        driver.log = lambda kind, **fields: driver.logged.append(kind)
        return driver

    def test_a_client_dropped_while_the_database_is_down_is_witnessed_after_it_is_back(self):
        # R1 on candidate 6: a client drop 20 s into a 173 s database outage; its replay needs the database, so its
        # 60 s witness deadline ran out while the database was still down.
        now = time.time()
        driver = self.client_drop_driver([[now - 20, float("inf")]])
        driver.witnesses()
        self.assertEqual(len(driver.pending), 1, "still waiting, not failed")
        self.assertGreater(driver.pending[0]["deadline"], time.monotonic() + 55, "a full witness time after the return")
        driver.clients[0].connections, driver.clients[0].snapshots = 2, 2
        driver.witnesses()
        self.assertEqual((driver.pending, driver.logged), ([], ["fault_witness"]))

    def stack_wide_driver(self, events, fail=None):
        """A driver whose stack records the order of its calls; `fail` names a stack call that raises."""
        def call(name):
            def run(*_):
                events.append(name)
                if name == fail:
                    raise AuditError(name + " failed")
                return {}
            return run
        driver = soak.Soak.__new__(soak.Soak)
        driver.stack = types.SimpleNamespace(**{name: call(name) for name in
                                                ("database_fault", "restart", "kill_gateway", "sandbox")})
        driver.started, driver.slots, driver.conversations = time.monotonic(), 2, ["c0", "c1"]
        driver.windows, driver.database_windows, driver.faults, driver.pending = [], [], [], []
        driver.mark_fault = lambda kind, indices: None
        driver.log = lambda kind, **fields: events.append((kind, fields.get("fault_kind")))
        return driver

    def test_a_stack_wide_fault_logs_its_recovery_once_its_inline_check_passed(self):
        # The four kinds with no target are never pending, so no fault_witness line: their recovery is checked where
        # they are injected, and fault_recovered records it, so every fault in the journal has its outcome.
        for kind, check in (("db_restart", ["database_fault"]), ("db_crash", ["database_fault"]),
                            ("gateway_restart", ["restart", "sandbox", "sandbox"]),
                            ("gateway_kill", ["kill_gateway", "sandbox", "sandbox"])):
            with self.subTest(kind=kind):
                events = []
                self.stack_wide_driver(events).inject({"kind": kind, "target": None, "at": 1})
                calls = [event for event in events if isinstance(event, str)]
                self.assertEqual(calls, check)
                self.assertEqual(events[-1], ("fault_recovered", kind), "the last line, after every check")
                self.assertEqual(events.count(("fault_recovered", kind)), 1)

    def test_a_stack_wide_fault_whose_check_fails_logs_no_recovery(self):
        for kind, failing in (("db_crash", "database_fault"), ("gateway_kill", "sandbox")):
            with self.subTest(kind=kind):
                events = []
                with self.assertRaises(AuditError):
                    self.stack_wide_driver(events, fail=failing).inject({"kind": kind, "target": None, "at": 1})
                self.assertNotIn(("fault_recovered", kind), events)

    def test_a_client_drop_with_the_database_up_keeps_its_deadline(self):
        now = time.time()
        driver = self.client_drop_driver([[now - 200, now - 100]])
        with self.assertRaisesRegex(AuditError, "no reconnect/replay witness"):
            driver.witnesses()

    def test_a_replacement_on_a_full_disk_is_timed_from_the_gateway_log(self):
        conversation = "01a0e5a7-8ea6-76ed-be4d-301ea9b5b601"
        log = "\n".join(json.dumps(record) for record in (
            {"timestamp": "2026-09-28T01:36:11.500Z", "fields": {"message": "started sandbox"},
             "span": {"conversation_id": conversation}},
            {"timestamp": "2026-09-28T01:36:11.900Z", "fields": {"message": "destroyed sandbox"},
             "span": {"conversation_id": "someone-else"}},
            {"timestamp": "2026-09-28T01:36:11.960Z", "fields": {"message": "destroyed sandbox"},
             "span": {"conversation_id": conversation}},
            {"timestamp": "2026-09-28T01:36:12.470Z", "fields": {"message": "started sandbox"},
             "span": {"conversation_id": conversation}})) + "\nnot json\n"
        resumed = 1790559372.7558
        times = replacement_times(log, conversation, resumed)
        self.assertAlmostEqual(times["destroyed"], 1790559371.96, places=2)
        self.assertAlmostEqual(times["started"], 1790559372.47, places=2)
        self.assertEqual(times["resumed"], resumed)
        self.assertAlmostEqual(times["destroy_to_resume_seconds"], 0.80, places=1)
        self.assertEqual(replacement_times("", conversation, resumed)["destroyed"], None)

    def test_a_disk_resume_alone_also_owes_the_recovery(self):
        driver = soak.Soak.__new__(soak.Soak)
        driver.conversations, driver.recovered, driver.probed = ["c0"], set(), {}
        driver.clients = [types.SimpleNamespace(io_times=[]), types.SimpleNamespace(io_times=[])]
        driver.audits = [types.SimpleNamespace(disk_resumes=[5.0])]
        self.assertTrue(driver.owes_io_recovery(0))
        driver.audits = [types.SimpleNamespace(disk_resumes=[])]
        self.assertFalse(driver.owes_io_recovery(0))

    def test_a_probed_conversation_with_a_step_but_no_run_fails_when_it_closes(self):
        # A conversation considered only through the probe (or a disk resume) has no io notice,
        # so judging its close as final only on io notices let "a step, no run" pass silently.
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations, driver.recovered = 2, ["c0", "c1"], set()
        driver.probed, driver.disk_windows = {"c0": [100.0, 150.0]}, [[100.0, 150.0]]
        driver.clients = [types.SimpleNamespace(io_times=[]) for _ in range(4)]
        step_no_run = types.SimpleNamespace(disk_resumes=[], step_times=[200.0], completed_times=[])
        driver.audits = [step_no_run, types.SimpleNamespace(disk_resumes=[], step_times=[], completed_times=[])]
        driver.log = mock.Mock()
        with mock.patch.object(soak.time, "time", return_value=400.0):
            driver.io_recoveries()  # mid-soak: still owed, not yet a failure
            with self.assertRaisesRegex(AuditError, "completed no run after the free before the soak ended"):
                driver.io_recoveries(closing=0)
            # Another slot closing judges only itself: c0 stays pending.
            driver.io_recoveries(closing=1)
            step_no_run.completed_times = [380.0]
            driver.io_recoveries(closing=0)
        self.assertEqual(driver.recovered, {"c0"})

    def test_closing_a_probed_conversation_that_owes_its_run_fails(self):
        # The call site itself: close_conversation judges its own slot final, with no io notice.
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations, driver.recovered = 1, ["c0"], set()
        driver.probed, driver.disk_windows = {"c0": [100.0, 150.0]}, [[100.0, 150.0]]
        driver.clients = [types.SimpleNamespace(io_times=[], store_times=[]) for _ in range(2)]
        driver.audits = [types.SimpleNamespace(disk_resumes=[], step_times=[200.0], completed_times=[])]
        driver.log, driver.store_recoveries = mock.Mock(), mock.Mock()
        with mock.patch.object(soak.time, "time", return_value=400.0), \
                self.assertRaisesRegex(AuditError, "completed no run after the free before the soak ended"):
            driver.close_conversation(0)

    def test_a_disruption_kind_can_be_left_out_on_purpose_and_only_those(self):
        self.assertEqual(planned_kinds("docker", ["disk_full"]),
                         tuple(kind for kind in backend_kinds("docker") if kind != "disk_full"))
        self.assertEqual(planned_kinds("docker", []), backend_kinds("docker"))
        for bad in (["client_sse"], ["gateway_restart"], ["meteor"]):
            with self.subTest(bad), self.assertRaises(AuditError):
                planned_kinds("docker", bad)

    def test_container_logs_are_collected_in_whole_hours_and_once_more_at_the_end(self):
        self.assertEqual(log_windows(1000.0, 1000.0 + 3599, 1000.0), [])
        self.assertEqual(log_windows(1000.0, 1000.0 + 7300, 1000.0), [(1000.0, 4600.0), (4600.0, 8200.0)])
        self.assertEqual(log_windows(1000.0, 1000.0 + 7300, 4600.0), [(4600.0, 8200.0)])
        self.assertEqual(log_windows(1000.0, 9000.0, 8200.0, final=True), [(8200.0, 9000.0)])
        self.assertEqual(log_windows(1000.0, 8200.0, 8200.0, final=True), [])

    def test_every_fault_kind_must_reach_some_conversation(self):
        kinds = {"client_sse", "runtime_link", "gateway_restart"}
        self.assertTrue(covered([{"client_sse", "gateway_restart"}, {"runtime_link"}], kinds))
        # A generation opened in the last minutes may see no fault; the kinds as a whole must still be covered.
        self.assertTrue(covered([kinds, set()], kinds))
        self.assertFalse(covered([{"client_sse", "gateway_restart"}, {"client_sse"}], kinds))

    def test_queue_overflow_fails_after_recording_the_whole_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                events = queue.Queue(maxsize=1)
                client = SseClient(("http://localhost", "/events", "", "0/0"), receipts, events, [])
                client.receive(1, {"type": "event", "seq": 1})
                with self.assertRaisesRegex(AuditError, "queue"):
                    client.receive(2, {"type": "event", "seq": 2})
                self.assertEqual(receipts.last_seq, 2)
                self.assertEqual(events.qsize(), 1)

    def test_secondary_clients_still_reject_duplicates_and_unexpected_errors(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                client = SseClient(("http://localhost", "/events", "", "0/1"), receipts, None, [])
                client.receive(1, {"type": "event", "seq": 1})
                with self.assertRaises(AuditError):
                    client.receive(1, {"type": "event", "seq": 1})
                for code in ("restarting", "seq_gap"):
                    with self.subTest(code=code), self.assertRaises(AuditError):
                        client.receive(None, {"type": "error", "seq": None, "code": code})
                # store_unavailable outside a database fault is load, recorded for the driver's recovery check.
                client.receive(None, {"type": "error", "seq": None, "code": "store_unavailable"})
                self.assertEqual(len(client.store_times), 1)


if __name__ == "__main__":
    unittest.main()


class ReadyWaitTests(unittest.TestCase):
    """SOAK-001: a new conversation is waited for as long as the gateway's own start bounds allow (soak 4, 7 Oct 2026,
    failed on a fixed 60 s that gave up before the gateway's own retry of a slow create)."""

    def test_the_wait_is_the_gateways_start_bounds_plus_the_margin(self):
        self.assertEqual(soak.ready_wait("[sandbox]\ncreate_budget_seconds=120\nready_seconds=30\n"), 180)
        self.assertEqual(soak.ready_wait("[sandbox]\nready_seconds=45\ncreate_budget_seconds=600\nmax_live=4\n"),
                         600 + 45 + soak.READY_MARGIN_SECONDS)
        # Never shorter than one container create plus its retry: the old 60 s was.
        self.assertGreater(soak.ready_wait("[sandbox]\ncreate_budget_seconds=120\nready_seconds=30\n"), 120)

    def test_an_unset_or_odd_bound_is_refused_rather_than_guessed(self):
        for text in ("[sandbox]\nready_seconds=30\n", "[sandbox]\ncreate_budget_seconds=120\n", "",
                     "[sandbox]\ncreate_budget_seconds='120'\nready_seconds=30\n",
                     "[sandbox]\ncreate_budget_seconds=true\nready_seconds=30\n",
                     "[sandbox]\ncreate_budget_seconds=0\nready_seconds=30\n"):
            with self.subTest(text=text), self.assertRaisesRegex(Exception, "must be set"):
                soak.ready_wait(text)

    def test_the_stacks_gateway_toml_carries_the_bounds_the_wait_reads(self):
        import compose
        with tempfile.TemporaryDirectory() as temporary:
            directory, env = Path(temporary), {"TQA_GATEWAY_IMAGE": "gw", "TQA_WEB_IMAGE": "web",
                                               "TQA_WORKSTATION_IMAGE": "sha256:" + "b" * 64}
            with mock.patch.object(compose, "command"), mock.patch.object(compose, "ownership"), \
                    mock.patch.object(compose, "engine_subnets", return_value=[]), \
                    mock.patch.object(compose, "ensure_image"), \
                    mock.patch.object(compose, "gateway_takes_pins", return_value=True), \
                    mock.patch.object(compose, "image_id", side_effect=lambda reference, _env: reference):
                compose.prepare(directory, env, runtime="sha256:" + "c" * 64, max_live=17)
            self.assertEqual(soak.ready_wait((directory / "gateway.toml").read_text()), 180)

    def test_prepare_reads_the_wait_before_the_folder_is_the_gateways(self):
        # Soak 5 (8 Oct 2026) failed at its first wait: gateway.toml was read after the folder went to 1000:1000.
        import compose
        import demo
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "installation"
            directory.mkdir()
            stack = soak_stack.Stack(directory, {"TQA_BOOTSTRAP_TOKEN": "t"}, Path(temporary))

            def prepared(folder, env, **_):
                (folder / "gateway.toml").write_text("[sandbox]\ncreate_budget_seconds=120\nready_seconds=30\n")
                (folder / "models.toml").write_text("")

            def ownership(folder, env, owner=None):
                if owner == "1000:1000":  # as on the host: this user can't read the folder's files any more
                    (folder / "gateway.toml").chmod(0)

            with mock.patch.object(compose, "preflight"), mock.patch.object(compose, "prepare", prepared), \
                    mock.patch.object(compose, "ownership", ownership), \
                    mock.patch.object(soak_stack, "image_references", return_value={"gateway": "g", "web": "w", "runtime": "r",
                                                                              "fake-model": "f"}), \
                    mock.patch.object(demo, "runtime_image", side_effect=lambda env, reference: reference), \
                    mock.patch.object(soak_stack.Stack, "command"), mock.patch.object(soak_stack.Stack, "service"), \
                    mock.patch.object(soak_stack.Stack, "wait_ready"), mock.patch.object(soak_stack.Stack, "request"):
                stack.prepare(rehearsal=False)
            self.assertEqual(stack.ready_wait_seconds, 180)

    def test_wait_active_gives_up_only_after_the_read_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "gateway.toml").write_text("[sandbox]\ncreate_budget_seconds=120\nready_seconds=30\n")
            driver = soak.Soak.__new__(soak.Soak)
            bound = soak.ready_wait((directory / "gateway.toml").read_text())
            driver.stack = types.SimpleNamespace(ready_wait_seconds=bound)
            driver.audits, driver.clients = [types.SimpleNamespace(started=0)], [None, None]
            clock, polls = [1000.0], []

            def poll():
                polls.append(clock[0])
                clock[0] += 10
            driver.poll = poll
            with mock.patch.object(soak.time, "monotonic", lambda: clock[0]):
                with self.assertRaisesRegex(Exception, "were not ready"):
                    driver.wait_active([0])
            # Polled through the whole bound: well past the old 60 s, and not past 180 s.
            self.assertGreaterEqual(polls[-1] - 1000.0, 170)
            self.assertLess(polls[-1] - 1000.0, 180)
