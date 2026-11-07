"""PERF-002's reproduction script without Docker or a build: the pinned database image, the checks on the result it
wrote, its refusals, and the throwaway database removed on a failure."""
import contextlib
import hashlib
import io
import itertools
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import perf_002
from soak_audit import AuditError

ON = {"fsync": "on", "synchronous_commit": "on", "full_page_writes": "on", "server_version": "18.6"}


def written(directory, database, raw=b'{"lags_ns":[1]}', sha256=None):
    """A PERF-002 result and its lag file in `directory`; returns the result's path."""
    (directory / "PERF-002-t.lag.json").write_bytes(raw)
    entry = {"name": "lag", "file": "PERF-002-t.lag.json",
             "sha256": sha256 or hashlib.sha256(raw).hexdigest()}
    path = directory / "PERF-002-t.json"
    path.write_text(json.dumps({"id": "PERF-002", "environment": {"database": database},
                                "result": {"raw_files": [entry]}, "verdict": "pass"}))
    return path


class Image(unittest.TestCase):
    def test_the_disk_run_takes_the_postgres_image_the_installation_pins(self):
        text = (perf_002.ROOT / "deploy" / "compose" / "docker.yml").read_text()
        self.assertRegex(perf_002.postgres_image(text), r"^postgres:[\w.-]+@sha256:[0-9a-f]{64}$")

    def test_an_unpinned_image_is_refused(self):
        with self.assertRaisesRegex(AuditError, "pins no postgres image by digest"):
            perf_002.postgres_image("services:\n  postgres:\n    image: postgres:18\n")


class Result(unittest.TestCase):
    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def test_a_disk_result_with_every_durability_setting_on_and_its_lag_file_passes(self):
        result = perf_002.check_result(written(self.directory, ON), disk=True)
        self.assertEqual(result["environment"]["database"]["fsync"], "on")

    def test_the_test_cluster_may_run_with_fsync_off_but_must_say_so(self):
        perf_002.check_result(written(self.directory, dict(ON, fsync="off")), disk=False)
        with self.assertRaisesRegex(AuditError, "does not record the database's synchronous_commit"):
            perf_002.check_result(written(self.directory, {"fsync": "off", "full_page_writes": "on"}), disk=False)

    def test_a_disk_run_with_a_setting_off_is_refused_by_name(self):
        with self.assertRaisesRegex(AuditError, "the disk database ran with synchronous_commit off"):
            perf_002.check_result(written(self.directory, dict(ON, synchronous_commit="off")), disk=True)

    def test_a_missing_or_changed_lag_file_is_refused(self):
        path = written(self.directory, ON, sha256="0" * 64)
        with self.assertRaisesRegex(AuditError, "PERF-002-t.lag.json changed since its result named it"):
            perf_002.check_result(path, disk=True)
        (self.directory / "PERF-002-t.lag.json").unlink()
        with self.assertRaisesRegex(AuditError, "PERF-002-t.lag.json is missing beside its result"):
            perf_002.check_result(path, disk=True)


class Refusals(unittest.TestCase):
    def refused(self, argv, env):
        with patch.dict(os.environ, env, clear=True), self.assertRaises(SystemExit) as stopped:
            perf_002.main(argv)
        return str(stopped.exception)

    def test_a_disk_run_needs_docker_tests_and_a_given_one_needs_one_database(self):
        self.assertIn("set TQA_DOCKER_TESTS=1", self.refused(["--postgres", "disk"], {}))
        self.assertIn("set DATABASE_URL and TQA_DATABASE_URL to the same database",
                      self.refused([], {"DATABASE_URL": "postgres://a", "TQA_DATABASE_URL": "postgres://b"}))


class DiskPostgres(unittest.TestCase):
    def start(self, ready=0, state="running", logs="", clock=None):
        """Enters a DiskPostgres over recorded docker calls; returns (calls, the url or the error, its env)."""
        calls, outcome = [], None

        def run(args, **kwargs):
            calls.append((list(args), kwargs.get("env")))
            if args[:2] == ["docker", "exec"]:
                return SimpleNamespace(returncode=0 if sum(c[0][:2] == ["docker", "exec"] for c in calls) > ready
                                       else 1)
            if args[:2] == ["docker", "inspect"]:
                return SimpleNamespace(returncode=0, stdout=state + "\n")
            if args[:2] == ["docker", "logs"]:
                return SimpleNamespace(returncode=0, stdout=logs, stderr="")
            return SimpleNamespace(returncode=0, stdout="")

        def command(args, env, **_):
            calls.append((list(args), env))
            return SimpleNamespace(stdout="127.0.0.1:55001\n")
        database = perf_002.DiskPostgres({})
        with patch.object(perf_002.compose, "command", side_effect=command), \
                patch.object(perf_002.subprocess, "run", side_effect=run), \
                patch.object(perf_002.time, "sleep"), \
                patch.object(perf_002.time, "monotonic", side_effect=clock or itertools.count(0, 1).__next__), \
                contextlib.redirect_stdout(io.StringIO()):
            try:
                with database as url:
                    outcome = url
            except AuditError as error:
                outcome = error
        return calls, outcome, database.env

    @staticmethod
    def removed(calls):
        return [args[1] if args[1] == "rm" else "volume" for args, _ in calls
                if args[:2] == ["docker", "rm"] or args[:3] == ["docker", "volume", "rm"]]

    def test_the_password_reaches_the_container_from_the_environment_never_the_command_line(self):
        calls, url, env = self.start(ready=2)
        run, run_env = next((args, e) for args, e in calls if args[:2] == ["docker", "run"])
        self.assertIn("POSTGRES_PASSWORD", run)
        self.assertEqual(run_env["POSTGRES_PASSWORD"], env["TQA_POSTGRES_PASSWORD"])
        self.assertTrue(env["POSTGRES_PASSWORD"])
        self.assertFalse(any(env["POSTGRES_PASSWORD"] in argument for args, _ in calls for argument in args))
        self.assertRegex(url, r"^postgres://tqa:[^@]+@127\.0\.0\.1:55001/gateway$")
        self.assertEqual(self.removed(calls), ["rm", "volume"])

    def test_a_container_that_exits_while_starting_is_reported_with_its_redacted_log_and_removed(self):
        def logs(env):
            return "FATAL: password {} rejected\n".format(env["TQA_POSTGRES_PASSWORD"])
        database = perf_002.DiskPostgres({})
        password = database.env["TQA_POSTGRES_PASSWORD"]
        with patch.object(perf_002.secrets, "token_urlsafe", return_value=password):
            calls, error, _ = self.start(ready=10_000, state="exited", logs=logs(database.env))
        self.assertIsInstance(error, AuditError)
        self.assertIn("PostgreSQL exited before it was ready", str(error))
        self.assertIn("[REDACTED]", str(error))
        self.assertNotIn(password, str(error))
        self.assertEqual(self.removed(calls), ["rm", "volume"])

    def test_a_database_not_ready_within_its_bound_is_refused_and_removed(self):
        self.assertEqual(perf_002.READY_SECONDS, 180)
        calls, error, _ = self.start(ready=10_000, clock=itertools.count(0, 30).__next__)
        self.assertEqual(str(error), "PostgreSQL was not ready within 180 s")
        self.assertEqual(self.removed(calls), ["rm", "volume"])

    def test_the_container_and_its_volume_are_removed_when_the_run_fails(self):
        removed = []

        def run(args, **_):
            if args[:2] == ["docker", "rm"] or args[:3] == ["docker", "volume", "rm"]:
                removed.append(args[1])
            return SimpleNamespace(returncode=0, stdout="")
        with patch.object(perf_002.compose, "command",
                          return_value=SimpleNamespace(stdout="127.0.0.1:55001\n")), \
                patch.object(perf_002.subprocess, "run", side_effect=run), \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaises(RuntimeError):
            with perf_002.DiskPostgres({}):
                raise RuntimeError("the test failed")
        self.assertEqual(removed, ["rm", "volume"])


if __name__ == "__main__":
    unittest.main()
