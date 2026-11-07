"""DEP-U-004 to DEP-U-006: the database and load readings beside a test stack (readings.py), offline: a fake docker
runner and a fake /proc; no Docker daemon, no Postgres."""

from contextlib import redirect_stderr
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import compose
import readings
import soak_chaos

S = readings.SEPARATOR
CONTAINER = "c" * 64
PASSWORD = "p" * 64
QUIET = S.join(["7", "1", "0", "0", "0.4", "0", "0", "0"])
HELD = S.join(["8", "2", "1", "1", "21.5", "21.5", "1", "1"])
SESSIONS = "\n".join([
    S.join(["S", "101", "idle in transaction", "tqa-gateway", "21.5", "21.5", "", "{}",
            "UPDATE workspaces SET last_used_at = now() WHERE id = $1"]),
    S.join(["S", "202", "active", "tqa-gateway", "6.0", "6.0", "Lock:transactionid", "{101}",
            "UPDATE workspaces SET last_used_at = now() WHERE id = $1"]),
    S.join(["S", "303", "idle in transaction", "psql", "30.0", "29.0", "", "{}", ""]),
    S.join(["L", "202", "transactionid", "ShareLock", "f", "", "", "48211", ""]),
    S.join(["L", "202", "relation", "RowExclusiveLock", "t", "workspaces", "", "", ""]),
    S.join(["L", "202", "tuple", "ExclusiveLock", "t", "workspaces", "3", "", ""]),
    S.join(["L", "101", "transactionid", "ExclusiveLock", "t", "", "", "48211", ""]),
])


class Run:
    """A fake `subprocess.run` for docker: `ps` lists the container, `exec` answers each psql in turn."""

    def __init__(self, answers, container=CONTAINER):
        self.answers, self.container, self.calls = list(answers), container, []

    def __call__(self, args, **options):
        self.calls.append((args, options))
        if args[1] == "ps":
            return subprocess.CompletedProcess(args, 0, stdout=self.container + "\n", stderr="")
        answer = self.answers.pop(0) if self.answers else QUIET
        if isinstance(answer, Exception):
            raise answer
        return subprocess.CompletedProcess(args, 0, stdout=answer + "\n", stderr="")


def proc(folder, load="1.20", io_full="0.00", cpu_some="0.00"):
    folder = Path(folder)
    (folder / "pressure").mkdir(parents=True, exist_ok=True)
    (folder / "loadavg").write_text(load + " 1.00 0.90 2/900 12345\n")
    (folder / "pressure" / "io").write_text(
        "some avg10=0.10 avg60=0.00 avg300=0.00 total=1\nfull avg10=" + io_full + " avg60=0.00 avg300=0.00 total=1\n")
    (folder / "pressure" / "cpu").write_text(
        "some avg10=" + cpu_some + " avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n")
    (folder / "pressure" / "memory").write_text(
        "some avg10=0.00 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n")
    return folder


def env(directory, **extra):
    return dict({"COMPOSE_PROJECT_NAME": "tqa-compose-0123456789", "TQA_INSTALL_DIR": str(directory),
                 "TQA_POSTGRES_PASSWORD": PASSWORD}, **extra)


def lines(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines()]


class ReaderTests(unittest.TestCase):
    """DEP-U-004: each poll's reading, its thresholds and bounds, and what it never keeps."""

    def reader(self, folder, answers, **host):
        run = Run(answers)
        reader = readings.Readings(env(folder), Path(folder) / readings.FILE, proc=proc(Path(folder) / "proc", **host),
                                   run=run, clock=lambda: 1_792_660_000.0)
        return reader, run

    # TEST: DEP-U-004
    def test_a_quiet_poll_is_a_beat_with_host_and_database(self):
        with tempfile.TemporaryDirectory() as folder:
            reader, run = self.reader(folder, [QUIET])
            reader.poll()
            beat, = [line for line in lines(reader.path) if line["kind"] != "note"]
            self.assertEqual(beat["kind"], "beat")
            self.assertEqual(beat["host"]["load1"], 1.2)
            self.assertEqual(beat["host"]["io"], {"some": 0.1, "full": 0.0})
            self.assertEqual(beat["db"]["backends"], 7)
            self.assertEqual(beat["db"]["oldest_transaction_seconds"], 0.4)
            # One psql call for a quiet poll; it names itself and finds Postgres by the project's labels.
            execs = [args for args, _ in run.calls if args[1] == "exec"]
            self.assertEqual(len(execs), 1)
            self.assertIn("PGAPPNAME=" + readings.APPLICATION, execs[0])
            listing = [args for args, _ in run.calls if args[1] == "ps"][0]
            self.assertIn("label=com.docker.compose.project=tqa-compose-0123456789", listing)
            self.assertIn("label=com.docker.compose.service=postgres", listing)

    # TEST: DEP-U-004
    def test_a_held_row_is_an_alert_with_its_sessions_and_locks(self):
        with tempfile.TemporaryDirectory() as folder:
            reader, _ = self.reader(folder, [HELD, SESSIONS])
            reader.poll()
            alert = next(line for line in lines(reader.path) if line["kind"] == "alert")
            self.assertEqual(alert["why"], ["blocked", "idle_in_transaction", "old_transaction"])
            waiter = next(session for session in alert["sessions"] if session["pid"] == 202)
            self.assertEqual(waiter["blocked_by"], [101])
            self.assertIn({"pid": 202, "type": "relation", "mode": "RowExclusiveLock", "granted": True,
                           "relation": "workspaces", "tuple": None, "transaction": None, "object": None},
                          alert["locks"])
            summary = "\n".join(reader.summary())
            # As on a real Postgres: the ungranted lock is a transaction id; the row is named by the tuple lock.
            self.assertIn("longest block: pid 202 waited 6.0 s on workspaces", summary)
            self.assertIn("holder pid 101 (idle in transaction, tqa-gateway, transaction 21.5 s)", summary)
            self.assertIn("longest idle transaction: 21.5 s", summary)

    # TEST: DEP-U-004
    def test_only_the_gateways_own_statements_are_kept(self):
        with tempfile.TemporaryDirectory() as folder:
            reader, _ = self.reader(folder, [HELD, SESSIONS])
            reader.poll()
            alert = next(line for line in lines(reader.path) if line["kind"] == "alert")
            other = next(session for session in alert["sessions"] if session["pid"] == 303)
            self.assertIsNone(other["statement"], "another client's statement may hold literals")
            # The query itself only returns a statement for the gateway's application name.
            self.assertIn("CASE WHEN application_name = 'tqa-gateway'", readings.SESSIONS_QUERY)
            for query in (readings.SUMMARY_QUERY, readings.SESSIONS_QUERY, readings.LOCKS_QUERY):
                self.assertIn("application_name <> 'tqa-stack-readings'", query, "the reader leaves itself out")

    # TEST: DEP-U-004
    def test_each_threshold_alerts_and_just_under_it_does_not(self):
        quiet = {"blocked": 0, "idle_in_transaction_alerts": 0, "oldest_transaction_seconds": 4.9}
        host = {"io": {"full": 9.9}, "cpu": {"some": 49.9}}
        self.assertEqual(readings.reasons(host, quiet), [])
        self.assertEqual(readings.reasons(host, dict(quiet, oldest_transaction_seconds=5.0)), ["old_transaction"])
        self.assertEqual(readings.reasons({"io": {"full": 10.0}, "cpu": {"some": None}}, None), ["io_pressure"])
        self.assertEqual(readings.reasons({"io": {"full": None}, "cpu": {"some": 50.0}}, None), ["cpu_pressure"])

    # TEST: DEP-U-004
    def test_beats_every_sixth_poll_and_alerts_stop_at_the_cap(self):
        with tempfile.TemporaryDirectory() as folder:
            reader, _ = self.reader(folder, [QUIET] * 13)
            for _ in range(13):
                reader.poll()
            self.assertEqual(sum(line["kind"] == "beat" for line in lines(reader.path)), 3)
        with tempfile.TemporaryDirectory() as folder, patch.object(readings, "ALERT_CAP", 2):
            reader, _ = self.reader(folder, [QUIET] * 4, io_full="12.00")
            for _ in range(4):
                reader.poll()
            written = lines(reader.path)
            self.assertEqual(sum(line["kind"] == "alert" for line in written), 2)
            self.assertEqual(sum("beats only from here" in line.get("note", "") for line in written), 1)

    # TEST: DEP-U-004
    def test_a_failed_poll_is_noted_redacted_and_the_next_one_runs(self):
        failure = subprocess.CalledProcessError(2, ["docker"], stderr="FATAL: password " + PASSWORD + " rejected")
        with tempfile.TemporaryDirectory() as folder:
            reader, run = self.reader(folder, [QUIET, failure, QUIET])
            for _ in range(3):
                reader.poll()
            text = Path(reader.path).read_text()
            self.assertNotIn(PASSWORD, text)
            self.assertIn("[REDACTED]", text)
            self.assertEqual(reader.errors, 1)
            # The container is looked for again after a failure (a restarted database).
            self.assertEqual(sum(args[1] == "ps" for args, _ in run.calls), 2)
            self.assertEqual(reader.polls, 3)
            self.assertIn("poll_error", next(line for line in lines(reader.path) if line["kind"] == "alert")["why"])

    # TEST: DEP-U-004
    def test_each_psql_is_bounded_inside_the_container_and_by_its_own_client(self):
        with tempfile.TemporaryDirectory() as folder:
            reader, run = self.reader(folder, [QUIET])
            reader.poll()
            args, options = next(call for call in run.calls if call[0][1] == "exec")
            self.assertIn("PGOPTIONS=-c statement_timeout=3000 -c lock_timeout=1000", args)
            self.assertIn("PGCONNECT_TIMEOUT=3", args)
            self.assertEqual(args[args.index(CONTAINER) + 1:args.index(CONTAINER) + 4], ["timeout", "3", "psql"])
            # A stalled dockerd hangs the client itself, which no Postgres bound ends: every docker call has its own.
            self.assertTrue(all(call[1]["timeout"] == readings.POLL_TIMEOUT for call in run.calls))
            self.assertLess(readings.CONTAINER_SECONDS, readings.POLL_TIMEOUT)
            self.assertGreaterEqual(readings.STOP_TIMEOUT, 3 * readings.POLL_TIMEOUT)

    # TEST: DEP-U-004
    def test_postgres_starting_is_noted_once_not_alerted(self):
        refused = subprocess.CalledProcessError(2, ["docker"], stderr="psql: error: connection refused")
        with tempfile.TemporaryDirectory() as folder:
            reader, _ = self.reader(folder, [refused, refused, refused, QUIET])
            for _ in range(4):
                reader.poll()
            written = lines(reader.path)
            self.assertFalse([line for line in written if line["kind"] == "alert"])
            self.assertEqual(sum("waiting for the stack's Postgres" in line.get("note", "") for line in written), 1)
            self.assertEqual(reader.errors, 0)

    # TEST: DEP-U-004
    def test_teardown_is_not_an_anomaly_and_ends_the_poll_early(self):
        refused = subprocess.CalledProcessError(2, ["docker"], stderr="No such container")
        with tempfile.TemporaryDirectory() as folder:
            reader, run = self.reader(folder, [QUIET, refused, HELD])
            reader.poll()
            reader.stopping.set()
            reader.poll()
            reader.poll()
            written = lines(reader.path)
            self.assertFalse([line for line in written if line["kind"] == "alert" and "poll_error" in line["why"]])
            self.assertEqual(reader.errors, 0)
            # Told to stop: no second psql (sessions and locks) into a stack that is going.
            self.assertEqual(sum(args[1] == "exec" for args, _ in run.calls), 3)

    # TEST: DEP-U-004
    def test_a_session_without_the_gateways_name_keeps_no_statement(self):
        unnamed = SESSIONS.replace(S + "tqa-gateway" + S, S + "" + S)
        with tempfile.TemporaryDirectory() as folder:
            reader, _ = self.reader(folder, [HELD, unnamed])
            reader.poll()
            alert = next(line for line in lines(reader.path) if line["kind"] == "alert")
            self.assertTrue(all(session["statement"] is None for session in alert["sessions"]))
            self.assertIn("holder pid 101 (idle in transaction, unnamed, transaction 21.5 s)", "\n".join(reader.summary()))

    # TEST: DEP-U-004
    def test_malformed_rows_are_noted_not_fatal(self):
        self.assertIsNone(readings.database_summary("not a row"))
        sessions, locks, malformed = readings.sessions_and_locks("S" + S + "x\ngarbage\n" + SESSIONS)
        self.assertEqual((len(sessions), len(locks), malformed), (3, 4, 2))

    # TEST: DEP-U-004
    def test_the_host_reading_tolerates_a_kernel_without_pressure(self):
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / "loadavg").write_text("3.5 1 1 1/1 1\n")
            reading = readings.host(Path(folder))
            self.assertEqual(reading["load1"], 3.5)
            self.assertEqual(reading["io"], {"some": None, "full": None})


class HookTests(unittest.TestCase):
    """DEP-U-005: compose's hooks start, keep and stop the reader; a reader never fails a run."""

    def tearDown(self):
        readings._READERS.clear()

    # TEST: DEP-U-005
    def test_start_only_in_a_docker_run_and_not_when_switched_off(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(readings.Readings, "start") as started:
            self.assertIsNone(readings.start(env(folder)), "a unit test's fake installation starts none")
            self.assertIsNone(readings.start(env(folder, TQA_DOCKER_TESTS="1", TQA_STACK_READINGS="off")))
            reader = readings.start(env(folder, TQA_DOCKER_TESTS="1", TQA_STACK_READINGS_INTERVAL="0.5"))
            self.assertIsNotNone(reader)
            self.assertEqual(reader.interval, readings.MINIMUM_INTERVAL_SECONDS)
            started.assert_called_once()
            # Not in the installation: prepare hands that folder to the gateway's user, and this user can't write there.
            self.assertNotIn(Path(folder), reader.path.parents)
            self.assertTrue(reader.path.parent.name.startswith("tqa-stack-readings-"))
            with redirect_stderr(io.StringIO()):
                readings.stop(env(folder))
            self.assertFalse(reader.path.parent.exists(), "stop removes the reader's own folder")

    # TEST: DEP-U-005
    def test_a_reader_that_cannot_start_never_fails_prepare(self):
        """A full /tmp or a thread that won't start: one line, no reader, and prepare goes on."""
        with tempfile.TemporaryDirectory() as folder:
            docker_run = env(folder, TQA_DOCKER_TESTS="1")
            full = OSError(28, "No space left on device")
            with patch.object(readings.tempfile, "mkdtemp", side_effect=full), \
                    redirect_stderr(io.StringIO()) as printed:
                self.assertIsNone(readings.start(docker_run))
            self.assertIn("No stack readings for this run", printed.getvalue())
            self.assertNotIn(docker_run["COMPOSE_PROJECT_NAME"], readings._READERS)
            made = Path(folder) / "made"
            made.mkdir()
            with patch.object(readings.tempfile, "mkdtemp", return_value=str(made)), \
                    patch.object(readings.threading.Thread, "start", side_effect=RuntimeError("can't start new thread")), \
                    redirect_stderr(io.StringIO()) as printed:
                self.assertIsNone(readings.start(docker_run))
            self.assertIn("can't start new thread", printed.getvalue())
            self.assertNotIn(docker_run["COMPOSE_PROJECT_NAME"], readings._READERS)
            self.assertFalse(made.exists(), "the reader's folder doesn't outlive a failed start")

    # TEST: DEP-U-005
    def test_save_logs_keeps_the_readings_and_their_summary(self):
        with tempfile.TemporaryDirectory() as folder:
            reader = readings.Readings(env(folder), Path(folder) / readings.FILE, proc=proc(Path(folder) / "proc"),
                                       run=Run([QUIET]))
            readings._READERS[reader.project] = reader
            reader.poll()
            kept = Path(folder) / "kept"
            with patch.object(compose, "command", side_effect=OSError("no daemon")), redirect_stderr(io.StringIO()):
                written = compose.save_logs(env(folder), kept, configuration=["compose"])
            self.assertIn(kept / readings.FILE, written)
            self.assertIn(kept / readings.SUMMARY, written)
            self.assertIn("peak load 1.2", (kept / readings.SUMMARY).read_text())

    def retired(self, folder, failure, **extra):
        """compose.retire over a polled reader, recording the order of the reader's steps and teardown's commands."""
        order = []
        fixture = env(folder, TQA_SANDBOX_NETWORK="tqa-compose-0123456789-sandboxes", **extra)
        reader = readings.Readings(fixture, Path(folder) / "reader" / readings.FILE, proc=proc(Path(folder) / "proc"),
                                   run=Run([QUIET]))
        readings._READERS[reader.project] = reader
        reader.poll()
        real_stop = reader.stop

        def stop():
            order.append("join")
            real_stop()

        def command(args, *_args, **_options):
            order.append("stopping set" if reader.stopping.is_set() else "command before stopping")
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

        with patch.object(reader, "stop", side_effect=stop), patch.object(compose, "command", side_effect=command), \
                patch.object(compose, "guard_sandbox_images", side_effect=lambda _env, failure: failure), \
                redirect_stderr(io.StringIO()) as printed:
            result = compose.retire(fixture, failure, configuration=["compose"])
        return result, order, printed.getvalue(), reader

    # TEST: DEP-U-005
    def test_retire_tells_the_reader_to_stop_first_and_waits_for_it_last(self):
        with tempfile.TemporaryDirectory() as folder:
            target = Path(folder) / "evidence"
            result, order, printed, reader = self.retired(folder, None, TQA_STACK_READINGS_DIR=str(target))
            self.assertIsNone(result)
            self.assertTrue(order and set(order[:-1]) == {"stopping set"}, order)
            self.assertEqual(order[-1], "join", "teardown never waits on the reader; it is waited for last")
            self.assertTrue((target / "tqa-compose-0123456789-readings.jsonl").exists())
            self.assertTrue((target / "tqa-compose-0123456789-readings-summary.txt").exists())
            self.assertIn("Stack readings: peak load", printed)
            self.assertNotIn(reader.project, readings._READERS)

    # TEST: DEP-U-005
    def test_any_failure_keeps_the_readings_without_a_folder_named(self):
        with tempfile.TemporaryDirectory() as folder:
            result, _, printed, reader = self.retired(folder, AssertionError("PERF-011 failed"))
            self.assertIsInstance(result, AssertionError)
            kept = Path(printed.split("Kept the stack's readings: ")[1].splitlines()[0])
            try:
                self.assertTrue(kept.name.startswith("tqa-stack-readings-kept-"))
                self.assertTrue((kept / "tqa-compose-0123456789-readings.jsonl").exists())
            finally:
                import shutil
                shutil.rmtree(kept, ignore_errors=True)
        with tempfile.TemporaryDirectory() as folder:
            _, _, printed, _ = self.retired(folder, None)
            self.assertNotIn("Kept the stack's readings", printed, "a passing run without a folder keeps nothing")

    # TEST: DEP-U-005
    def test_an_interrupt_in_teardown_still_cleans_the_sandboxes_and_keeps_the_readings(self):
        with tempfile.TemporaryDirectory() as folder:
            fixture = env(folder, TQA_SANDBOX_NETWORK="tqa-compose-0123456789-sandboxes")
            reader = readings.Readings(fixture, Path(folder) / "reader" / readings.FILE,
                                       proc=proc(Path(folder) / "proc"), run=Run([QUIET]))
            readings._READERS[reader.project] = reader
            reader.poll()
            with patch.object(compose, "command", side_effect=KeyboardInterrupt("signal 15")), \
                    redirect_stderr(io.StringIO()) as printed:
                with self.assertRaises(KeyboardInterrupt):
                    compose.retire(fixture, None, configuration=["compose"])
            kept = Path(printed.getvalue().split("Kept the stack's readings: ")[1].splitlines()[0])
            self.assertTrue((kept / "tqa-compose-0123456789-readings.jsonl").exists())
            import shutil
            shutil.rmtree(kept, ignore_errors=True)

    # TEST: DEP-U-005
    def test_after_stop_nothing_is_written_and_the_folder_stays_gone(self):
        with tempfile.TemporaryDirectory() as folder:
            fixture = env(folder)
            made = Path(folder) / "tqa-stack-readings-test"
            reader = readings.Readings(fixture, made / readings.FILE, proc=proc(Path(folder) / "proc"), run=Run([]))
            readings._READERS[reader.project] = reader
            reader.poll()
            with redirect_stderr(io.StringIO()):
                readings.stop(fixture)
            self.assertFalse(made.exists())
            reader.note("a poll still running after the join")
            self.assertFalse(made.exists(), "a late write doesn't make the removed folder again")

    # TEST: DEP-U-005
    def test_a_reader_left_running_is_stopped_by_retire_and_never_raises(self):
        with tempfile.TemporaryDirectory() as folder:
            fixture = env(folder, TQA_SANDBOX_NETWORK="x", TQA_STACK_READINGS_DIR="/proc/no-such-place")
            reader = readings.Readings(fixture, Path(folder) / readings.FILE, interval=0.05,
                                       proc=proc(Path(folder) / "proc"), run=Run([]))
            readings._READERS[reader.project] = reader
            reader.start()
            with redirect_stderr(io.StringIO()) as printed:
                self.assertIs(readings.stop(fixture), reader)
            self.assertFalse(reader.thread.is_alive())
            self.assertIn("Could not keep the stack's readings", printed.getvalue())
            self.assertIsNone(readings.stop(fixture), "stopping twice is nothing")


class PerfReadingsTests(unittest.TestCase):
    """DEP-U-005: a perf run's verdict comes after its stack is retired, so a slow run with no error reaches retire as a
    pass: the perf stacks always name their own result folder for the readings."""

    # TEST: DEP-U-005
    def test_a_perf_run_keeps_its_readings_in_its_result_folder(self):
        import perf_common
        fixture = {}
        folder = Path(perf_common.keep_readings(fixture, "PERF-011"))
        self.assertEqual(fixture["TQA_STACK_READINGS_DIR"], str(folder))
        self.assertEqual(folder.parent.parent, perf_common.RESULTS)
        self.assertTrue(folder.name.startswith("perf-011-readings-"))
        self.assertFalse(folder.exists(), "made only when the readings are kept")
        # An int's own folder wins.
        named = {"TQA_STACK_READINGS_DIR": "/evidence"}
        self.assertEqual(perf_common.keep_readings(named, "PERF-010"), "/evidence")
        self.assertEqual(named["TQA_STACK_READINGS_DIR"], "/evidence")

    # TEST: DEP-U-005
    def test_each_perf_stack_names_its_folder_before_its_stack_starts(self):
        for module_name, function, identity, before in (
                ("perf_start", "def main(", "PERF-010", "stack, failure = StartStack("),
                ("perf_start", "def microvm(", "PERF-011", "vm_stack.run("),
                ("perf_idle", "def main(", "PERF-021", "stack, failure = IdleStack("),
                ("perf_concurrency", "def main(", "PERF-020", "stack, failure = ConcurrencyStack(")):
            source = Path(__import__(module_name).__file__).read_text()
            body = source[source.index(function):]
            call = 'perf_common.keep_readings('
            self.assertIn(call, body, module_name + " " + function)
            self.assertIn('"' + identity + '"', body[body.index(call):body.index(call) + 80], identity)
            self.assertLess(body.index(call), body.index(before), identity)


class BackendCountTests(unittest.TestCase):
    """DEP-U-006: whatever counts the stack's backends leaves the reader out, and soak stacks start none."""

    # TEST: DEP-U-006
    def test_the_soaks_database_counts_leave_the_reader_out(self):
        query = soak_chaos.DATABASE_QUERY
        # The total and each gateway column: four counts, each without the reader.
        self.assertEqual(query.count("application_name <> 'tqa-stack-readings'"), 4, query)
        self.assertEqual(query.count("backend_type = 'client backend'"), 3)

    # TEST: DEP-U-006
    def test_only_the_soaks_own_harness_switches_the_reader_off(self):
        import soak
        source = Path(soak.__file__).read_text()
        main = source[source.index("def main("):]
        self.assertLess(main.index('env["TQA_STACK_READINGS"] = "off"'), main.index("stack = Stack("))
        # The perf stacks inherit soak_stack.Stack: neither it nor they switch the reader off.
        for name in ("soak_stack", "perf_start", "perf_idle", "perf_concurrency"):
            module = __import__(name)
            self.assertNotIn("TQA_STACK_READINGS", Path(module.__file__).read_text(), name)


if __name__ == "__main__":
    unittest.main()
