"""SOAK-002: ordered receipts and terminal accounting must reject false success."""
import copy
import datetime
import json
from pathlib import Path
import tempfile
import types
import unittest
from unittest import mock

import soak
from soak_audit import AuditError, ReceiptLog, RunAudit


def frame(seq, kind="text.delta", **fields):
    return {"type": "event", "seq": seq, "run_id": "run-example", "step": 1,
            "ts": "2000-01-01T00:00:10Z", "event": {"type": kind, **fields}}


class ReceiptTests(unittest.TestCase):
    def test_empty_receipts_cannot_pass_a_soak(self):
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                with self.assertRaisesRegex(AuditError, "empty"):
                    receipts.verify([])

    def test_gap_duplicate_reorder_and_id_mismatch_are_rejected_before_recording(self):
        for incoming, identifier in [(1, 1), (3, 3), (0, 0), (2, 3), (True, True)]:
            with self.subTest(incoming=incoming, identifier=identifier):
                with tempfile.TemporaryDirectory() as temporary:
                    with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                        receipts.record(frame(1), 1)
                        with self.assertRaises(AuditError):
                            receipts.record(frame(incoming), identifier)
                        self.assertEqual(receipts.last_seq, 1)

    def test_pagination_preserves_every_full_payload_without_union_or_deduplication(self):
        values = [frame(1, text="first"), frame(2, text="second"), frame(3, text="third")]
        corruptions = [values[:2], values + [frame(4)], [values[0], values[0], values[2]],
                       [values[0], values[2], values[1]], copy.deepcopy(values)]
        corruptions[-1][1]["event"]["text"] = "changed"
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                for value in values:
                    receipts.record(value, value["seq"])
                self.assertEqual(receipts.verify([[values[0]], values[1:]]), 3)
                for stored in corruptions:
                    with self.subTest(stored=stored), self.assertRaises(AuditError):
                        receipts.verify([stored])

    def test_any_sequenced_frame_is_part_of_the_same_receipt_log(self):
        error = {"type": "error", "seq": 2, "code": "io", "message": "example", "retryable": False}
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                receipts.record(frame(1), 1)
                receipts.record(error, 2)
                self.assertEqual(receipts.verify([[frame(1), error]]), 2)

    def test_full_payload_equality_distinguishes_boolean_integer_and_float_values(self):
        original = frame(1, value=1)
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl") as receipts:
                receipts.record(original, 1)
                for changed in (True, 1.0):
                    with self.subTest(changed=changed), self.assertRaises(AuditError):
                        receipts.verify([[frame(1, value=changed)]])

    def test_receipt_cap_is_inclusive_and_does_not_advance_after_overflow(self):
        encoded = json.dumps(frame(1), sort_keys=True, separators=(",", ":")).encode() + b"\n"
        with tempfile.TemporaryDirectory() as temporary:
            with ReceiptLog(Path(temporary) / "client.jsonl", max_bytes=len(encoded)) as receipts:
                receipts.record(frame(1), 1)
                with self.assertRaisesRegex(AuditError, "cap"):
                    receipts.record(frame(2), 2)
                self.assertEqual(receipts.last_seq, 1)


class RunTests(unittest.TestCase):
    def started(self):
        audit = RunAudit()
        audit.record(frame(1, "message.user", text="work"))
        audit.record(frame(2, "run.started", run_id="run-example"))
        return audit

    def retry(self, audit, count):
        for attempt in range(1, count + 1):
            audit.record(frame(2 * attempt + 1, "step.started", step=1, attempt=attempt))
            if attempt < count:
                audit.record(frame(2 * attempt + 2, "provider.retry", attempt=attempt,
                                   delay_ms=10, reason="transport"))

    def test_link_cut_failure_is_never_accepted(self):
        audit = self.started()
        audit.fault("runtime_link", 2)
        self.retry(audit, 3)
        with self.assertRaisesRegex(AuditError, "run.failed"):
            audit.record(frame(9, "run.failed", run_id="run-example", code="provider_error"))

    def test_planned_restart_requires_exhausted_retries_and_a_matching_timestamp(self):
        for retries in (1, 2, 3):
            for seconds in (9, 10, 11, 12):
                with self.subTest(retries=retries, seconds=seconds):
                    audit = self.started()
                    audit.restart_windows.append((946684810, 946684811))
                    self.retry(audit, retries)
                    failure = frame(9, "run.failed", run_id="run-example", code="provider_error")
                    failure["ts"] = "2000-01-01T00:00:{:02d}Z".format(seconds)
                    if retries == 3 and seconds in (10, 11):
                        audit.record(failure)
                        self.assertEqual(audit.failed, 1)
                    else:
                        with self.assertRaises(AuditError):
                            audit.record(failure)

    def test_a_store_outage_failure_is_accepted_only_inside_a_database_window(self):
        # GW-E-020: every attempt spent on 503 store_unavailable during a database outage ends run.failed by design.
        for reason, window, accepted in (("gateway.store_unavailable 503", (946684800, float("inf")), True),
                                         ("gateway.store_unavailable 503", (946684700, 946684705), False),
                                         ("transport", (946684800, float("inf")), False)):
            with self.subTest(reason=reason, window=window):
                audit = self.started()
                audit.database_windows.append(window)
                self.retry(audit, 3)
                failure = frame(9, "run.failed", run_id="run-example", code="provider_error", reason=reason)
                failure["ts"] = "2000-01-01T00:00:10Z"
                if accepted:
                    audit.record(failure)
                    self.assertEqual(audit.failed, 1)
                else:
                    with self.assertRaises(AuditError):
                        audit.record(failure)

    def test_a_provider_failure_inside_its_own_link_cut_now_fails(self):
        # The runtime waits for its own link before spending a model attempt (CORE-U-062, RT-I-033): a run failed by a
        # link cut is a product failure again, with no allowance.
        audit = self.started()
        self.assertFalse(hasattr(audit, "link_cut_windows"))
        self.retry(audit, 3)
        with self.assertRaises(AuditError):
            audit.record(frame(9, "run.failed", run_id="run-example", code="provider_error"))

    def given_back_then_spent(self, audit):
        """Attempt 1 is given back (the runtime's link was down), then attempts 2-4 are spent."""
        audit.record(frame(3, "step.started", step=1, attempt=1))
        audit.record(frame(4, "provider.retry", attempt=1, delay_ms=45000, reason="gateway_link"))
        for attempt in (2, 3, 4):
            audit.record(frame(2 * attempt + 1, "step.started", step=1, attempt=attempt))
            if attempt < 4:
                audit.record(frame(2 * attempt + 2, "provider.retry", attempt=attempt, delay_ms=10,
                                   reason="transport"))

    def test_a_given_back_attempt_is_not_counted_among_the_spent_ones(self):
        audit = self.started()
        audit.restart_windows.append((946684810, 946684811))
        self.given_back_then_spent(audit)
        audit.record(frame(11, "run.failed", run_id="run-example", code="provider_error"))
        self.assertEqual((audit.failed, audit.link_waits), (1, [45000]))

    def test_a_given_back_attempt_does_not_stand_in_for_a_spent_one(self):
        # Two attempts spent and one given back is not "every attempt": the failure is not the restart's.
        audit = self.started()
        audit.restart_windows.append((946684810, 946684811))
        audit.record(frame(3, "step.started", step=1, attempt=1))
        audit.record(frame(4, "provider.retry", attempt=1, delay_ms=45000, reason="gateway_link"))
        for attempt in (2, 3):
            audit.record(frame(2 * attempt + 1, "step.started", step=1, attempt=attempt))
        audit.record(frame(6, "provider.retry", attempt=2, delay_ms=10, reason="transport"))
        with self.assertRaises(AuditError):
            audit.record(frame(11, "run.failed", run_id="run-example", code="provider_error"))

    def test_wrong_code_cannot_hide_inside_a_planned_restart_window(self):
        audit = self.started()
        audit.restart_windows.append((946684800, 946684900))
        self.retry(audit, 3)
        with self.assertRaises(AuditError):
            audit.record(frame(9, "run.failed", run_id="run-example", code="io"))

    def test_restart_classification_accepts_the_protocols_nanosecond_timestamps(self):
        audit = self.started()
        audit.restart_windows.append((946684810, 946684811))
        self.retry(audit, 3)
        failure = frame(9, "run.failed", run_id="run-example", code="provider_error")
        failure["ts"] = "2000-01-01T00:00:10.123456789Z"
        audit.record(failure)
        self.assertEqual(audit.failed, 1)

    def tool(self, audit, name="shell", identity="call-example"):
        audit.record(frame(3, "tool.call", call_id=identity, name=name))
        audit.record(frame(4, "tool.start", call_id=identity))

    def test_only_a_timeout_from_an_inflight_tool_inside_its_own_pause_window_is_allowed(self):
        for when, content, status in [(19, "timeout: tool deadline elapsed", "error"),
                                      (20, "timeout: tool deadline elapsed", "error"),
                                      (31, "timeout: tool deadline elapsed", "error"),
                                      (20, "io: failed", "error"), (20, "cancelled", "cancelled")]:
            audit = self.started()
            self.tool(audit)
            audit.pause_windows.append({"start": 946684815, "end": 946684820})
            result = frame(5, "tool.result", call_id="call-example", status=status, content=content)
            result["ts"] = "2000-01-01T00:00:{:02d}Z".format(when)
            with self.subTest(when=when, content=content):
                if when in (19, 20) and content == "timeout: tool deadline elapsed" and status == "error":
                    audit.record(result)
                    self.assertEqual(len(audit.tool_timeouts), 1)
                else:
                    with self.assertRaises(AuditError):
                        audit.record(result)

    def test_same_timeout_without_a_pause_or_from_a_tool_started_after_it_fails(self):
        for window in (None, {"start": 946684800, "end": 946684810}):
            audit = self.started()
            self.tool(audit)
            if window is not None:
                audit.pause_windows.append(window)
            with self.assertRaises(AuditError):
                audit.record(frame(5, "tool.result", call_id="call-example", status="error",
                                   content="timeout: tool deadline elapsed"))

    def at(self, event, seconds):
        event["ts"] = "2000-01-01T00:{:02d}:{:02d}Z".format(*divmod(seconds, 60))
        return event

    def test_a_tool_running_when_its_sandbox_was_killed_may_come_back_interrupted(self):
        audit = self.started()
        self.tool(audit)
        audit.kill_windows.append((946684815, 946684815))
        audit.record(self.at(frame(5, "tool.result", call_id="call-example", status="interrupted", content=""), 20))
        self.assertEqual(audit.tool_outcomes, {"shell.interrupted_by_kill": 1})

    def test_a_tool_running_when_the_disk_filled_may_come_back_interrupted_by_its_replacement(self):
        # The gateway replaces a sandbox lost on a full disk and the run resumes: the tool in flight is interrupted,
        # as after a kill (the candidate 6 rehearsal: started 2.8 s before the fill, interrupted 6 s into it).
        audit = self.started()
        self.tool(audit)
        audit.disk_windows.append([946684815, 946684818])
        audit.record(self.at(frame(5, "tool.result", call_id="call-example", status="interrupted", content=""), 20))
        self.assertEqual(audit.tool_outcomes, {"shell.interrupted_by_full_disk": 1})

    def test_an_interrupted_tool_outside_the_disk_window_and_margin_or_started_after_it_fails(self):
        for window, start in (([946684600, 946684610], None), ([946684815, 946684818], 19)):
            with self.subTest(window=window, start=start):
                audit = self.started()
                audit.record(frame(3, "tool.call", call_id="call-example", name="shell"))
                started = frame(4, "tool.start", call_id="call-example")
                audit.record(self.at(started, start) if start is not None else started)
                audit.disk_windows.append(window)
                with self.assertRaises(AuditError):
                    audit.record(self.at(frame(5, "tool.result", call_id="call-example", status="interrupted",
                                               content=""), 20))

    def test_an_interrupted_tool_without_a_kill_or_started_after_it_fails(self):
        for kill, when in ((None, 20), (946684805, 20), (946684815, 59), (946684815, 14)):
            with self.subTest(kill=kill, when=when):
                audit = self.started()
                self.tool(audit)
                if kill is not None:
                    audit.kill_windows.append((kill, kill))
                result = frame(5, "tool.result", call_id="call-example", status="interrupted", content="")
                result["ts"] = "2000-01-01T00:{:02d}:{:02d}Z".format(*divmod(when if when != 59 else 15 + 181, 60))
                with self.assertRaises(AuditError):
                    audit.record(result)

    def test_a_run_resumes_once_inside_its_own_kill_window_and_never_parks(self):
        audit = self.started()
        audit.kill_windows.append((946684815, 946684815))
        audit.record(self.at(frame(6, "run.resumed", run_id="run-example"), 20))
        self.assertEqual(audit.resumed, 1)
        with self.assertRaisesRegex(AuditError, "resumed"):
            audit.record(self.at(frame(7, "run.resumed", run_id="run-example"), 21))
        with self.assertRaisesRegex(AuditError, "parked"):
            audit.record(self.at(frame(8, "run.parked", run_id="run-example"), 22))

    def test_up_to_four_resumes_per_full_disk_window_each_after_its_backoff_and_a_fifth_is_the_loop_guard(self):
        # The product's bound: 4 replacements in 10 minutes, waiting 0, 5, 15 and 45 s (GW-U-070 of the fix).
        audit = self.started()
        audit.disk_windows.append([946684815, 946684818])
        for seq, when in enumerate((20, 26, 42, 88), 6):
            audit.record(self.at(frame(seq, "run.resumed", run_id="run-example"), when))
        self.assertEqual(audit.disk_resumes, [946684820, 946684826, 946684842, 946684888])
        with self.assertRaisesRegex(AuditError, "5th resume in one disk_full window"):
            audit.record(self.at(frame(10, "run.resumed", run_id="run-example"), 150))

    def test_a_resume_sooner_than_its_backoff_is_the_loop_guard(self):
        for first, second in ((20, 23), (20, 20)):
            with self.subTest(second=second):
                audit = self.started()
                audit.disk_windows.append([946684815, 946684818])
                audit.record(self.at(frame(6, "run.resumed", run_id="run-example"), first))
                with self.assertRaisesRegex(AuditError, "sooner than the product's backoff"):
                    audit.record(self.at(frame(7, "run.resumed", run_id="run-example"), second))

    def test_a_resume_after_the_disk_margin_still_needs_a_kill(self):
        audit = self.started()
        audit.disk_windows.append([946684600, 946684610])  # the margin ends 180 s after 610, before 820
        with self.assertRaisesRegex(AuditError, "resumed"):
            audit.record(self.at(frame(6, "run.resumed", run_id="run-example"), 20))

    def test_a_resume_without_a_kill_fails(self):
        audit = self.started()
        with self.assertRaisesRegex(AuditError, "resumed"):
            audit.record(self.at(frame(6, "run.resumed", run_id="run-example"), 20))

    def test_only_no_space_errors_inside_a_disk_full_window_are_allowed(self):
        cases = [(20, "error", "sh: can't create soak.txt: No space left on device", True),
                 (20, "error", "io: failed", False),
                 (40, "error", "sh: can't create soak.txt: No space left on device", False)]
        for when, status, content, allowed in cases:
            with self.subTest(when=when, content=content):
                audit = self.started()
                self.tool(audit)
                audit.disk_windows.append([946684815, 946684825])
                result = self.at(frame(5, "tool.result", call_id="call-example", status=status, content=content), when)
                if allowed:
                    audit.record(result)
                    self.assertEqual(audit.tool_outcomes, {"shell.no_space": 1})
                else:
                    with self.assertRaises(AuditError):
                        audit.record(result)

    def test_the_new_fault_kinds_are_known(self):
        audit = self.started()
        for kind in ("gateway_kill", "db_restart", "db_outage", "db_crash", "engine_stall", "disk_full",
                     "sandbox_kill", "link_cut"):
            audit.fault(kind, 2)
        with self.assertRaises(AuditError):
            audit.fault("meteor", 2)

    def test_workload_requires_successful_shell_and_read_results_despite_allowed_timeouts(self):
        audit = self.started()
        for index, name in enumerate(("shell", "read_file")):
            with self.assertRaises(AuditError):
                audit.verify_workload()
            identity = "call-{}".format(index)
            self.tool(audit, name, identity)
            audit.record(frame(5 + index, "tool.result", call_id=identity, status="ok", content="done"))
        audit.verify_workload()

    def test_unfinished_and_duplicate_terminals_fail_and_completion_after_fault_is_required(self):
        audit = self.started()
        with self.assertRaises(AuditError):
            audit.verify()
        audit.fault("client_sse", 2)
        audit.record(frame(3, "run.completed", run_id="run-example"))
        audit.verify()
        with self.assertRaises(AuditError):
            audit.record(frame(4, "run.completed", run_id="run-example"))
        audit.fault("gateway_restart", 4)
        with self.assertRaises(AuditError):
            audit.verify()


if __name__ == "__main__":
    unittest.main()


class FaultWindowStampTests(unittest.TestCase):
    """A fault's window is stamped around the moment it takes effect (soak 47c679f55, 21 Nov: a shell call started
    23 ms after the pause window's stamp, before `docker pause` froze it, timed out while paused, and was rejected)."""
    BASE = 946684800.0

    @staticmethod
    def iso(seconds):
        return datetime.datetime.fromtimestamp(seconds, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")

    def at(self, seq, kind, seconds, **fields):
        event = frame(seq, kind, **fields)
        event["ts"] = self.iso(self.BASE + seconds)
        return event

    def driver(self, clock):
        audit = RunAudit()
        audit.record(self.at(1, "message.user", 1, text="work"))
        audit.record(self.at(2, "run.started", 1, run_id="run-example"))
        paused = {}

        def pause(conversation):
            clock[0] += 0.030  # the lookup, then `docker pause`: frozen when it returns
            paused[conversation] = {"container": "runtime-1"}
            return paused[conversation]

        def unpause(conversation):
            clock[0] += 0.020
            del paused[conversation]

        def kill_sandbox(conversation):
            clock[0] += 0.030  # the lookup, then `docker kill`: dead when it returns
            return "runtime-1"
        driver = soak.Soak.__new__(soak.Soak)
        driver.slots, driver.conversations, driver.audits, driver.windows = 1, ["task-1"], [audit], []
        driver.clients = [types.SimpleNamespace(receipts=types.SimpleNamespace(last_seq=2), snapshots=0, connections=1)
                          for _ in range(2)]
        driver.stack = types.SimpleNamespace(paused=paused, pause=pause, unpause=unpause, kill_sandbox=kill_sandbox,
                                             gateway="gateway-1", logs=lambda *args: "")
        driver.started, driver.pending, driver.faults = 0.0, [], []
        driver.log = lambda kind, **fields: None
        return driver, audit

    def clocked(self, clock):
        now = lambda: self.BASE + clock[0]  # noqa: E731
        return mock.patch.object(soak.time, "time", now), mock.patch.object(soak.time, "monotonic", now)

    def test_a_call_started_before_the_freeze_but_after_the_fault_was_asked_for_may_time_out_while_paused(self):
        for start, result, allowed in [(15.023, 164.15, True), (14.5, 164.15, True), (164.3, 165.0, False)]:
            with self.subTest(start=start):
                clock = [15.0]
                driver, audit = self.driver(clock)
                wall, monotonic = self.clocked(clock)
                with wall, monotonic:
                    driver.inject({"kind": "runtime_link", "target": 0, "pause_seconds": 149})
                    clock[0] = 164.13
                    driver.resumed_link(driver.pending[0])
                audit.record(self.at(3, "tool.call", start, call_id="call-1", name="shell"))
                audit.record(self.at(4, "tool.start", start, call_id="call-1"))
                timeout = self.at(5, "tool.result", result, call_id="call-1", status="error",
                                  content="timeout: tool deadline elapsed")
                if allowed:
                    audit.record(timeout)
                    self.assertEqual(len(audit.tool_timeouts), 1)
                else:
                    with self.assertRaisesRegex(AuditError, "outside its in-flight pause window"):
                        audit.record(timeout)

    def test_a_call_started_while_its_sandbox_was_being_killed_may_come_back_interrupted(self):
        # Interrupted 20 ms after it started, before `docker kill` returned; one started after the kill fails.
        for start, result, allowed in [(15.010, 15.020, True), (14.0, 15.5, True), (15.5, 16.0, False)]:
            with self.subTest(start=start):
                clock = [15.0]
                driver, audit = self.driver(clock)
                wall, monotonic = self.clocked(clock)
                with wall, monotonic:
                    driver.inject({"kind": "sandbox_kill", "target": 0})
                audit.record(self.at(3, "tool.call", start, call_id="call-1", name="shell"))
                audit.record(self.at(4, "tool.start", start, call_id="call-1"))
                interrupted = self.at(5, "tool.result", result, call_id="call-1", status="interrupted", content="")
                if allowed:
                    audit.record(interrupted)
                    self.assertEqual(audit.tool_outcomes, {"shell.interrupted_by_kill": 1})
                else:
                    with self.assertRaisesRegex(AuditError, "outside its sandbox-kill"):
                        audit.record(interrupted)
