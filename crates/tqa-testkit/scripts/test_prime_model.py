"""LLM-C-002/019: standalone setup discovers one artifact and fails closed."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import prime_model


SERVER = '''#!/usr/bin/env python3
import os
from pathlib import Path
import signal
import socket
import sys

scenario = Path(sys.argv[-1])
mode = scenario.read_text()
state = Path(__file__).parent / "state"
state.with_suffix(".pid").write_text(str(os.getpid()))
if mode == "exit":
    print("synthetic startup failure", file=sys.stderr)
    sys.exit(7)
if mode == "partial":
    sys.stdout.write("listening ")
    sys.exit(0)
if mode == "oversize":
    print("x" * 1024, flush=True)
    signal.pause()
if mode == "invalid":
    print("listening 127.0.0.1:65536", flush=True)
    signal.pause()
if mode == "unexpected":
    print("unexpected readiness", flush=True)
    signal.pause()
server = socket.socket()
server.bind(("127.0.0.1", 0))
server.listen()
def stop(*_):
    state.with_suffix(".stopped").write_text("terminated")
    sys.exit(9 if mode == "bad_exit" else 0)
signal.signal(signal.SIGTERM, stop)
print("listening 127.0.0.1:" + str(server.getsockname()[1]), flush=True)
signal.pause()
'''


class ArtifactDiscovery(unittest.TestCase):
    def test_native_coverage_release_and_custom_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for folder in ["native/debug", "coverage/debug", "custom/release", "with spaces"]:
                with self.subTest(folder=folder):
                    artifact = root / folder / "tqa-fake-model"
                    artifact.parent.mkdir(parents=True)
                    artifact.write_text("fixture")
                    artifact.chmod(0o755)
                    search = os.pathsep.join([str(root / "missing"), str(artifact.parent)])
                    self.assertEqual(prime_model.discover_binary(search), artifact.resolve())

    def test_duplicate_search_entries_resolve_to_one_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "tqa-fake-model"
            artifact.write_text("fixture")
            artifact.chmod(0o755)
            self.assertEqual(
                prime_model.discover_binary(os.pathsep.join([directory, directory])), artifact.resolve()
            )

    def test_missing_and_nonexecutable_artifacts_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            for search in ["", directory]:
                with self.subTest(search=search), self.assertRaisesRegex(ValueError, "one executable"):
                    prime_model.discover_binary(search)
            (Path(directory) / "tqa-fake-model").write_text("not executable")
            with self.assertRaisesRegex(ValueError, "one executable"):
                prime_model.discover_binary(directory)

    def test_ambiguous_artifacts_are_never_selected_by_order(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / "native", Path(directory) / "coverage"]
            for path in paths:
                path.mkdir()
                artifact = path / "tqa-fake-model"
                artifact.write_text("fixture")
                artifact.chmod(0o755)
            with self.assertRaisesRegex(ValueError, "one executable"):
                prime_model.discover_binary(os.pathsep.join(map(str, paths)))


class Priming(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.binary = self.root / "tqa-fake-model"
        self.binary.write_text(SERVER)
        self.binary.chmod(0o755)
        self.scenario = self.root / "scenario"

    def test_ready_server_is_signalled_and_reaped_before_success(self):
        self.scenario.write_text("ready")
        prime_model.prime(self.binary, self.scenario)
        self.assertEqual((self.root / "state.stopped").read_text(), "terminated")
        self.assert_reaped()

    def test_failure_to_start_reports_the_server_error(self):
        self.scenario.write_text("exit")
        with self.assertRaisesRegex(RuntimeError, "synthetic startup failure"):
            prime_model.prime(self.binary, self.scenario)

    def test_invalid_or_incomplete_ready_line_fails(self):
        for mode in ["partial", "oversize", "invalid", "unexpected"]:
            with self.subTest(mode=mode):
                self.scenario.write_text(mode)
                with self.assertRaisesRegex(RuntimeError, "readiness"):
                    prime_model.prime(self.binary, self.scenario)
                self.assert_reaped()

    def test_nonzero_shutdown_is_not_a_successful_prime(self):
        self.scenario.write_text("bad_exit")
        with self.assertRaisesRegex(RuntimeError, "exit status 9"):
            prime_model.prime(self.binary, self.scenario)
        self.assert_reaped()

    def test_setup_publishes_only_the_successfully_primed_artifact(self):
        environment_file = self.root / "nextest.env"
        for mode in ["ready", "exit", "bad_exit", "unexpected"]:
            with self.subTest(mode=mode):
                self.binary.write_text(SERVER.replace(
                    "mode = scenario.read_text()", "mode = {!r}".format(mode)
                ))
                environment_file.write_text("EXISTING=value\n")
                environment = os.environ.copy()
                environment["NEXTEST_DYLD_FALLBACK_LIBRARY_PATH"] = str(self.root)
                environment["NEXTEST_ENV"] = str(environment_file)
                result = subprocess.run(
                    [sys.executable, "-B", prime_model.__file__], env=environment,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60,
                )
                if mode == "ready":
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(
                        environment_file.read_text(),
                        "EXISTING=value\nTQA_PRIMED_FAKE_MODEL={}\n".format(self.binary.resolve()),
                    )
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(environment_file.read_text(), "EXISTING=value\n")
                    self.assertIn(b"standalone priming failed", result.stderr)

    def assert_reaped(self):
        pid = int((self.root / "state.pid").read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)


if __name__ == "__main__":
    unittest.main()
