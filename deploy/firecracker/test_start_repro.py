"""The 500-start reproduction's harness (unit): what counts as a crash, which engine events are VM hosts' ends, the
summary, the bare clone's container request and the loop's early stop, without Docker or KVM."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import start_repro  # noqa: E402

IMAGE = "sha256:" + "c" * 64


def event(action, code, **labels):
    attributes = dict(labels, exitCode=str(code), name="n")
    return json.dumps({"Type": "container", "Action": action, "timeNano": 1, "Actor": {"ID": "f" * 64,
                                                                                         "Attributes": attributes}})


def inspected(directory):
    return {
        "Config": {"Image": IMAGE, "User": "0:0", "Cmd": ["/usr/local/bin/launch.sh"],
                   "Labels": {"tqa.instance": "net", "tqa.conversation": "c1", "org.opencontainers.image.title": "vmm"}},
        "HostConfig": {"ReadonlyRootfs": True, "CapDrop": ["ALL"], "CapAdd": ["NET_ADMIN", "SETUID"],
                       "SecurityOpt": ["no-new-privileges"],
                       "Devices": [{"PathOnHost": "/dev/kvm", "PathInContainer": "/dev/kvm", "CgroupPermissions": "rwm"}],
                       "Sysctls": {"net.ipv4.ip_forward": "1"}, "Tmpfs": {"/run": "rw,size=16m"},
                       "Memory": 1073741824, "MemorySwap": 1073741824, "NanoCpus": 1500000000, "PidsLimit": 64,
                       "NetworkMode": "tqa-sandbox-net"},
        "Mounts": [{"Source": "/srv/kernel/vmlinux", "Destination": "/vm/vmlinux", "RW": False},
                   {"Source": "/srv/s/ws.img", "Destination": "/vm/workspace.img", "RW": True},
                   {"Source": "/srv/s/config.img", "Destination": "/vm/config.img", "RW": False}],
    }


class Classification(unittest.TestCase):
    def test_a_bare_signal_death_and_firecrackers_fault_exits_are_crashes(self):
        for code in (139, 148, 150, 157):
            self.assertEqual(start_repro.classify(code), "crash", code)
        self.assertEqual([start_repro.classify(code) for code in (None, 0, 137, 143, 1, 147, 158)],
                         ["alive", "clean", "stopped", "stopped", "other", "other", "other"])

    def test_only_vm_hosts_dies_count_and_each_keeps_its_code(self):
        text = "\n".join([
            event("die", 139, **{"tqa.conversation": "c1"}),
            event("die", 0, **{"tqa.instance": "net"}),            # a helper: no conversation label
            event("start", 0, **{"tqa.conversation": "c2"}),        # not an end
            "not json",
            event("die", 143, **{"tqa.conversation": "c3"}),
        ])
        ends = start_repro.vm_host_ends(text)
        self.assertEqual([(end["conversation"], end["exit_code"], end["class"]) for end in ends],
                         [("c1", 139, "crash"), ("c3", 143, "stopped")])


class Summary(unittest.TestCase):
    def test_a_crash_is_reported_with_the_counts(self):
        starts = [{"ok": True}, {"ok": False, "status": 409}, {"ok": True}]
        ends = [{"exit_code": 139, "class": "crash"}, {"exit_code": 143, "class": "stopped"},
                {"exit_code": 143, "class": "stopped"}]
        summary = start_repro.summarize("product", starts, ends, 3)
        self.assertEqual((summary["starts"], summary["succeeded"], summary["failed"]), (3, 2, 1))
        self.assertEqual(summary["ends_by_class"], {"crash": 1, "stopped": 2})
        self.assertEqual(summary["ends_by_exit_code"], {"139": 1, "143": 2})
        self.assertEqual(summary["verdict"], "reproduced: 1 crash(es) in 3 starts")
        self.assertFalse(summary["stopped_early"])

    def test_no_crash_says_how_many_starts_it_covered_and_an_early_stop(self):
        summary = start_repro.summarize("bare", [{"ok": True}] * 4, [{"exit_code": None, "class": "alive"}], 500)
        self.assertEqual(summary["verdict"], "not reproduced in 4 starts")
        self.assertTrue(summary["stopped_early"])
        self.assertEqual(summary["ends_by_exit_code"], {"alive": 1})


class Clone(unittest.TestCase):
    def test_the_clone_is_the_vm_hosts_request_with_copied_files_and_none_of_its_labels(self):
        with tempfile.TemporaryDirectory() as directory:
            for name in ("workspace.img", "config.img"):
                (Path(directory) / name).write_bytes(b"")
            args = start_repro.clone_command(inspected(directory), directory, "tqa-repro-bare-1-0")
        text = " ".join(args)
        for expected in ("--read-only", "--cap-drop ALL", "--cap-add NET_ADMIN", "--cap-add SETUID",
                         "--security-opt no-new-privileges", "--device /dev/kvm:/dev/kvm:rwm",
                         "--sysctl net.ipv4.ip_forward=1", "--tmpfs /run:rw,size=16m", "--memory 1073741824",
                         "--memory-swap 1073741824", "--cpus 1.500", "--pids-limit 64", "--network tqa-sandbox-net",
                         "--user 0:0", "--restart no", "--label tqa-repro.bare=1",
                         "--label org.opencontainers.image.title=vmm"):
            self.assertIn(expected, text)
        self.assertNotIn("tqa.instance", text)
        self.assertNotIn("tqa.conversation", text)
        self.assertIn("type=bind,source=/srv/kernel/vmlinux,target=/vm/vmlinux,readonly", text)
        self.assertIn("type=bind,source={}/workspace.img,target=/vm/workspace.img".format(directory), text)
        self.assertNotIn("/srv/s/ws.img", text)
        self.assertIn("type=bind,source={}/config.img,target=/vm/config.img,readonly".format(directory), text)
        self.assertEqual(args[-2:], [IMAGE, "/usr/local/bin/launch.sh"])


class Loop(unittest.TestCase):
    def test_every_start_is_journaled_and_a_run_of_failures_stops_the_loop(self):
        outcomes = iter([True, False, False, False, True])
        with tempfile.TemporaryDirectory() as directory:
            records = start_repro.loop(5, lambda index: {"ok": next(outcomes)}, Path(directory), stop_after=3)
            journal = (Path(directory) / "starts.jsonl").read_text().splitlines()
        self.assertEqual([record["index"] for record in records], [0, 1, 2, 3])
        self.assertEqual(len(journal), 4)

    def test_a_failure_between_successes_does_not_stop_it(self):
        outcomes = iter([False, True, False, True])
        with tempfile.TemporaryDirectory() as directory:
            records = start_repro.loop(4, lambda index: {"ok": next(outcomes)}, Path(directory), stop_after=2)
        self.assertEqual(len(records), 4)


class Arguments(unittest.TestCase):
    def test_the_mode_and_an_output_directory_are_required(self):
        options = start_repro.arguments(["bare", "--out", "/tmp/x"])
        self.assertEqual((options.mode, options.starts), ("bare", start_repro.STARTS))
        for argv in (["product"], ["other", "--out", "x"], ["product", "--out", "x", "--starts", "0"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit):
                start_repro.arguments(argv)


if __name__ == "__main__":
    unittest.main()
