"""SOAK-001…003: paginated storage, best-effort owned cleanup that fails closed, hourly log windows and rotation."""
import types
import unittest
from unittest import mock

import soak
import soak_stack
from soak_audit import AuditError
from soak_stack import pages, cleanup_actions


class WindowLogTests(unittest.TestCase):
    def stack(self, output):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.env = {}
        calls = []

        def command(arguments, env, capture=False, timeout=90):
            calls.append(arguments)
            return types.SimpleNamespace(stdout=output, stderr="")
        return stack, calls, mock.patch.object(soak_stack.compose, "command", command)

    def test_a_window_asks_the_engine_for_exactly_its_hour(self):
        stack, calls, patched = self.stack("line\n")
        with patched:
            self.assertEqual(stack.window_logs("abc", 1000.4, 4600.4), "line\n")
        self.assertEqual(calls, [["docker", "logs", "--since", "1000", "--until", "4600", "abc"]])

    def test_a_verbose_hour_is_cut_with_a_marker_instead_of_failing_the_run(self):
        stack, _, patched = self.stack("x" * (soak_stack.LOG_CAP + 10))
        with patched:
            text = stack.window_logs("abc", 0, 1)
        self.assertTrue(text.endswith("[cut at 8 MiB]\n"))
        self.assertEqual(len(text), soak_stack.LOG_CAP + len("\n[cut at 8 MiB]\n"))


class FaultCommandTests(unittest.TestCase):
    """The harness's own fault commands (kill, pause, network) are bounded by ENGINE_COMMAND_SECONDS: on a loaded engine
    a kill took over 15 s (the candidate 6 rehearsal's db_crash) and failed the soak for the harness, not the product."""

    def stack(self):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.env, stack.runtimes, stack.paused = {}, {}, {}
        stack.gateway, stack.database = "gateway-id", "database-id"
        calls = []

        def command(arguments, env, capture=False, timeout=90):
            calls.append((arguments[1], timeout))
            return types.SimpleNamespace(stdout="", stderr="")
        return stack, calls, mock.patch.object(soak_stack.compose, "command", command)

    def test_every_kill_waits_the_engine_bound(self):
        self.assertGreaterEqual(soak_stack.ENGINE_COMMAND_SECONDS, 60)
        stack, calls, patched = self.stack()
        with patched, mock.patch.object(soak_stack.Stack, "database_ready", lambda self: None), \
                mock.patch.object(soak_stack.Stack, "wait_ready", lambda self: None), \
                mock.patch.object(soak_stack.Stack, "service", lambda self, name: "gateway-id"), \
                mock.patch.object(soak_stack.Stack, "sandbox", lambda self, conversation: {"Id": "sandbox-id"}):
            stack.database_fault("db_crash")
            stack.kill_gateway()
            stack.kill_sandbox("c")
        kills = [timeout for verb, timeout in calls if verb == "kill"]
        self.assertEqual(kills, [soak_stack.ENGINE_COMMAND_SECONDS] * 3)


class ServiceTests(unittest.TestCase):
    """Right after a gateway restart, `compose ps -q` briefly listed no container (the leak-proof rehearsal's first
    gateway_restart failed as "invalid container identity" with the gateway back and ready). The harness asks again,
    for a bounded time, before it fails."""

    GATEWAY_ID = "a" * 64

    def stack(self, answers, clock):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.env, stack.configuration = {"COMPOSE_PROJECT_NAME": "soak"}, ["docker", "compose"]
        asked = []

        def command(arguments, timeout=90):
            asked.append(arguments)
            if arguments[:2] == ["docker", "inspect"]:
                return '[{"Id": "%s", "Config": {"Labels": {"com.docker.compose.project": "soak"}}}]' % self.GATEWAY_ID
            return next(answers)
        stack.command = command

        def sleep(seconds):
            clock[0] += seconds
        return stack, asked, mock.patch.object(soak_stack.time, "monotonic", lambda: clock[0]), \
            mock.patch.object(soak_stack.time, "sleep", sleep)

    def test_a_service_that_briefly_lists_no_container_is_asked_again(self):
        clock = [0.0]
        stack, asked, monotonic, sleep = self.stack(iter(["", "", self.GATEWAY_ID[:12]]), clock)
        with monotonic, sleep:
            self.assertEqual(stack.service("gateway"), self.GATEWAY_ID)
        self.assertEqual(sum(1 for arguments in asked if arguments[-3:] == ["ps", "-q", "gateway"]), 3)

    def test_a_service_with_no_container_past_the_bound_fails_in_plain_words(self):
        clock = [0.0]
        stack, asked, monotonic, sleep = self.stack(iter([""] * 1000), clock)
        with monotonic, sleep, self.assertRaisesRegex(
                AuditError, "the gateway service has no running container after 30 s"):
            stack.service("gateway")
        self.assertLessEqual(clock[0], soak_stack.SERVICE_SECONDS + 1)


class EngineCommandTests(unittest.TestCase):
    """The harness's own engine commands (kill, stop, start, pause, inspect, network) on a saturated engine: R2 failed
    at 97 minutes when `docker kill` of a sandbox didn't return within 60 s while the disk was saturated. They get a
    180-second bound and one retry, and each slow one is a measurement (the command, its seconds, the I/O pressure
    then), never a failure: only the product's behaviour fails the soak."""

    def stack(self, script):
        """`script`: per call, (seconds it takes, outcome): "ok", "timeout" or an error's stderr."""
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.env, stack.slow_commands, stack.on_slow = {}, [], None
        clock, calls, steps = [0.0], [], iter(script)

        def command(arguments, timeout=90):
            seconds, outcome = next(steps)
            calls.append((arguments, timeout))
            clock[0] += min(seconds, timeout)
            if outcome == "timeout" or seconds > timeout:
                raise soak_stack.subprocess.TimeoutExpired(arguments, timeout)
            if outcome != "ok":
                raise soak_stack.subprocess.CalledProcessError(1, arguments, stderr=outcome)
            return ""
        stack.command = command
        patches = (mock.patch.object(soak_stack.time, "monotonic", lambda: clock[0]),
                   mock.patch.object(soak_stack, "io_full", lambda: 71.5))
        return stack, calls, patches

    def test_the_bound_is_180_seconds(self):
        self.assertEqual(soak_stack.ENGINE_COMMAND_SECONDS, 180)

    def test_a_command_that_times_out_once_is_retried_and_recorded_not_failed(self):
        stack, calls, (clock, io) = self.stack([(180, "timeout"), (40, "ok")])
        with clock, io:
            stack.engine(["docker", "kill", "sandbox-id"])
        self.assertEqual([timeout for _, timeout in calls], [180, 180])
        self.assertEqual(stack.slow_commands, [{"command": "kill", "seconds": 220.0, "attempts": 2, "io_full": 71.5}])

    def test_a_retry_finding_the_first_attempt_took_effect_counts_as_done(self):
        for verb, already in [("kill", "Error response from daemon: container x is not running"),
                              ("pause", "Error response from daemon: container x is already paused"),
                              ("unpause", "Error response from daemon: container x is not paused"),
                              ("network", "Error response from daemon: container x is not connected to network n"),
                              ("network", "Error response from daemon: endpoint with name x already exists in network n")]:
            stack, calls, (clock, io) = self.stack([(180, "timeout"), (1, already)])
            arguments = ["docker", verb, "x"] if verb != "network" else ["docker", "network", "disconnect", "n", "x"]
            with self.subTest(verb=verb, already=already), clock, io:
                stack.engine(arguments)
                self.assertEqual(len(calls), 2)

    def test_an_error_that_isnt_already_done_still_fails(self):
        stack, _, (clock, io) = self.stack([(1, "Error response from daemon: No such container: x")])
        with clock, io, self.assertRaises(soak_stack.subprocess.CalledProcessError):
            stack.engine(["docker", "kill", "x"])
        self.assertEqual(stack.slow_commands, [], "a quick failure is no slow measurement")

    def test_a_command_that_times_out_twice_fails_in_plain_words(self):
        stack, _, (clock, io) = self.stack([(180, "timeout"), (180, "timeout")])
        with clock, io, self.assertRaisesRegex(AuditError, "the engine didn't finish `docker kill` in two tries of "
                                                           "180 s each \\(io full 71.5 %\\)"):
            stack.engine(["docker", "kill", "x"])
        self.assertEqual(stack.slow_commands[-1]["attempts"], 2)

    def test_a_slow_command_that_succeeds_is_recorded_and_reported(self):
        stack, _, (clock, io) = self.stack([(30, "ok"), (2, "ok")])
        seen = []
        stack.on_slow = seen.append
        with clock, io:
            stack.engine(["docker", "stop", "--time", "10", "db"])
            stack.engine(["docker", "inspect", "db"])
        self.assertEqual(stack.slow_commands, [{"command": "stop", "seconds": 30.0, "attempts": 1, "io_full": 71.5}])
        self.assertEqual(seen, stack.slow_commands, "each slow command is logged as it happens")


class DatabaseWindowTests(unittest.TestCase):
    def test_nothing_is_sent_while_a_database_fault_window_is_open(self):
        driver = soak.Soak.__new__(soak.Soak)
        driver.database_windows = [[0.0, float("inf")]]
        driver.audits = None  # replenish must return before touching any conversation
        self.assertTrue(driver.database_down())
        driver.replenish(draining=False)
        driver.database_windows = [[0.0, 1.0]]
        self.assertFalse(driver.database_down())


class ReadinessTests(unittest.TestCase):
    def test_a_conversation_whose_first_run_already_finished_is_ready(self):
        ready = types.SimpleNamespace(is_set=lambda: True)
        audit = soak.RunAudit()
        self.assertFalse(soak.started_and_connected(audit, [types.SimpleNamespace(ready=ready)] * 2))
        audit.started, audit.active = 1, None  # the first run has already completed
        self.assertTrue(soak.started_and_connected(audit, [types.SimpleNamespace(ready=ready)] * 2))
        waiting = types.SimpleNamespace(is_set=lambda: False)
        self.assertFalse(soak.started_and_connected(audit, [types.SimpleNamespace(ready=ready),
                                                           types.SimpleNamespace(ready=waiting)]))


class AdmissionTests(unittest.TestCase):
    def test_the_installation_admits_every_conversation_plus_room_for_replacements(self):
        self.assertEqual(soak_stack.max_live(8), 16)
        self.assertEqual(soak_stack.max_live(4), 8)
        with self.assertRaises(AuditError):
            soak_stack.max_live(9)  # the /24 pool of /29 networks holds 16 at most


class ImageReferenceTests(unittest.TestCase):
    def test_each_image_follows_its_variable_the_fake_model_too(self):
        # The candidate's tag file names every image by ID, the test model included; none may fall back to a tag.
        ids = {name: "sha256:" + letter * 64 for name, letter in
               (("TQA_GATEWAY_IMAGE", "a"), ("TQA_RUNTIME_IMAGE", "b"), ("TQA_WEB_IMAGE", "c"),
                ("TQA_FAKE_MODEL_IMAGE", "d"))}
        self.assertEqual(soak_stack.image_references(ids),
                         {"gateway": ids["TQA_GATEWAY_IMAGE"], "runtime": ids["TQA_RUNTIME_IMAGE"],
                          "web": ids["TQA_WEB_IMAGE"], "fake-model": ids["TQA_FAKE_MODEL_IMAGE"]})

    def test_a_run_that_does_not_name_every_image_is_refused_never_given_a_bare_tag(self):
        named = {name: "sha256:" + "a" * 64 for name in ("TQA_GATEWAY_IMAGE", "TQA_RUNTIME_IMAGE", "TQA_WEB_IMAGE",
                                                          "TQA_FAKE_MODEL_IMAGE")}
        for missing in named:
            env = {name: value for name, value in named.items() if name != missing}
            with self.subTest(missing=missing), self.assertRaisesRegex(ValueError, missing + " must name"):
                soak_stack.image_references(env)


class EngineProxyWiringTests(unittest.TestCase):
    def test_the_gateway_is_pointed_at_a_private_stall_proxy(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as root:
            stack = soak_stack.Stack.__new__(soak_stack.Stack)
            stack.directory = Path(root) / "installation"
            stack.env = {"TQA_DOCKER_SOCKET": str(Path(root) / "engine.sock")}
            stack.start_engine_proxy()
            try:
                proxy = Path(stack.env["TQA_DOCKER_SOCKET"])
                self.assertEqual(proxy, Path(root) / "engine" / "proxy.sock")
                self.assertEqual(proxy.parent.stat().st_mode & 0o777, 0o700)
                self.assertEqual(stack.proxy.upstream, Path(root) / "engine.sock")
            finally:
                stack.stop_engine_proxy()

    def test_without_an_engine_socket_the_proxy_refuses_to_start(self):
        stack = soak_stack.Stack.__new__(soak_stack.Stack)
        stack.env = {}
        with self.assertRaises(AuditError):
            stack.start_engine_proxy()


class OrphanSettleTests(unittest.TestCase):
    def driver(self, inventories):
        driver = soak.Soak.__new__(soak.Soak)
        driver.stack = types.SimpleNamespace(inventory=lambda: inventories.pop(0) if len(inventories) > 1
                                             else inventories[0])
        driver.journal = None
        return driver

    def test_a_leftover_that_clears_within_the_bound_passes(self):
        leftover = ([{"id": "1", "conversation": "a", "state": "exited", "networks": []}], [], [], [])
        driver = self.driver([leftover, ([], [], [], [])])
        with mock.patch.object(soak.time, "sleep"), mock.patch("builtins.print"):
            self.assertEqual(driver.orphans({"a"})["problems"], [])

    def test_a_leftover_that_stays_fails_with_its_name(self):
        leftover = ([{"id": "1", "conversation": "a", "state": "exited", "networks": []}], [], [], [])
        driver = self.driver([leftover])
        clock = iter(range(0, 10000, 60))
        with mock.patch.object(soak.time, "sleep"), mock.patch.object(soak.time, "monotonic", lambda: next(clock)):
            with self.assertRaisesRegex(AuditError, "container 1 .conversation a. is exited"):
                driver.orphans({"a"})


class MicrovmTests(unittest.TestCase):
    def test_a_microvm_soak_uses_the_firecracker_overlay_and_network(self):
        from pathlib import Path
        env = {}
        stack = soak_stack.Stack(Path("/tmp/soak-x/installation"), env, Path("/tmp/soak-x/out"), backend="firecracker")
        self.assertIn("deploy/compose/firecracker.yml", stack.configuration)
        self.assertEqual(stack.configuration[-1], "/tmp/soak-x/soak.json")
        # Placeholders for a teardown before the draw; to_microvm draws the stack's own network.
        undrawn = soak_stack.firecracker_stack().UNDRAWN
        self.assertEqual((env["TQA_FC_SUBNET"], env["TQA_FC_GATEWAY_ADDRESS"]),
                         (undrawn["TQA_FC_SUBNET"], undrawn["TQA_FC_GATEWAY_ADDRESS"]))
        docker = soak_stack.Stack(Path("/tmp/soak-x/installation"), {}, Path("/tmp/soak-x/out"))
        self.assertNotIn("deploy/compose/firecracker.yml", docker.configuration)


class RealModelConfigTests(unittest.TestCase):
    def stack(self, env):
        import tempfile
        from pathlib import Path
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        installation = Path(directory.name) / "installation"
        installation.mkdir()
        (installation / "gateway.toml").write_text("[sandbox]\n")
        (installation / "models.toml").write_text("[[routes]]\nname='local'\n")
        stack = soak_stack.Stack(installation, env, Path(directory.name) / "out", model="real")
        stack.images = {"fake-model": "sha256:" + "e" * 64}
        return stack, installation

    def test_the_local_route_is_pointed_at_the_real_endpoint(self):
        stack, installation = self.stack({"TQA_SOAK_MODEL_URL": "http://vllm:8000/v1", "TQA_SOAK_MODEL_NAME": "qwen"})
        with mock.patch.object(soak_stack.compose, "write",
                               lambda path, text: path.write_text(text)):
            stack.configure(rehearsal=True)
        text = (installation / "models.toml").read_text()
        self.assertIn("base_url='http://vllm:8000/v1'", text)
        self.assertEqual(stack.model_route["context_window"], 32768)

    def test_without_an_endpoint_the_real_soak_refuses_to_start(self):
        stack, _ = self.stack({})
        with mock.patch.object(soak_stack.compose, "write", lambda path, text: path.write_text(text)):
            with self.assertRaisesRegex(AuditError, "TQA_SOAK_MODEL_URL"):
                stack.configure(rehearsal=True)


class SaveLogsTests(unittest.TestCase):
    def test_a_runtime_already_removed_is_skipped_not_a_cleanup_failure(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as out:
            stack = soak_stack.Stack.__new__(soak_stack.Stack)
            stack.output, stack.env = Path(out), {}
            stack.gateway, stack.model = "g" * 12, None
            stack.runtimes = {"conv-a": "a" * 12, "conv-b": "b" * 12}
            present = {"g" * 12, "b" * 12}
            stack.exists = lambda identity: identity in present
            stack.logs = lambda identity, since="0": "log of " + identity
            with mock.patch.object(soak_stack.compose, "write", lambda path, text: path.write_text(text)):
                stack.save_logs()
            self.assertEqual(sorted(path.name for path in Path(out).iterdir()), ["gateway.log", "runtime-conv-b.log"])


class DiskWindowTests(unittest.TestCase):
    def driver(self, disk_windows, io_times=(), recovered=()):
        driver = soak.Soak.__new__(soak.Soak)
        driver.database_windows, driver.disk_windows = [], disk_windows
        driver.conversations = ["c0"]
        driver.clients = [types.SimpleNamespace(io_times=list(io_times)), types.SimpleNamespace(io_times=[])]
        driver.recovered = set(recovered)
        return driver

    def test_nothing_is_sent_while_the_disk_is_full(self):
        self.assertTrue(self.driver([[0.0, float("inf")]]).sending_paused())
        self.assertFalse(self.driver([[0.0, 1.0]]).sending_paused())

    def test_a_conversation_owing_io_recovery_owes_a_completion(self):
        self.assertTrue(self.driver([[0.0, 1.0]], io_times=[0.5]).owes_io_recovery(0))
        self.assertFalse(self.driver([[0.0, 1.0]], io_times=[0.5], recovered={"c0"}).owes_io_recovery(0))
        self.assertFalse(self.driver([[0.0, 1.0]]).owes_io_recovery(0))


class RotationTests(unittest.TestCase):
    def test_a_pending_fault_holds_the_slot_it_targets(self):
        self.assertEqual(soak.Soak.slot_of({"kind": "client_sse", "target": 5}), 2)
        self.assertEqual(soak.Soak.slot_of({"kind": "runtime_link", "target": 5}), 5)


class StorageTests(unittest.TestCase):
    def test_pages_preserve_order_and_request_each_exact_cursor(self):
        cursors = []
        def fetch(after):
            cursors.append(after)
            return {0: {"items": [{"seq": 1}, {"seq": 2}], "next_after": 2},
                    2: {"items": [{"seq": 3}], "next_after": 3},
                    3: {"items": [], "next_after": None}}[after]
        self.assertEqual(list(pages(fetch)), [[{"seq": 1}, {"seq": 2}], [{"seq": 3}]])
        self.assertEqual(cursors, [0, 2, 3])

    def test_bad_pagination_cannot_loop_forever_or_silently_truncate(self):
        for page in ({"items": [], "next_after": 9}, {"items": [{"seq": 1}], "next_after": None},
                     {"items": [{"seq": 1}], "next_after": 0},
                     {"items": [{"seq": 1}], "next_after": 2}):
            with self.subTest(page=page), self.assertRaises(AuditError):
                list(pages(lambda _after: page))
        with self.assertRaisesRegex(AuditError, "cap"):
            list(pages(lambda after: {"items": [{"seq": after + 1}], "next_after": after + 1}, max_pages=2))

    def test_cleanup_attempts_every_owner_after_partial_startup_fault_or_signal(self):
        for original in (None, RuntimeError("startup"), AuditError("fault"), KeyboardInterrupt()):
            visited = []
            failed = RuntimeError("cleanup")
            def first():
                visited.append("reader")
                raise failed
            def second():
                visited.append("containers")
            result = cleanup_actions([first, second], original)
            self.assertIs(result, failed if original is None else original)
            self.assertEqual(visited, ["reader", "containers"])


class NoSwapTests(unittest.TestCase):
    """TQA_SOAK_NO_SWAP=1, the launcher's hard memory cap: no service of the stack may swap (sandboxes never do: the
    gateway sets their swap to their memory)."""

    def test_every_service_swaps_nothing_beyond_its_memory(self):
        rendered = {"services": {"gateway": {"mem_limit": "536870912"}, "postgres": {"mem_limit": "536870912"},
                                 "fake-model": {"mem_limit": "268435456"}}}
        overlay = {"services": {"fake-model": {"image": "x"}}}
        capped = soak_stack.no_swap(overlay, rendered)
        self.assertEqual(capped["services"]["gateway"], {"memswap_limit": "536870912"})
        self.assertEqual(capped["services"]["fake-model"], {"image": "x", "memswap_limit": "268435456"})
        self.assertEqual(set(capped["services"]), {"gateway", "postgres", "fake-model"})

    def test_a_service_without_a_memory_limit_refuses_the_run(self):
        with self.assertRaisesRegex(AuditError, "web has no mem_limit"):
            soak_stack.no_swap({}, {"services": {"web": {}}})

    def test_off_unless_asked(self):
        self.assertFalse(soak_stack.wants_no_swap({}))
        self.assertTrue(soak_stack.wants_no_swap({"TQA_SOAK_NO_SWAP": "1"}))
        with self.assertRaises(AuditError):
            soak_stack.wants_no_swap({"TQA_SOAK_NO_SWAP": "yes"})


class StartupLogTests(unittest.TestCase):
    """A stack that doesn't come up keeps every service's log in the evidence: a service that exits at start (a
    refused configuration) is removed with the installation, and its reason with it."""

    def test_a_failed_start_keeps_the_services_logs_and_fails_as_before(self):
        import subprocess
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as out:
            stack = soak_stack.Stack.__new__(soak_stack.Stack)
            stack.env, stack.output, stack.configuration = {}, Path(out), ["docker", "compose"]
            calls = []

            def command(arguments, timeout=90):
                calls.append(arguments)
                if "up" in arguments:
                    raise subprocess.CalledProcessError(1, arguments)
                return "gateway-1  | error: models.toml: duplicate route"
            stack.command = command
            with self.assertRaises(subprocess.CalledProcessError):
                stack.start_services("60")
            self.assertEqual(calls[-1], ["docker", "compose", "logs", "--no-color", "--timestamps"])
            self.assertIn("duplicate route", (Path(out) / "startup.log").read_text())

    def test_a_start_that_succeeds_keeps_nothing(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as out:
            stack = soak_stack.Stack.__new__(soak_stack.Stack)
            stack.env, stack.output, stack.configuration = {}, Path(out), ["docker", "compose"]
            stack.command = lambda arguments, timeout=90: ""
            stack.start_services("60")
            self.assertFalse((Path(out) / "startup.log").exists())


if __name__ == "__main__":
    unittest.main()
