"""SBX-F-020 (unit): the Firecracker stack's configuration and rootfs reuse, without Docker or KVM."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import stack  # noqa: E402

VMM = "sha256:" + "b" * 64
RUNTIME = "sha256:" + "a" * 64
# An example sandbox network for the configuration and Compose checks (a documentation range).
SUBNET, GATEWAY = "203.0.113.0/24", "203.0.113.10"
DOCKER_CONFIG = ("listen='0.0.0.0:8080'\n[storage]\nroot=\"/srv/s\"\n[runtime]\nbackend='docker'\nimage=\"" + RUNTIME
                 + "\"\n[sandbox]\ngateway_url=\"http://gateway:8080\"\n")


class Configuration(unittest.TestCase):
    def test_the_backend_switches_and_gets_its_section_with_the_pinned_address(self):
        text = stack.firecracker_config(DOCKER_CONFIG, VMM, GATEWAY)
        self.assertIn("backend='firecracker'", text)
        self.assertNotIn("backend='docker'", text)
        self.assertEqual(tomllib.loads(text)["firecracker"], {"vmm_image": VMM, "gateway_address": GATEWAY})

    def test_the_workspace_image_is_sized_by_the_quota_not_the_deprecated_setting(self):
        # A new workspace image is the workspace quota plus ext4 metadata (SBX-F-025); `[firecracker] workspace_bytes`
        # is ignored, so a stack that set only it got the 10 GiB default and the full case's fill outran its deadline.
        for asked, quota in ((None, stack.WORKSPACE_QUOTA), (64 << 20, 64 << 20)):
            with self.subTest(asked=asked):
                text = (stack.firecracker_config(DOCKER_CONFIG, VMM, GATEWAY) if asked is None
                        else stack.firecracker_config(DOCKER_CONFIG, VMM, GATEWAY, workspace_quota=asked))
                self.assertEqual(tomllib.loads(text)["workspaces"], {"quota_bytes": quota})
                self.assertNotIn("workspace_bytes", text)
        self.assertLess(stack.WORKSPACE_QUOTA, 10 << 30, "a test workspace needs little; the default quota is 10 GiB")

    def test_only_a_docker_installation_is_converted_once(self):
        for text in (DOCKER_CONFIG.replace("docker", "firecracker"), DOCKER_CONFIG + "[firecracker]\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                stack.firecracker_config(text, VMM, GATEWAY)

    def test_a_stack_draws_a_free_sandbox_network_and_pins_the_gateway_in_it(self):
        # A fixed subnet made a second microVM stack on the engine fail to create its network (13h-pre's G-13).
        import ipaddress
        network = lambda third: ipaddress.ip_network(stack.VM_NETWORKS.format(third))  # noqa: E731
        expected = lambda third: (str(network(third)), str(network(third).network_address + 10))  # noqa: E731
        used = [str(network(0)), str(list(network(1).subnets(new_prefix=25))[1]), "198.51.100.0/24", "fd00::/64"]
        self.assertEqual(stack.vm_network(used, randbelow=lambda count: 0), expected(2))
        self.assertEqual(stack.vm_network(used, randbelow=lambda count: count - 1), expected(255))
        docker_pools = ipaddress.ip_network(stack.compose.sandbox_pool([], randbelow=lambda count: 0)).supernet(new_prefix=16)
        for third in range(256):  # never the KVM test network, never the Docker stacks' pools
            for other in (ipaddress.ip_network("198.51.100.0/24"), docker_pools):
                self.assertFalse(network(third).overlaps(other), (third, other))
        self.assertEqual(stack.UNDRAWN, {"TQA_FC_SUBNET": expected(0)[0], "TQA_FC_GATEWAY_ADDRESS": expected(0)[1]})

    def test_no_free_sandbox_network_is_refused(self):
        import ipaddress
        with self.assertRaises(RuntimeError):
            stack.vm_network([str(ipaddress.ip_network(stack.VM_NETWORKS.format(0)).supernet(new_prefix=16))])

    def test_a_scenario_may_depend_on_the_gateway_address(self):
        fixed = {"model": "fixture", "responses": []}
        self.assertIs(stack.scenario_for(fixed, GATEWAY), fixed)
        self.assertEqual(stack.scenario_for(lambda gateway: {"gateway": gateway}, GATEWAY), {"gateway": GATEWAY})


class SandboxImages(unittest.TestCase):
    def test_the_vm_host_image_counts_as_under_test_beside_the_pinned_built_ins(self):
        runtime, vmm = "sha256:" + "1" * 64, "sha256:" + "5" * 64
        env = {"TQA_PINNED_IMAGES": runtime}
        stack.vm_host_under_test(env, vmm)
        self.assertEqual(env["TQA_PINNED_IMAGES"], ",".join(sorted([runtime, vmm])))
        stack.vm_host_under_test(env, vmm)
        self.assertEqual(env["TQA_PINNED_IMAGES"].count(vmm), 1, "never twice")


class RootfsCache(unittest.TestCase):
    def cache(self, builder):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: subprocess.run(["rm", "-rf", str(root)], check=True))
        directory = root / RUNTIME.removeprefix("sha256:")
        directory.mkdir()
        if builder is not None:
            (directory / "manifest.json").write_text(json.dumps({"schema": 2, "builder": {"image_id": builder}}))
        return root, directory

    def test_a_rootfs_built_by_this_vm_host_image_is_reused(self):
        root, directory = self.cache(VMM)
        self.assertEqual(stack.cached_rootfs(str(root), RUNTIME, VMM), directory)

    def test_a_rootfs_from_an_older_builder_schema_is_rebuilt(self):
        root, directory = self.cache(VMM)
        (directory / "manifest.json").write_text(json.dumps({"schema": 1, "builder": {"image_id": VMM}}))
        self.assertIsNone(stack.cached_rootfs(str(root), RUNTIME, VMM))

    def test_another_builder_a_missing_or_broken_manifest_or_no_cache_means_building(self):
        root, _ = self.cache("sha256:" + "c" * 64)
        self.assertIsNone(stack.cached_rootfs(str(root), RUNTIME, VMM))
        root, directory = self.cache(None)
        self.assertIsNone(stack.cached_rootfs(str(root), RUNTIME, VMM))
        (directory / "manifest.json").write_text("{")
        self.assertIsNone(stack.cached_rootfs(str(root), RUNTIME, VMM))
        self.assertIsNone(stack.cached_rootfs(None, RUNTIME, VMM))



class ExitedServices(unittest.TestCase):
    ROWS = [{"Service": "gateway", "State": "exited", "ExitCode": 1}, {"Service": "postgres", "State": "running", "ExitCode": 0},
            {"Service": "migrate", "State": "exited", "ExitCode": 0}, {"Service": "web", "State": "dead", "ExitCode": 137}]

    def test_a_failed_service_is_always_reported_and_a_clean_exit_only_after_a_failure(self):
        lines = "\n".join(json.dumps(row) for row in self.ROWS)
        self.assertEqual(stack.exited_services(lines, failed=False), ["gateway", "web"])
        self.assertEqual(stack.exited_services(lines, failed=True), ["gateway", "migrate", "web"])

    def test_both_compose_output_forms_and_nothing_are_understood(self):
        self.assertEqual(stack.exited_services(json.dumps(self.ROWS), failed=False), ["gateway", "web"])
        self.assertEqual(stack.exited_services("", failed=True), [])
        self.assertEqual(stack.exited_services("[]", failed=True), [])

    def test_the_logs_of_each_are_printed_redacted_before_teardown(self):
        calls = []

        def run(args, env, **_):
            calls.append(args[-4:])
            output = json.dumps(self.ROWS[0]) if "ps" in args else "boot failed: password=secret-value\n"
            return subprocess.CompletedProcess(args, 0, stdout=output, stderr="")
        printed = []
        stack.dump_exited(["docker", "compose"], {"TQA_POSTGRES_PASSWORD": "secret-value"}, failed=True, run=run,
                          out=printed.append)
        self.assertEqual(calls[1], ["--no-color", "--tail", str(stack.LOG_TAIL_LINES), "gateway"])
        self.assertIn("gateway exited", printed[0])
        self.assertIn("boot failed: password=[REDACTED]", printed[0])
        self.assertNotIn("secret-value", "".join(printed))

    def test_a_failing_diagnostic_never_hides_the_case_outcome(self):
        def run(args, env, **_):
            raise subprocess.CalledProcessError(1, args)
        printed = []
        stack.dump_exited(["docker", "compose"], {}, failed=True, run=run, out=printed.append)
        self.assertIn("could not list", printed[0])


class UpFailure(unittest.TestCase):
    """When the stack does not come up, each service's engine state and health probes, and the gateway's log tail."""

    INSPECTED = [{"Name": "/tqa-x-gateway",
                  "State": {"Status": "running", "Running": True, "ExitCode": 0, "StartedAt": "2026-09-28T02:39:30Z",
                            "FinishedAt": "0001-01-01T00:00:00Z",
                            "Health": {"Status": "unhealthy", "FailingStreak": 14, "Log": [
                                {"ExitCode": 1, "Output": "old probe"},
                                {"ExitCode": 1, "Output": "Error: io token=secret-value"},
                                {"ExitCode": -1, "Output": "Health check exceeded timeout (3s)"}]}}},
                 {"Name": "/tqa-x-postgres-1",
                  "State": {"Status": "running", "Running": True, "ExitCode": 0, "StartedAt": "2026-09-28T02:39:10Z",
                            "FinishedAt": "0001-01-01T00:00:00Z", "Health": {"Status": "healthy", "Log": []}}}]

    def runner(self, calls, logs="line one\nline two token=secret-value\n"):
        def run(args, env, **_):
            calls.append(args)
            if "ps" in args:
                return subprocess.CompletedProcess(args, 0, stdout="id1\nid2\n", stderr="")
            if args[:2] == ["docker", "inspect"]:
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(self.INSPECTED), stderr="")
            return subprocess.CompletedProcess(args, 0, stdout=logs, stderr="")
        return run

    def test_each_service_prints_its_state_health_and_last_probes_redacted(self):
        calls, printed = [], []
        stack.dump_service_states(["docker", "compose"], {"TQA_BOOTSTRAP_TOKEN": "secret-value"},
                                  run=self.runner(calls), out=printed.append)
        joined = "\n".join(printed)
        self.assertEqual(calls[0], ["docker", "compose", "ps", "--all", "-q"])
        self.assertEqual(calls[1], ["docker", "inspect", "id1", "id2"])
        for expected in ("tqa-x-gateway", "running", "unhealthy", "streak 14", "Health check exceeded timeout",
                         "tqa-x-postgres-1", "healthy", "2026-09-28T02:39:30Z"):
            self.assertIn(expected, joined)
        self.assertNotIn("old probe", joined)  # the last two probes only
        self.assertNotIn("secret-value", joined)

    def test_the_gateway_log_tail_is_printed_whole_and_redacted(self):
        calls, printed = [], []
        stack.dump_gateway_tail(["docker", "compose"], {"TQA_BOOTSTRAP_TOKEN": "secret-value"},
                                run=self.runner(calls), out=printed.append)
        self.assertEqual(calls[0], ["docker", "compose", "logs", "--no-color", "--tail", str(stack.LOG_TAIL_LINES),
                                    "gateway"])
        joined = "\n".join(printed)
        self.assertIn("line one", joined)
        self.assertNotIn("secret-value", joined)

    def test_a_failing_diagnostic_never_raises(self):
        def broken(args, env, **_):
            raise subprocess.CalledProcessError(1, args)
        printed = []
        stack.dump_service_states(["docker", "compose"], {}, run=broken, out=printed.append)
        stack.dump_gateway_tail(["docker", "compose"], {}, run=broken, out=printed.append)
        self.assertEqual(len(printed), 2)
        self.assertTrue(all(line.startswith("could not") for line in printed), printed)

    def test_a_running_gateway_prints_its_threads_and_wait_channels(self):
        calls, printed = [], []
        top = "PID LWP STAT WCHAN ELAPSED COMMAND\n7 7 Ssl ep_poll 02:00 tqa-gateway\n7 9 Dl folio_wait 02:00 tokio-blocking\n"

        def run(args, env, **_):
            calls.append(args)
            return subprocess.CompletedProcess(args, 0, stdout="gw1\n" if "ps" in args else top, stderr="")
        stack.dump_gateway_threads(["docker", "compose"], {}, run=run, out=printed.append)
        self.assertEqual(calls[0], ["docker", "compose", "ps", "-q", "gateway"])
        self.assertEqual(calls[1], ["docker", "top", "gw1", "-eLo", "pid,lwp,stat,wchan:32,etime,comm"])
        self.assertIn("folio_wait", "\n".join(printed))

        def gone(args, env, **_):
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
        printed = []
        stack.dump_gateway_threads(["docker", "compose"], {}, run=gone, out=printed.append)
        self.assertIn("no gateway process", printed[0])

    def test_the_start_up_retries_are_printed_after_every_start(self):
        lines = "\n".join([
            json.dumps({"level": "INFO", "fields": {"message": "settings loaded"}}),
            json.dumps({"level": "WARN", "fields": {"message": "a Docker API request for the Firecracker backend timed out",
                                                    "bound_seconds": 10}}),
            json.dumps({"level": "WARN", "fields": {
                "message": "the sandbox backend's engine timed out at start-up; retrying", "attempts": 1,
                "retry_in_seconds": 1}}),
            "tqa-gateway: not json"])

        def run(args, env, **_):
            return subprocess.CompletedProcess(args, 0, stdout=lines, stderr="")
        printed = []
        stack.dump_startup_retries(["docker", "compose"], {}, run=run, out=printed.append)
        joined = "\n".join(printed)
        self.assertIn("gateway start-up: 1 engine timeout(s), 1 retry(ies)", joined)
        self.assertIn('"attempts": 1', joined)
        self.assertNotIn("settings loaded", joined)

    def test_both_are_printed_only_when_the_stack_did_not_come_up(self):
        self.assertTrue(stack.came_up_failed(subprocess.CalledProcessError(1, ["docker", "compose", "up", "-d"]),
                                             prepared=False))
        self.assertFalse(stack.came_up_failed(AssertionError("case failed"), prepared=True))
        self.assertFalse(stack.came_up_failed(None, prepared=False))


class VmHostRecording(unittest.TestCase):
    """A VM host lost mid-case is removed by the gateway before any failure dump; the recorder follows every VM host
    from the moment it appears, so a failed case prints each one that existed, gone ones included."""

    def recorder(self, listed, states):
        self.followed = []
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        recorder = stack.VmHostRecorder(
            {"TQA_BOOTSTRAP_TOKEN": "secret-value"}, Path(directory.name),
            hosts=lambda: listed.pop(0) if listed else [],
            state=lambda host: states.get(host),
            follow=lambda host, path: (self.followed.append(host), path.write_text(
                "2026-09-28T09:16:40Z console line\n2026-09-28T09:16:48Z tqa-vm-init: stage two ended: signal 9 "
                "token=secret-value\n" if host == "aaa" else "2026-09-28T09:16:56Z new vm\n"))[0])
        return recorder

    def test_each_host_is_followed_once_from_when_it_appears_and_kept_after_it_is_gone(self):
        states = {"aaa": {"Status": "exited", "ExitCode": 137, "FinishedAt": "2026-09-28T09:16:48Z"},
                  "bbb": {"Status": "running", "ExitCode": 0}}
        recorder = self.recorder([["aaa"], ["aaa"], ["bbb"], ["bbb"]], states)
        for _ in range(4):
            recorder.poll()
        self.assertEqual(self.followed, ["aaa", "bbb"])
        printed = []
        recorder.dump(out=printed.append)
        joined = "\n".join(printed)
        self.assertIn("VM host aaa (gone): exited, exit 137, finished 2026-09-28T09:16:48Z", joined)
        self.assertIn("stage two ended: signal 9", joined)
        self.assertIn("VM host bbb: running", joined)
        self.assertNotIn("secret-value", joined)

    def test_the_engine_events_name_an_oom_kill_and_the_exit_code_of_a_host_gone_between_polls(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        events = [{"Type": "container", "Action": "start", "Actor": {"ID": "c7e3" + "0" * 60,
                                                                       "Attributes": {"name": "vm-a"}}, "time": 1},
                  {"Type": "container", "Action": "oom", "Actor": {"ID": "c7e3" + "0" * 60,
                                                                     "Attributes": {"name": "vm-a"}}, "time": 2},
                  {"Type": "container", "Action": "die", "Actor": {"ID": "c7e3" + "0" * 60,
                                                                     "Attributes": {"name": "vm-a", "exitCode": "137"}},
                   "time": 2}]
        recorder = stack.VmHostRecorder({}, Path(directory.name), hosts=lambda: [], state=lambda host: None,
                                        follow=lambda host, path: None,
                                        events=lambda path: path.write_text("\n".join(json.dumps(e) for e in events)))
        recorder.start_events()
        printed = []
        recorder.dump(out=printed.append)
        joined = "\n".join(printed)
        self.assertIn("c7e300000000 vm-a oom", joined)
        self.assertIn("c7e300000000 vm-a die exitCode=137", joined)
        self.assertNotIn(" start", joined.split("--- engine events")[1])

    def test_a_failing_listing_or_inspect_never_raises(self):
        def broken():
            raise subprocess.CalledProcessError(1, ["docker"])
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        recorder = stack.VmHostRecorder({}, Path(directory.name), hosts=broken, state=lambda host: None,
                                        follow=lambda host, path: None)
        recorder.poll()
        printed = []
        recorder.dump(out=printed.append)
        self.assertIn("no VM host was seen", printed[0])


class Overlay(unittest.TestCase):
    ENV = {"TQA_INSTALL_DIR": "/srv/i", "TQA_STORAGE_ROOT": "/srv/s", "TQA_POSTGRES_PASSWORD": "rendered-only",
           "TQA_BOOTSTRAP_TOKEN": "rendered-only"}

    def render(self, extra):
        env = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/"), **self.ENV, **extra}
        files = ["-f", "deploy/compose/docker.yml", "-f", "deploy/compose/firecracker.yml"]
        return subprocess.run(["docker", "compose", *files, "config", "--format", "json"], cwd=stack.ROOT, env=env,
                              capture_output=True, text=True, timeout=60, check=False)

    def test_the_gateway_has_the_pinned_address_on_the_internal_sandbox_network(self):
        result = self.render({"TQA_FC_SUBNET": SUBNET, "TQA_FC_GATEWAY_ADDRESS": GATEWAY})
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads(result.stdout)
        networks = config["services"]["gateway"]["networks"]
        self.assertEqual(networks["sandboxes"]["ipv4_address"], GATEWAY)
        self.assertEqual(set(networks), {"control", "sandboxes", "web"})
        self.assertEqual(config["networks"]["sandboxes"]["ipam"]["config"], [{"subnet": SUBNET}])
        self.assertIs(config["networks"]["sandboxes"]["internal"], True)

    def test_the_overlay_needs_both_values(self):
        for missing in ("TQA_FC_SUBNET", "TQA_FC_GATEWAY_ADDRESS"):
            values = {"TQA_FC_SUBNET": SUBNET, "TQA_FC_GATEWAY_ADDRESS": GATEWAY}
            del values[missing]
            with self.subTest(missing=missing):
                result = self.render(values)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(missing, result.stderr)


if __name__ == "__main__":
    unittest.main()


class ForcedRemoval(unittest.TestCase):
    """The destroy contract's `docker rm --force` after a stop: the gateway may have removed the VM host first."""

    def test_a_removal_that_succeeds_is_removed(self):
        self.assertEqual(stack.forced_removal(0, ""), "removed")

    def test_only_an_already_gone_host_is_accepted(self):
        for stderr in ("Error response from daemon: No such container: bd3d95b425ed",
                       "Error response from daemon: removal of container bd3d95b425ed is already in progress"):
            with self.subTest(stderr):
                self.assertEqual(stack.forced_removal(1, stderr), "already gone")

    def test_anything_else_fails_and_carries_its_stderr(self):
        with self.assertRaisesRegex(AssertionError, "cannot kill container: permission denied"):
            stack.forced_removal(1, "Error response from daemon: cannot kill container: permission denied")
        with self.assertRaisesRegex(AssertionError, "exit 1"):
            stack.forced_removal(1, "")


class HistoryScenario(unittest.TestCase):
    """contract.py history's script, served in arrival order: the first conversation never runs the probe."""

    def test_only_the_second_conversation_runs_the_probe(self):
        import contract
        import history_checks
        scenario = contract.HISTORY
        self.assertEqual(scenario["select"], "arrival")
        texts = [json.dumps(response) for response in scenario["responses"]]
        self.assertNotIn(history_checks.FORGED, texts[0] + texts[1], "the first conversation's turn is harmless")
        self.assertIn(history_checks.PROBE_MARK, texts[2], "the second conversation's turn is the probe")
        self.assertNotIn(history_checks.FORGED, texts[4] + texts[5], "the first conversation's resume is harmless")


class PidsScenario(unittest.TestCase):
    """contract.py pids (SEC-010 on the microVM), served in arrival order: the stormed conversation's storm first, the
    neighbour's command and answer while the storm holds, then the stormed conversation's answer."""

    def test_the_storm_comes_first_and_the_neighbour_answers_inside_it(self):
        import contract
        import pids_checks
        scenario, check, settings, _ = contract.CASES["pids"]
        self.assertIs(check, contract.pids)
        self.assertEqual(scenario["select"], "arrival")
        commands = [response["events"][0].get("arguments", {}).get("command", "") for response in scenario["responses"]]
        self.assertIn(pids_checks.STORM, commands[0])
        self.assertIn(contract.NEIGHBOUR_MARK, commands[1])
        self.assertEqual(commands[2:], ["", ""], "then two closing answers")

    def test_the_case_configures_the_limit_it_checks_and_a_short_idle(self):
        import contract
        settings = contract.CASES["pids"][2]
        self.assertIn("pids={}\n".format(contract.PIDS_CONFIGURED), settings)
        self.assertIn("idle_seconds=30\n", settings)


class FailureDiagnostics(unittest.TestCase):
    """What a failed case prints besides the exited services: the VM hosts, the gateway's warnings, the timeline."""

    def test_a_timeout_names_the_conversations_last_events(self):
        stored = [{"seq": index, "event": {"type": kind}}
                  for index, kind in enumerate(["run.started", "tool.start", "run.resumed", "step.started"], 1)]
        self.assertEqual(stack.timeline(stored, last=3), "2 tool.start, 3 run.resumed, 4 step.started")
        self.assertEqual(stack.timeline([], last=3), "no stored events")

    def test_every_vm_host_is_listed_with_its_state_and_last_lines_redacted(self):
        calls = []

        def run(args, env, **_):
            calls.append(args)
            if args[:2] == ["docker", "ps"]:
                output = "aaa111 exited(1) tqa-fc-x\nbbb222 running tqa-fc-y\n"
            else:
                output = "KVM: Permission denied token=secret-value\n"
            return subprocess.CompletedProcess(args, 0, stdout=output, stderr="")
        printed = []
        stack.dump_vm_hosts({"TQA_SANDBOX_NETWORK": "net", "TQA_BOOTSTRAP_TOKEN": "secret-value"}, run=run,
                            out=printed.append)
        self.assertIn("label=tqa.instance=net", " ".join(calls[0]))
        self.assertTrue(any("aaa111 exited(1)" in line for line in printed))
        self.assertTrue(any("KVM: Permission denied" in line for line in printed))
        self.assertNotIn("secret-value", "".join(printed))

    def test_the_gateway_warnings_are_printed_and_a_failing_diagnostic_hides_nothing(self):
        lines = "\n".join([json.dumps({"level": "INFO", "fields": {"message": "started sandbox"}}),
                           json.dumps({"level": "WARN", "fields": {"message": "sandbox failed with no queued action"}}),
                           "not json"])

        def run(args, env, **_):
            return subprocess.CompletedProcess(args, 0, stdout=lines, stderr="")
        printed = []
        stack.dump_gateway_warnings(["docker", "compose"], {}, run=run, out=printed.append)
        joined = "\n".join(printed)
        self.assertIn("sandbox failed with no queued action", joined)
        self.assertNotIn("started sandbox", joined)

        def broken(args, env, **_):
            raise subprocess.CalledProcessError(1, args)
        printed = []
        stack.dump_gateway_warnings(["docker", "compose"], {}, run=broken, out=printed.append)
        self.assertIn("could not read", printed[0])
