"""Offline regressions for the GW-I-017 Compose harness; no Docker daemon is used."""

from contextlib import redirect_stderr
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import MagicMock, Mock, patch

import compose


class CommandTests(unittest.TestCase):
    """Failures retain useful bounded diagnostics without printing fixture credentials."""

    def test_failure_names_subcommand_and_redacts_last_forty_stderr_lines(self):
        password, token = "synthetic-password", "synthetic-bootstrap-token"
        env = {"TQA_POSTGRES_PASSWORD": password, "TQA_BOOTSTRAP_TOKEN": token}
        args = ["docker", "compose", "up", "--token=" + token]
        lines = ["line {}".format(index) for index in range(50)]
        lines[-1] = password + " " + token
        failure = subprocess.CalledProcessError(7, args, output=password, stderr="\n".join(lines))
        output = io.StringIO()
        with patch.object(compose.subprocess, "run", side_effect=failure), redirect_stderr(output):
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                compose.command(args, env, capture=True)
        self.assertIs(caught.exception, failure)
        diagnostic = output.getvalue()
        self.assertIn("docker compose up --token=[REDACTED]", diagnostic)
        self.assertIn("exit 7", diagnostic)
        self.assertNotIn(password, diagnostic)
        self.assertNotIn(token, diagnostic)
        self.assertEqual(diagnostic.splitlines()[1:], lines[10:-1] + ["[REDACTED] [REDACTED]"])

    def test_timeout_reports_subcommand_and_redacts_byte_stderr(self):
        token = "synthetic-timeout-token"
        args = ["docker", "pull", "fixture"]
        failure = subprocess.TimeoutExpired(args, 90, stderr=("waiting " + token).encode())
        output = io.StringIO()
        with patch.object(compose.subprocess, "run", side_effect=failure), redirect_stderr(output):
            with self.assertRaises(subprocess.TimeoutExpired):
                compose.command(args, {"TQA_BOOTSTRAP_TOKEN": token})
        self.assertIn("docker pull fixture", output.getvalue())
        self.assertIn("90", output.getvalue())
        self.assertIn("waiting [REDACTED]", output.getvalue())
        self.assertNotIn(token, output.getvalue())

    def test_missing_executable_reports_the_attempted_command(self):
        output = io.StringIO()
        with patch.object(compose.subprocess, "run", side_effect=FileNotFoundError("missing executable")):
            with redirect_stderr(output), self.assertRaises(FileNotFoundError):
                compose.command(["docker", "version"], {})
        self.assertIn("docker version", output.getvalue())
        self.assertIn("missing executable", output.getvalue())


class PreflightTests(unittest.TestCase):
    """Require the tested server and Compose versions before creating any stack files."""

    def test_older_or_unreadable_versions_fail_with_both_minimums(self):
        for engine, plugin in (("20.10.11", "2.31.0"), ("27.4.0", "2.2.1"),
                               ("27.3.9", "2.31.0"), ("27.4.0", "2.30.9"),
                               ("unknown", "2.31.0"), ("27.4.0", ""),
                               ("27.4.0", "v2.31.0-rc.1")):
            with self.subTest(engine=engine, plugin=plugin):
                outputs = [Mock(stdout=engine), Mock(stdout=plugin)]
                with patch.object(compose, "command", side_effect=outputs):
                    with self.assertRaises(SystemExit) as caught:
                        compose.preflight({})
                self.assertIn("Docker Engine 27.4.0 or newer", str(caught.exception))
                self.assertIn("Docker Compose 2.31.0 or newer", str(caught.exception))
                self.assertIn("minimum tested versions", str(caught.exception))

    def test_exact_minimum_newer_and_desktop_versions_pass(self):
        for engine, plugin in (("27.4.0", "2.31.0"), ("28.0.0", "v2.32.0"),
                               ("27.4.0\n", "v2.31.0-desktop.2\n")):
            with self.subTest(engine=engine, plugin=plugin):
                outputs = [Mock(stdout=engine), Mock(stdout=plugin)]
                with patch.object(compose, "command", side_effect=outputs):
                    compose.preflight({})

    def test_unavailable_engine_reports_requirements(self):
        with patch.object(compose, "command", side_effect=OSError("daemon unavailable")):
            with self.assertRaises(SystemExit) as caught:
                compose.preflight({})
        self.assertIn("Docker Engine 27.4.0 or newer", str(caught.exception))
        self.assertIn("Docker Compose 2.31.0 or newer", str(caught.exception))

    def test_failed_preflight_creates_no_files_and_starts_no_contract(self):
        with patch.dict(compose.os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True), \
                patch.object(compose, "preflight", side_effect=SystemExit("unsupported"), create=True), \
                patch.object(compose.tempfile, "TemporaryDirectory") as temporary, \
                patch.object(compose, "prepare") as prepare, patch.object(compose, "verify"):
            temporary.return_value.__enter__.return_value = "."
            with self.assertRaisesRegex(SystemExit, "unsupported"):
                compose.main()
        temporary.assert_not_called()
        prepare.assert_not_called()


class OwnershipTests(unittest.TestCase):
    """Files handed back after a container-side chown belong to the invoking host user again."""

    @staticmethod
    def daemon(security_options):
        def run(args, **_kwargs):
            stdout = json.dumps(security_options) if args[:2] == ["docker", "info"] else ""
            return subprocess.CompletedProcess(args, 0, stdout=stdout, stderr="")
        return run

    def owner_passed_to_chown(self, security_options):
        with patch.object(compose.subprocess, "run", side_effect=self.daemon(security_options)) as run:
            compose.ownership("/synthetic/install", {})
        chown = run.call_args_list[-1].args[0]
        self.assertEqual(chown[-4:-2], ["chown", "-R"])
        return chown[-2]

    def test_rootless_daemon_hands_back_to_container_root(self):
        options = ["name=seccomp,profile=builtin", "name=rootless", "name=cgroupns"]
        self.assertEqual(self.owner_passed_to_chown(options), "0:0")

    def test_rootful_daemon_hands_back_to_the_host_uid_and_gid(self):
        options = ["name=apparmor", "name=seccomp,profile=builtin", "name=cgroupns"]
        expected = "{}:{}".format(compose.os.getuid(), compose.os.getgid())
        self.assertEqual(self.owner_passed_to_chown(options), expected)

    def test_unreadable_daemon_mode_fails_before_any_chown(self):
        failure = subprocess.CalledProcessError(1, ["docker", "info"], stderr="daemon unavailable")
        with patch.object(compose.subprocess, "run", side_effect=failure) as run, redirect_stderr(io.StringIO()):
            with self.assertRaises(subprocess.CalledProcessError):
                compose.ownership("/synthetic/install", {})
        run.assert_called_once()

    def test_an_explicit_owner_is_used_without_querying_the_daemon(self):
        with patch.object(compose.subprocess, "run", side_effect=self.daemon([])) as run:
            compose.ownership("/synthetic/install", {}, "1000:1000")
        run.assert_called_once()
        self.assertEqual(run.call_args.args[0][-2], "1000:1000")


class ImageTests(unittest.TestCase):
    """The digest-pinned helper image is pulled only when it is not already present."""

    def calls(self, present):
        calls = []

        def run(args, **_options):
            calls.append(args)
            if args[:3] == ["docker", "image", "inspect"] and not present:
                raise subprocess.CalledProcessError(1, args)
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

        with patch.object(compose.subprocess, "run", side_effect=run):
            compose.ensure_image(compose.TOYBOX, {})
        return calls

    def test_a_present_pinned_image_is_not_pulled(self):
        calls = self.calls(present=True)
        self.assertEqual(calls, [["docker", "image", "inspect", "--format", "{{.Id}}", compose.TOYBOX]])

    def test_a_missing_pinned_image_is_pulled_by_its_digest(self):
        calls = self.calls(present=False)
        self.assertEqual(calls[-1], ["docker", "pull", compose.TOYBOX])
        self.assertIn("@sha256:", compose.TOYBOX)


class StandInTests(unittest.TestCase):
    """The harness runtime carries the protocol revision label, so readiness can pass (GW-I-017)."""

    def run_with(self, labels):
        calls = []

        def run(args, **_options):
            calls.append(args)
            if args[:3] == ["docker", "image", "inspect"] and args[4] == "{{.Id}}":
                return subprocess.CompletedProcess(args, 0, stdout="sha256:" + "b" * 64 + "\n", stderr="")
            if args[:3] == ["docker", "image", "inspect"]:
                if labels is None:
                    raise subprocess.CalledProcessError(1, args)
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(labels), stderr="")
            if args[:2] == ["docker", "create"]:
                return subprocess.CompletedProcess(args, 0, stdout="container-id\n", stderr="")
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

        with patch.object(compose.subprocess, "run", side_effect=run):
            reference = compose.stand_in({}, revision=6)
        return reference, calls

    def test_the_revision_is_read_from_its_one_definition(self):
        with tempfile.NamedTemporaryFile("w", suffix=".rs") as source:
            source.write("/// doc\npub const PROTOCOL_REVISION: u16 = 12;\n")
            source.flush()
            self.assertEqual(compose.protocol_revision(source.name), 12)
        self.assertGreaterEqual(compose.protocol_revision(), 1)

    def test_a_missing_stand_in_is_derived_locally_with_the_label_and_the_container_removed(self):
        identity, calls = self.run_with(None)
        self.assertEqual(identity, "sha256:" + "b" * 64, "the gateway takes a runtime only by digest")
        self.assertIn(["docker", "create", "--network", "none", compose.TOYBOX, "true"], calls)
        self.assertIn(["docker", "commit", "--change", "LABEL org.tensorquay.protocol-revision=6", "container-id",
                       "tqa-compose-runtime:rev6"], calls)
        self.assertIn(["docker", "rm", "--force", "container-id"], calls)
        self.assertFalse(any(call[:2] == ["docker", "pull"] for call in calls), "nothing is pulled")

    def test_a_present_stand_in_with_the_right_label_is_reused(self):
        identity, calls = self.run_with({"org.tensorquay.protocol-revision": "6"})
        self.assertEqual(identity, "sha256:" + "b" * 64)
        self.assertFalse(any(call[:2] == ["docker", "commit"] for call in calls), "reused, not rebuilt")

    def test_a_stand_in_with_another_label_is_rebuilt(self):
        _, calls = self.run_with({"org.tensorquay.protocol-revision": "5"})
        self.assertTrue(any(call[:2] == ["docker", "commit"] for call in calls))


class InstallationTests(unittest.TestCase):
    """Only the installation is given to the gateway user; the host CLI keeps its overlays in the owned root."""

    def test_installation_is_a_private_child_of_the_owned_temporary_root(self):
        with tempfile.TemporaryDirectory() as root:
            temporary = Mock()
            temporary.name = root
            directory = compose.installation(temporary)
            self.assertEqual(directory, compose.Path(root).resolve() / "installation")
            self.assertTrue(directory.is_dir())
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)

    def test_stack_overlays_are_written_outside_the_installation(self):
        for path in ("robustness.py", "soak_stack.py", "../../crates/tqa-sandbox/tests/egress/stack.py"):
            source = (compose.Path(__file__).parent / path).read_text()
            self.assertNotRegex(source, r'directory / "[a-z_]+\.json"\)\]', path)
            self.assertRegex(source, r'directory\.parent / "[a-z_]+\.json"', path)


class RetireTests(unittest.TestCase):
    """Retirement stops admission and removes only this installation's sandboxes, which Compose does not own."""

    ENV = {"COMPOSE_PROJECT_NAME": "owned", "TQA_SANDBOX_NETWORK": "owned-sandboxes"}

    def test_sandbox_cleanup_is_scoped_and_continues_after_gateway_stop_failure(self):
        calls = []
        failure = OSError("gateway stop failed")

        def command(args, _env, **_options):
            calls.append(args)
            if "stop" in args:
                raise failure
            return Mock(stdout="a" * 12 + "\n")

        with patch.object(compose, "command", side_effect=command), redirect_stderr(io.StringIO()):
            self.assertIs(compose.retire(dict(self.ENV), None), failure)
        self.assertEqual(calls[0], compose.COMPOSE + ["stop", "--timeout", "10", "gateway"])
        self.assertEqual(calls[1], ["docker", "ps", "-aq", "--filter", "label=tqa.instance=owned-sandboxes"])
        self.assertEqual(calls[2], ["docker", "rm", "--force", "a" * 12])
        self.assertEqual(calls[4], ["docker", "network", "rm", "a" * 12])

    def test_per_sandbox_networks_are_removed_after_the_containers_with_the_same_label(self):
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            if args[:3] == ["docker", "ps", "-aq"]:
                raise OSError("listing failed")
            return Mock(stdout="b" * 12 + "\n" + "c" * 64 + "\n")

        with patch.object(compose, "command", side_effect=command), redirect_stderr(io.StringIO()):
            self.assertIsInstance(compose.retire(dict(self.ENV), None, configuration=["compose"]), OSError)
        self.assertEqual(calls[2], ["docker", "network", "ls", "-q", "--filter", "label=tqa.subnet", "--filter", "label=tqa.instance=owned-sandboxes"])
        self.assertEqual(calls[3], ["docker", "network", "rm", "b" * 12, "c" * 64])
        self.assertEqual(len(calls), 4)

    def test_no_network_is_removed_when_none_is_labelled_for_this_installation(self):
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            return Mock(stdout="")

        with patch.object(compose, "command", side_effect=command):
            self.assertIsNone(compose.retire(dict(self.ENV), None, configuration=["compose"]))
        self.assertEqual([call[:3] for call in calls], [["compose", "stop", "--timeout"], ["docker", "ps", "-aq"],
                                                         ["docker", "network", "ls"]])

    def test_unexpected_identifiers_are_never_removed_and_the_first_failure_stays(self):
        original = AssertionError("contract failed")
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            return Mock(stdout="not-an-id\n")

        with patch.object(compose, "command", side_effect=command), redirect_stderr(io.StringIO()):
            self.assertIs(compose.retire(dict(self.ENV), original, configuration=["compose"]), original)
        self.assertEqual(calls[0], ["compose", "stop", "--timeout", "10", "gateway"])
        self.assertFalse(any("rm" in call for call in calls))
        self.assertIn(["docker", "network", "ls", "-q", "--filter", "label=tqa.subnet", "--filter", "label=tqa.instance=owned-sandboxes"], calls)

    def test_a_stack_that_never_started_is_left_alone(self):
        with patch.object(compose, "command") as command:
            self.assertIsNone(compose.retire({}, None))
        command.assert_not_called()


class SaveLogsTests(unittest.TestCase):
    """On a failure, the stack's own logs are kept before anything is torn down, redacted and bounded (WEB-E2E-027:
    an environment that couldn't start left no gateway log behind)."""

    ENV = {"COMPOSE_PROJECT_NAME": "owned", "TQA_SANDBOX_NETWORK": "owned-sandboxes",
           "TQA_POSTGRES_PASSWORD": "synthetic-password", "TQA_BOOTSTRAP_TOKEN": "synthetic-token"}

    def test_every_service_and_this_installations_sandboxes_are_written_redacted(self):
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            if args[:3] == ["docker", "ps", "-aq"]:
                return Mock(stdout="a" * 12 + "\n" + "b" * 12 + "\n")
            if args[:2] == ["docker", "logs"]:
                # `docker logs` answers with the container's stderr on its own stream.
                return Mock(stdout="started synthetic-password\n", stderr="refused synthetic-token\n")
            return Mock(stdout="started synthetic-password and synthetic-token\n")

        with tempfile.TemporaryDirectory() as folder, patch.object(compose, "command", side_effect=command):
            written = compose.save_logs(dict(self.ENV), Path(folder), configuration=["compose"])
            names = sorted(path.name for path in written)
            self.assertEqual(names, ["compose.log", "sandbox-" + "a" * 12 + ".log", "sandbox-" + "b" * 12 + ".log"])
            for path in written:
                text = path.read_text()
                self.assertIn("started", text)
                self.assertNotIn("synthetic-password", text)
                self.assertNotIn("synthetic-token", text)
            self.assertIn("refused", (Path(folder) / ("sandbox-" + "a" * 12 + ".log")).read_text())
        self.assertEqual(calls[0], ["compose", "logs", "--no-color", "--timestamps", "--tail", str(compose.LOG_LINES)])
        self.assertEqual(calls[1], ["docker", "ps", "-aq", "--filter", "label=tqa.instance=owned-sandboxes"])
        self.assertEqual(calls[2], ["docker", "logs", "--timestamps", "--tail", str(compose.LOG_LINES), "a" * 12])

    def test_a_log_that_cannot_be_read_is_reported_and_the_rest_still_kept(self):
        def command(args, _env, **_options):
            if args[0] == "compose":
                raise OSError("compose logs failed")
            if args[:3] == ["docker", "ps", "-aq"]:
                return Mock(stdout="c" * 12 + "\n")
            return Mock(stdout="sandbox output\n")

        output = io.StringIO()
        with tempfile.TemporaryDirectory() as folder, patch.object(compose, "command", side_effect=command), \
                redirect_stderr(output):
            written = compose.save_logs(dict(self.ENV), Path(folder), configuration=["compose"])
            self.assertEqual([path.name for path in written], ["sandbox-" + "c" * 12 + ".log"])
        self.assertIn("compose logs failed", output.getvalue())

    def test_an_unexpected_sandbox_identifier_is_never_passed_to_docker(self):
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            if args[:3] == ["docker", "ps", "-aq"]:
                return Mock(stdout="--rm\n")
            return Mock(stdout="")

        with tempfile.TemporaryDirectory() as folder, patch.object(compose, "command", side_effect=command), \
                redirect_stderr(io.StringIO()):
            compose.save_logs(dict(self.ENV), Path(folder), configuration=["compose"])
        self.assertFalse(any(args[:2] == ["docker", "logs"] for args in calls))


class TeardownTests(unittest.TestCase):
    """No stack outlives its run: Compose down, else removal by the project's label, and signals end through cleanup."""

    def test_a_failed_compose_down_removes_the_project_by_its_label(self):
        calls = []

        def run(args, **_options):
            calls.append(args)
            if "down" in args:
                raise subprocess.TimeoutExpired(args, 300)
            listed = {"ps": "c0ffee00c0ffee\n", "network": "abc123def456\n", "volume": "tqa-compose-x_postgres\n"}
            kind = next((key for key in listed if key in args[:3]), None)
            stdout = listed[kind] if kind and ("-aq" in args or "ls" in args) else ""
            return subprocess.CompletedProcess(args, 0, stdout=stdout, stderr="")

        env = {"COMPOSE_PROJECT_NAME": "tqa-compose-x"}
        with patch.object(compose.subprocess, "run", side_effect=run), redirect_stderr(io.StringIO()):
            compose.down(["docker", "compose"], env)
        label = "label=com.docker.compose.project=tqa-compose-x"
        self.assertIn(["docker", "ps", "-aq", "--filter", label], calls)
        self.assertIn(["docker", "rm", "--force", "c0ffee00c0ffee"], calls)
        self.assertIn(["docker", "network", "rm", "abc123def456"], calls)
        self.assertIn(["docker", "volume", "rm", "--force", "tqa-compose-x_postgres"], calls)

    def test_a_successful_compose_down_still_removes_what_a_killed_up_created_late(self):
        # Run 41: `compose up` was killed at its timeout while the daemon still created containers; `down` succeeded on
        # the listing it saw, and the containers created after it stayed behind.
        calls = []

        def run(args, **_options):
            calls.append(args)
            stdout = "c0ffee00c0ffee\n" if args[:3] == ["docker", "ps", "-aq"] else ""
            return subprocess.CompletedProcess(args, 0, stdout=stdout, stderr="")

        with patch.object(compose.subprocess, "run", side_effect=run):
            compose.down(["docker", "compose"], {"COMPOSE_PROJECT_NAME": "tqa-compose-x"})
        self.assertIn("down", calls[0])
        self.assertIn(["docker", "rm", "--force", "c0ffee00c0ffee"], calls)

    def test_an_unexpected_identifier_is_never_passed_on(self):
        def run(args, **_options):
            return subprocess.CompletedProcess(args, 0, stdout="--all\n" if "ps" in args else "", stderr="")

        with patch.object(compose.subprocess, "run", side_effect=run):
            with self.assertRaises(ValueError):
                compose.remove_project({"COMPOSE_PROJECT_NAME": "tqa-compose-x"})

    def test_a_sigterm_ends_the_harness_through_its_cleanup(self):
        previous = {number: compose.signal.getsignal(number) for number in (compose.signal.SIGTERM, compose.signal.SIGHUP)}
        try:
            compose.teardown_on_signals()
            with self.assertRaises(KeyboardInterrupt):
                compose.signal.raise_signal(compose.signal.SIGTERM)
        finally:
            for number, handler in previous.items():
                compose.signal.signal(number, handler)


class CleanupTests(unittest.TestCase):
    """Every cleanup is attempted and the first failure remains authoritative."""

    @staticmethod
    def prepared(_directory, env):
        env["COMPOSE_PROJECT_NAME"] = "synthetic-owned-project"

    def test_cleanup_errors_do_not_mask_the_contract_failure(self):
        original = AssertionError("readiness failed")
        with tempfile.TemporaryDirectory() as directory:
            temporary = MagicMock()
            temporary.name = directory
            temporary.__enter__.return_value = directory
            temporary.cleanup.side_effect = OSError("temporary cleanup failed")
            with patch.dict(compose.os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True), \
                    patch.object(compose, "preflight", create=True), \
                    patch.object(compose.tempfile, "TemporaryDirectory", return_value=temporary), \
                    patch.object(compose, "prepare", side_effect=self.prepared), \
                    patch.object(compose, "verify", side_effect=original), \
                    patch.object(compose, "command", side_effect=subprocess.CalledProcessError(1, ["down"])) as down, \
                    patch.object(compose, "ownership", side_effect=OSError("ownership failed")) as ownership, \
                    patch.object(compose, "remove_project") as removed, \
                    redirect_stderr(io.StringIO()):
                with self.assertRaises(AssertionError) as caught:
                    compose.main()
            self.assertIs(caught.exception, original)
            down.assert_called_once()
            removed.assert_called_once()
            ownership.assert_called_once()
            temporary.cleanup.assert_called_once()

    def test_cleanup_failure_after_success_still_fails_and_attempts_ownership(self):
        original = subprocess.CalledProcessError(9, ["down"])
        with tempfile.TemporaryDirectory() as directory:
            temporary = MagicMock()
            temporary.name = directory
            temporary.__enter__.return_value = directory
            with patch.dict(compose.os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True), \
                    patch.object(compose, "preflight", create=True), \
                    patch.object(compose.tempfile, "TemporaryDirectory", return_value=temporary), \
                    patch.object(compose, "prepare", side_effect=self.prepared), \
                    patch.object(compose, "verify"), \
                    patch.object(compose, "command", side_effect=original), \
                    patch.object(compose, "ownership") as ownership, redirect_stderr(io.StringIO()):
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    compose.main()
            self.assertIs(caught.exception, original)
            ownership.assert_called_once()
            temporary.cleanup.assert_called_once()

    def test_success_removes_temporary_files_after_stack_cleanup(self):
        directories = []

        def prepare(directory, env):
            self.prepared(directory, env)
            directories.append(directory)

        with patch.dict(compose.os.environ, {"TQA_DOCKER_TESTS": "1"}, clear=True), \
                patch.object(compose, "preflight", create=True), \
                patch.object(compose, "prepare", side_effect=prepare), \
                patch.object(compose, "verify"), patch.object(compose, "command") as down, \
                patch.object(compose, "remove_project") as removed, \
                patch.object(compose, "ownership") as ownership:
            compose.main()
        down.assert_called_once()
        removed.assert_called_once()
        ownership.assert_called_once()
        self.assertEqual(len(directories), 1)
        self.assertFalse(directories[0].exists())


class ModelRouteTest(unittest.TestCase):
    def test_the_default_route_is_the_unchanged_chat_route(self):
        self.assertEqual(
            compose.models_toml("openai_chat"),
            "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://fake-model:8080/v1'\n"
            "model='fixture'\nauth={kind='bearer'}\nkey_ref='secrets/local'\n",
        )

    def test_each_protocol_names_itself_and_messages_declares_an_output_limit(self):
        for protocol in compose.PROTOCOLS:
            route = compose.models_toml(protocol)
            self.assertIn("protocol='" + protocol + "'\n", route)
            self.assertEqual("max_output=4096\n" in route, protocol == "anthropic_messages", protocol)

    def test_an_unknown_protocol_is_refused_before_any_file_is_written(self):
        with self.assertRaises(ValueError):
            compose.models_toml("future_protocol")
        with tempfile.TemporaryDirectory() as root:
            directory = compose.Path(root)
            with self.assertRaises(ValueError):
                compose.prepare(directory, {"TQA_DEMO_PROTOCOL": "future_protocol"})
            self.assertEqual(list(directory.iterdir()), [])


def net(*parts, prefix):
    """A network in CIDR form, built from its parts at run time."""
    return "{}/{}".format(".".join(str(part) for part in parts), prefix)


class SandboxPoolTests(unittest.TestCase):
    """The installation's sandbox pool: a 10.213.X.0/24 no existing network overlaps (the review stack's pool among
    them); a clash would make the engine refuse the installation's sandbox networks."""

    def test_a_pool_in_use_is_never_chosen(self):
        used = [net(10, 213, 41, 0, prefix=29), net(10, 213, 41, 8, prefix=29), net(172, 18, 0, 0, prefix=16)]
        for draw in range(256):
            with self.subTest(draw=draw):
                chosen = compose.sandbox_pool(used, randbelow=lambda n, d=draw: d % n)
                self.assertNotEqual(chosen, net(10, 213, 41, 0, prefix=24))
                self.assertTrue(chosen.startswith("10.213.") and chosen.endswith(".0/24"))

    def test_a_wider_network_covering_the_range_counts_as_used(self):
        used = [net(10, 213, 0, 0, prefix=17)]  # the lower half of the range: third octets 0 to 127
        chosen = compose.sandbox_pool(used, randbelow=lambda n: 0)
        self.assertGreaterEqual(int(chosen.split(".")[2]), 128)

    def test_no_free_pool_fails_clearly(self):
        with self.assertRaisesRegex(RuntimeError, "no free 10.213.X.0/24"):
            compose.sandbox_pool([net(10, 213, 0, 0, prefix=16)], randbelow=lambda n: 0)

    def test_a_base_off_the_default_range_draws_there_and_only_there(self):
        """A host with a soak running draws its sandboxes anywhere in 10.213.0.0/16, so another stack beside it takes
        a base of its own (TQA_SANDBOX_POOL_BASE, for example 10.214) and never a 10.213 pool."""
        used = [net(10, 213, 0, 0, prefix=16), net(10, 214, 7, 0, prefix=24)]
        for draw in range(256):
            with self.subTest(draw=draw):
                chosen = compose.sandbox_pool(used, randbelow=lambda n, d=draw: d % n, base="10.214")
                self.assertTrue(chosen.startswith("10.214.") and chosen.endswith(".0/24"), chosen)
                self.assertNotEqual(chosen, net(10, 214, 7, 0, prefix=24))
        with self.assertRaisesRegex(RuntimeError, "no free 10.214.X.0/24"):
            compose.sandbox_pool([net(10, 214, 0, 0, prefix=16)], randbelow=lambda n: 0, base="10.214")
        with self.assertRaisesRegex(ValueError, "base"):
            compose.sandbox_pool([], randbelow=lambda n: 0, base="10.213.5")

    def test_the_pool_base_comes_from_the_environment(self):
        self.assertEqual(compose.pool_base({"TQA_SANDBOX_POOL_BASE": "10.214"}), "10.214")
        self.assertEqual(compose.pool_base({}), "10.213")
        self.assertEqual(compose.pool_base({"TQA_SANDBOX_POOL_BASE": ""}), "10.213")

    def test_the_engines_networks_are_read_from_their_ipam_config(self):
        subnets = [net(10, 213, 41, 0, prefix=29), "fd00::/64", net(172, 18, 0, 0, prefix=16)]
        inspected = json.dumps([{"IPAM": {"Config": [{"Subnet": subnets[0]}]}},
                                {"IPAM": {"Config": [{"Subnet": subnets[1]}, {"Subnet": subnets[2]}]}},
                                {"IPAM": {"Config": None}}])
        self.assertEqual(compose.used_subnets(inspected), subnets)


class EngineSubnetsTests(unittest.TestCase):
    """A parallel stack job may delete networks between `docker network ls` and `inspect` (candidate12's integration:
    rt_i_015 aborted on "network fbbadc7c7db2 not found"). Docker still prints the networks that exist and exits 1; a
    vanished network's subnet is free again, so only a "not found" is tolerated. Any other failure still stops."""

    def scan(self, returncode, networks, stderr):
        listed = subprocess.CompletedProcess([], 0, stdout="a1\nb2\nc3\nd4\n", stderr="")
        inspected = subprocess.CompletedProcess([], returncode, stdout=json.dumps(networks), stderr=stderr)
        with patch.object(compose, "command", return_value=listed), \
                patch.object(compose.subprocess, "run", return_value=inspected) as run, \
                redirect_stderr(io.StringIO()):
            subnets = compose.engine_subnets({})
        self.assertEqual(run.call_args.args[0], ["docker", "network", "inspect", "a1", "b2", "c3", "d4"])
        return subnets

    def test_networks_that_vanish_between_list_and_inspect_are_skipped(self):
        kept = net(10, 213, 7, 0, prefix=29)
        stderr = "".join("Error response from daemon: network {} not found\n".format(name) for name in ("b2", "c3", "d4"))
        self.assertEqual(self.scan(1, [{"IPAM": {"Config": [{"Subnet": kept}]}}], stderr), [kept])
        self.assertEqual(self.scan(1, [], stderr), [], "every one gone: nothing is in use")

    def test_any_other_inspect_failure_still_stops(self):
        for stderr in ["permission denied while trying to connect to the Docker daemon socket\n",
                       "Error response from daemon: network b2 not found\nError response from daemon: context canceled\n"]:
            with self.subTest(stderr=stderr), self.assertRaises(subprocess.CalledProcessError):
                self.scan(1, [], stderr)

    def test_a_clean_inspect_is_read_as_before(self):
        subnet = net(172, 18, 0, 0, prefix=16)
        self.assertEqual(self.scan(0, [{"IPAM": {"Config": [{"Subnet": subnet}]}}], ""), [subnet])

if __name__ == "__main__":
    unittest.main()


RUNTIME = "sha256:" + "1" * 64
WORKSTATION = "sha256:" + "2" * 64
FOREIGN = "sha256:" + "9" * 64


class BuiltinPinTests(unittest.TestCase):
    """Every stack pins its built-in environments to the images under test (GW-U-120's key), never to whatever the
    shared engine's tqa-runtime:<name> tag names."""

    def test_minimal_is_the_runtime_and_workstation_the_named_image_resolved_once(self):
        with patch.object(compose, "command", return_value=Mock(stdout=WORKSTATION + "\n")) as command:
            pins = compose.builtin_pins({"TQA_WORKSTATION_IMAGE": "my-workstation:dev"}, RUNTIME, stand=False)
        self.assertEqual(pins, {"minimal": RUNTIME, "workstation": WORKSTATION})
        self.assertEqual(command.call_args[0][0], ["docker", "image", "inspect", "--format", "{{.Id}}",
                                                    "my-workstation:dev"])

    def test_a_run_that_does_not_name_its_workstation_is_refused_and_the_bare_tag_never_read(self):
        with patch.object(compose, "command") as command:
            with self.assertRaisesRegex(ValueError, "TQA_WORKSTATION_IMAGE must name the Workstation image under test"):
                compose.builtin_pins({}, RUNTIME, stand=False)
        command.assert_not_called()

    def test_the_stand_in_stack_pins_workstation_to_the_stand_in_so_none_registers(self):
        with patch.object(compose, "command") as command:
            self.assertEqual(compose.builtin_pins({}, RUNTIME, stand=True),
                             {"minimal": RUNTIME, "workstation": RUNTIME})
        command.assert_not_called()

    def test_a_named_image_that_is_not_on_the_engine_is_refused(self):
        with patch.object(compose, "command", return_value=Mock(stdout="\n")):
            with self.assertRaisesRegex(ValueError, "my-workstation:dev doesn't resolve to a local image ID"):
                compose.builtin_pins({"TQA_WORKSTATION_IMAGE": "my-workstation:dev"}, RUNTIME, stand=False)

    def test_the_section_names_each_pin_and_never_data(self):
        section = compose.pins_toml({"minimal": RUNTIME, "workstation": WORKSTATION})
        self.assertEqual(section, '\n[runtime.builtin_images]\nminimal = "{}"\nworkstation = "{}"\n'.format(
            RUNTIME, WORKSTATION))
        self.assertNotIn("data", section)


class SandboxImageGuardTests(unittest.TestCase):
    """Before the stack is removed, every sandbox it ever created must have run an image under test."""

    ENV = {"COMPOSE_PROJECT_NAME": "owned", "TQA_SANDBOX_NETWORK": "owned-sandboxes",
           "TQA_PINNED_IMAGES": RUNTIME + "," + WORKSTATION, "TQA_STACK_SINCE": "1790000000"}

    def test_sandboxes_on_the_pinned_images_or_ones_the_suite_built_pass(self):
        events = "a{} {}\nb{} {}\nc{} {}\n".format("1" * 11, RUNTIME, "2" * 11, WORKSTATION, "3" * 11, FOREIGN)
        self.assertEqual(compose.foreign_sandboxes(events, {RUNTIME, WORKSTATION}, lambda image: image),
                         [("c" + "3" * 11, FOREIGN)])
        self.assertEqual(compose.foreign_sandboxes(events, {RUNTIME, WORKSTATION, FOREIGN}, lambda image: image), [])

    def test_an_image_recorded_by_tag_is_compared_by_its_id(self):
        events = "a{} tqa-runtime:workstation\n".format("1" * 11)
        self.assertEqual(compose.foreign_sandboxes(events, {WORKSTATION}, lambda image: WORKSTATION), [])
        self.assertEqual(compose.foreign_sandboxes(events, {RUNTIME}, lambda image: FOREIGN),
                         [("a" + "1" * 11, FOREIGN)])

    def run_retire(self, env, events, failure=None):
        calls = []

        def command(args, _env, **_options):
            calls.append(args)
            if args[:2] == ["docker", "events"]:
                return Mock(stdout=events)
            return Mock(stdout="")

        stderr = io.StringIO()
        with patch.object(compose, "command", side_effect=command), redirect_stderr(stderr):
            result = compose.retire(env, failure, configuration=["compose"])
        return result, calls, stderr.getvalue()

    def test_retire_fails_a_run_whose_sandbox_ran_another_build_and_names_it(self):
        result, calls, _ = self.run_retire(dict(self.ENV), "d{} {}\n".format("4" * 11, FOREIGN))
        self.assertIsInstance(result, AssertionError)
        self.assertIn("sandbox d{} ran {}, not an image under test".format("4" * 11, FOREIGN), str(result))
        events = next(call for call in calls if call[:2] == ["docker", "events"])
        self.assertEqual(events[events.index("--since") + 1], "1790000000")
        self.assertIn("label=tqa.instance=owned-sandboxes", events)
        self.assertIn("event=create", events)

    def test_a_suite_declares_the_images_it_built(self):
        env = dict(self.ENV, TQA_BUILT_IMAGES=FOREIGN)
        result, _, _ = self.run_retire(env, "d{} {}\n".format("4" * 11, FOREIGN))
        self.assertIsNone(result)

    def test_the_runs_own_failure_stays_first_and_the_guard_still_reports(self):
        original = AssertionError("contract failed")
        result, _, stderr = self.run_retire(dict(self.ENV), "d{} {}\n".format("4" * 11, FOREIGN), original)
        self.assertIs(result, original)
        self.assertIn("not an image under test", stderr)

    def test_a_stack_without_pins_is_not_checked(self):
        env = {key: value for key, value in self.ENV.items() if key != "TQA_PINNED_IMAGES"}
        result, calls, _ = self.run_retire(env, "d{} {}\n".format("4" * 11, FOREIGN))
        self.assertIsNone(result)
        self.assertFalse(any(call[:2] == ["docker", "events"] for call in calls))


class GatewaySupportTests(unittest.TestCase):
    """The key is written only for a gateway that accepts it (its image says so); an older gateway refuses unknown
    keys, so its stack is refused unless the run explicitly takes the engine's tags (an A/B baseline)."""

    def labels(self, value):
        return patch.object(compose, "command", return_value=Mock(stdout=json.dumps(value) + "\n"))

    def test_a_labelled_gateway_takes_the_key(self):
        with self.labels({"org.tensorquay.gateway.builtin-images": "1"}) as command:
            self.assertTrue(compose.gateway_takes_pins({"TQA_GATEWAY_IMAGE": "gw:new"}))
        self.assertEqual(command.call_args[0][0], ["docker", "image", "inspect", "--format", "{{json .Config.Labels}}",
                                                    "gw:new"])
        for labels in (None, {}, {"org.tensorquay.gateway.builtin-images": "0"}):
            with self.labels(labels):
                self.assertFalse(compose.gateway_takes_pins({"TQA_GATEWAY_IMAGE": "gw:old"}), labels)

    def test_an_older_gateway_is_refused_without_the_explicit_override(self):
        refusal = "gw:old predates \\[runtime.builtin_images\\].*TQA_UNPINNED_BUILTINS=1"
        with self.assertRaisesRegex(ValueError, refusal):
            compose.pin_section({"TQA_GATEWAY_IMAGE": "gw:old"}, {"minimal": RUNTIME}, takes_pins=False)

    def test_the_override_writes_no_key_says_so_and_keeps_the_guard(self):
        env = {"TQA_GATEWAY_IMAGE": "gw:old", "TQA_UNPINNED_BUILTINS": "1"}
        with patch("sys.stdout", new_callable=io.StringIO) as out:
            self.assertEqual(compose.pin_section(env, {"minimal": RUNTIME}, takes_pins=False), "")
        self.assertIn("Built-ins not pinned: the gateway image gw:old predates [runtime.builtin_images]",
                      out.getvalue())
        self.assertIn("the sandbox image guard still", out.getvalue())

    def test_a_gateway_that_takes_the_key_gets_it(self):
        self.assertIn("[runtime.builtin_images]", compose.pin_section({}, {"minimal": RUNTIME}, takes_pins=True))


class RequiredImageTests(unittest.TestCase):
    """A stack runs the images it names: the engine is shared, so a bare tag (tqa-gateway:local, tqa-runtime:base)
    may be another member's or a candidate's build, and is never a default."""

    def test_a_named_image_is_used_as_named(self):
        self.assertEqual(compose.required_image({"TQA_GATEWAY_IMAGE": "sha256:" + "a" * 64}, "TQA_GATEWAY_IMAGE"),
                         "sha256:" + "a" * 64)

    def test_a_missing_or_empty_one_is_refused_in_plain_words(self):
        for env in ({}, {"TQA_RUNTIME_IMAGE": ""}):
            with self.subTest(env=env), self.assertRaisesRegex(
                    ValueError, "TQA_RUNTIME_IMAGE must name the image under test: the engine is shared, so a bare tag "
                                "may be another build's"):
                compose.required_image(env, "TQA_RUNTIME_IMAGE")

    def test_prepare_names_the_gateway_and_web_images_before_writing_anything(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(compose, "command") as command:
            with self.assertRaisesRegex(ValueError, "TQA_GATEWAY_IMAGE must name"):
                compose.prepare(Path(temporary), {"TQA_WEB_IMAGE": "w"}, runtime="sha256:" + "1" * 64)
            self.assertEqual(list(Path(temporary).iterdir()), [], "nothing written")
        command.assert_not_called()
