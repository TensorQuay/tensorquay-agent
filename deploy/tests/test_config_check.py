"""The egress stack drivers' gateway.toml check with `tqa-gateway check`: only exit 0 with its accepted line counts as
accepted; other lines beside it are warnings, logged, not refusals (TK-U-043, candidate 14a)."""

import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock, patch

SOURCE = Path(__file__).resolve().parents[2] / "crates/tqa-sandbox/tests/egress/stack.py"
SPEC = importlib.util.spec_from_file_location("egress_stack", SOURCE)
stack = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stack)

ACCEPTED = stack.CONFIG_ACCEPTED
REFUSED = "tqa-gateway: invalid gateway configuration: surprise: unknown field, expected one of `listen`, `storage`"


class RefusalTests(unittest.TestCase):
    """Fail closed: every outcome but one is a refusal that carries the gateway's output."""

    def test_the_accepted_line_alone_is_accepted(self):
        self.assertEqual(ACCEPTED, "configuration ok: /etc/tqa/gateway.toml")
        self.assertIsNone(stack.config_refusal(0, ACCEPTED))
        self.assertIsNone(stack.config_refusal(0, ACCEPTED + "\n"))

    def test_lines_beside_the_accepted_line_are_warnings(self):
        for output in ("starting\n" + ACCEPTED, ACCEPTED + "\ntqa-gateway: later"):
            with self.subTest(output=output):
                self.assertIsNone(stack.config_refusal(0, output))
                self.assertEqual(len(stack.config_warnings(output)), 1)

    def test_every_other_outcome_is_refused(self):
        cases = [
            (1, ACCEPTED, "exit 1"),
            (2, ACCEPTED, "exit 2"),
            (0, "", "exit 0"),
            (2, REFUSED, "surprise: unknown field"),
            (0, ACCEPTED.upper(), "exit 0"),
            (2, "", "no output"),
            (125, "docker: Error response from daemon", "daemon"),
            (None, "", "timed out"),
            (None, ACCEPTED, "timed out"),
        ]
        for returncode, output, detail in cases:
            with self.subTest(returncode=returncode, output=output):
                refusal = stack.config_refusal(returncode, output)
                self.assertIsNotNone(refusal)
                self.assertIn(detail, refusal)

    def test_long_output_keeps_its_tail(self):
        refusal = stack.config_refusal(1, "x" * 5000 + "the end")
        self.assertTrue(refusal.endswith("the end"))
        self.assertLess(len(refusal), 2200)


class ControlTests(unittest.TestCase):
    """The positive control passes only as a configuration error other than the accepted one."""

    def test_a_refused_copy_proves_the_check(self):
        self.assertIsNone(stack.control_refusal(2, REFUSED + "\n"))
        self.assertIsNone(stack.control_refusal(2, "tqa-gateway: invalid gateway configuration: configuration exceeds 64 KiB"))

    def test_anything_else_means_the_check_proves_nothing(self):
        cases = [
            (0, ACCEPTED, "exit 0"),
            (0, REFUSED, "exit 0"),
            (0, "", "no output"),
            (1, "tqa-gateway: host storage operation failed", "host storage"),
            (2, "starting\n" + REFUSED, "starting"),
            (2, REFUSED + "\n" + REFUSED, "exit 2"),
            (2, "tqa-gateway: invalid gateway configuration: ", "exit 2"),
            (1, REFUSED, "exit 1"),
            (None, REFUSED, "timed out"),
        ]
        for returncode, output, detail in cases:
            with self.subTest(returncode=returncode, output=output):
                refusal = stack.control_refusal(returncode, output)
                self.assertIsNotNone(refusal)
                self.assertIn("proves nothing", refusal)
                self.assertIn(detail, refusal)


class CheckConfigTests(unittest.TestCase):
    """The check runs the service's own image, user and mounts, without the Docker socket, network or environment; the
    positive control runs first, on the broken copy mounted in place of gateway.toml."""

    SERVICE = {"image": "sha256:" + "a" * 64, "user": "1000:1000", "environment": {"TQA_DATABASE_URL": "hidden"},
               "volumes": [
                   {"type": "bind", "source": "/private/gateway.toml", "target": "/etc/tqa/gateway.toml"},
                   {"type": "bind", "source": "/storage", "target": "/storage"},
                   {"type": "bind", "source": "/var/run/docker.sock", "target": "/var/run/docker.sock"},
               ]}

    def driver(self, service=None):
        driver = stack.Stack(Path("/install"), {"PATH": "/usr/bin"})
        driver.configuration = ["docker", "compose"]
        driver.command = Mock(return_value=json.dumps({"services": {"gateway": service or self.SERVICE}}))
        return driver

    @staticmethod
    def outcome(returncode, output):
        return subprocess.CompletedProcess([], returncode, stdout="", stderr=output + "\n")

    @staticmethod
    def mounts(args):
        return [args[index + 1] for index, arg in enumerate(args) if arg == "--mount"]

    def test_the_control_is_refused_then_the_file_is_accepted_in_isolated_runs(self):
        with patch.object(stack.subprocess, "run",
                          side_effect=[self.outcome(2, REFUSED), self.outcome(0, ACCEPTED)]) as run:
            self.driver().check_config()
        self.assertEqual(run.call_count, 2)
        control, real = (call.args[0] for call in run.call_args_list)
        self.assertEqual(self.mounts(control)[0],
                         "type=bind,source=/install/gateway-control.toml,target=/etc/tqa/gateway.toml,readonly")
        self.assertEqual(self.mounts(real), ["type=bind,source=/private/gateway.toml,target=/etc/tqa/gateway.toml,readonly",
                                             "type=bind,source=/storage,target=/storage,readonly"])
        self.assertEqual(self.mounts(control)[1:], self.mounts(real)[1:])
        for call in run.call_args_list:
            args = call.args[0]
            self.assertEqual(call.kwargs["timeout"], stack.CONFIG_CHECK_SECONDS)
            self.assertEqual(args[-3:], [self.SERVICE["image"], "check", "/etc/tqa/gateway.toml"])
            self.assertEqual(args[args.index("--network") + 1], "none")
            self.assertEqual(args[args.index("--user") + 1], "1000:1000")
            self.assertIn("--read-only", args)
            self.assertNotIn("-e", args)
            self.assertNotIn("--env", args)
            self.assertNotIn("hidden", " ".join(args))

    def test_a_check_that_accepts_everything_fails_the_check(self):
        with patch.object(stack.subprocess, "run", side_effect=[self.outcome(0, ACCEPTED), self.outcome(0, ACCEPTED)]):
            with self.assertRaisesRegex(AssertionError, "proves nothing"):
                self.driver().check_config()

    def test_a_refused_file_fails_with_the_gateway_output(self):
        with patch.object(stack.subprocess, "run", side_effect=[self.outcome(2, REFUSED), self.outcome(2, REFUSED)]):
            with self.assertRaisesRegex(AssertionError, "did not accept.*surprise: unknown field"):
                self.driver().check_config()

    def test_an_exit_zero_fails(self):
        with patch.object(stack.subprocess, "run", side_effect=[self.outcome(2, REFUSED), self.outcome(0, "")]):
            with self.assertRaisesRegex(AssertionError, "exit 0"):
                self.driver().check_config()

    def test_a_timeout_fails_and_removes_the_container(self):
        removed, calls = subprocess.CompletedProcess([], 0), Mock()
        # The engine snapshot is stubbed: it would signal this user's real daemons, and its own commands would take
        # the mocked results.
        with patch.object(stack.subprocess, "run",
                          side_effect=[subprocess.TimeoutExpired("docker", 30, output=b"partial"), removed]) as run, \
                patch.object(stack, "engine_snapshot", return_value="snapshot") as snapshot:
            calls.attach_mock(run, "run")
            calls.attach_mock(snapshot, "snapshot")
            with self.assertRaisesRegex(AssertionError, "timed out.*partial"):
                self.driver().check_config()
        started, cleanup = run.call_args_list[0].args[0], run.call_args_list[1].args[0]
        name = started[started.index("--name") + 1]
        self.assertEqual(cleanup, ["docker", "rm", "-f", name])
        # The snapshot comes before the removal touches the engine.
        self.assertEqual([call[0] for call in calls.mock_calls], ["run", "snapshot", "run"])

    def test_a_service_without_its_gateway_toml_mount_fails(self):
        service = dict(self.SERVICE, volumes=self.SERVICE["volumes"][1:])
        with patch.object(stack.subprocess, "run") as run:
            with self.assertRaisesRegex(AssertionError, "no gateway.toml"):
                self.driver(service).check_config()
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
