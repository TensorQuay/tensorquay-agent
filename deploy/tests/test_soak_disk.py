"""H5: the soak's bounded storage and its disk_full fault, with a recording runner (no Docker, no mounts)."""
from pathlib import Path
import tempfile
import unittest

import soak_disk as disk
from soak_audit import AuditError


class Recorder:
    def __init__(self, outputs=None):
        self.calls, self.outputs = [], list(outputs or [])

    def __call__(self, arguments):
        self.calls.append(arguments)
        return self.outputs.pop(0) if self.outputs else ""


class Commands(unittest.TestCase):
    def test_the_tmpfs_is_mounted_in_the_engine_namespace_owned_by_the_container_user(self):
        command = disk.mount_command(4321, "/tmp/x/storage", 64)
        self.assertEqual(command[:7], ["nsenter", "-U", "--preserve-credentials", "-m", "-t", "4321", "--"])
        self.assertEqual(command[7:], ["mount", "-t", "tmpfs", "-o", "size=64m,uid=1000,gid=1000,mode=0700",
                                       "tmpfs", "/tmp/x/storage"])
        self.assertEqual(disk.umount_command(4321, "/tmp/x/storage")[7:], ["umount", "/tmp/x/storage"])

    def test_the_filler_runs_unprivileged_without_network_on_the_storage_only(self):
        command = disk.fill_command("sha256:" + "a" * 64, "/tmp/x/storage", 64)
        for flag in ("--network", "none", "--read-only", "--cap-drop", "ALL", "--user", "1000:1000"):
            self.assertIn(flag, command)
        self.assertIn("type=bind,source=/tmp/x/storage,target=/tmp/x/storage", command)
        self.assertIn(disk.FILLER, command[-1])

    def test_the_filler_may_hold_the_whole_tmpfs_in_its_memory_cgroup(self):
        # tmpfs pages are charged to the writer's memory cgroup: a filler limited below the tmpfs size is OOM-killed
        # (exit 137, candidate 5's rehearsal).
        command = disk.fill_command("sha256:" + "a" * 64, "/tmp/x/storage", 64)
        limit = command[command.index("--memory") + 1]
        self.assertTrue(limit.endswith("m") and int(limit[:-1]) >= 64 + 16, limit)
        self.assertEqual(disk.free_command("sha256:" + "a" * 64, "/tmp/x/storage")[
            disk.free_command("sha256:" + "a" * 64, "/tmp/x/storage").index("--memory") + 1], "32m")

    def test_paths_and_sizes_are_refused_when_unsafe(self):
        for bad in ("relative/storage", "/tmp/x,y/storage", "/"):
            with self.subTest(bad), self.assertRaises(AuditError):
                disk.mount_command(1, bad, 64)
        for size in (0, 4096):
            with self.subTest(size), self.assertRaises(AuditError):
                disk.mount_command(1, "/tmp/x/storage", size)
        with self.assertRaises(AuditError):
            disk.fill_command("tqa-runtime:latest", "/tmp/x/storage", 64)


class Lifecycle(unittest.TestCase):
    def storage(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "storage"
        path.mkdir()
        return path

    def test_mount_fill_free_unmount_in_order(self):
        runner = Recorder(["", "0\n", "", "", ""])
        bounded = disk.BoundedStorage(self.storage(), 64, child_pid=4321, image="sha256:" + "b" * 64, run=runner)
        bounded.mount()
        self.assertEqual(bounded.fill(), 0)
        bounded.free()
        bounded.unmount()
        kinds = [call[7] if call[0] == "nsenter" else call[-1].split()[0] for call in runner.calls]
        self.assertEqual(kinds[0], "mount")
        self.assertEqual(kinds[-1], "umount")
        self.assertFalse(bounded.mounted)

    def test_a_fill_that_leaves_space_fails(self):
        bounded = disk.BoundedStorage(self.storage(), 64, child_pid=1, image="sha256:" + "c" * 64,
                                      run=Recorder(["", "1048576\n"]))
        bounded.mount()
        with self.assertRaisesRegex(AuditError, "not full"):
            bounded.fill()

    def test_unmount_is_safe_when_nothing_was_mounted(self):
        runner = Recorder()
        disk.BoundedStorage(self.storage(), 64, child_pid=1, image="sha256:" + "d" * 64, run=runner).unmount()
        self.assertEqual(runner.calls, [])


class Child(unittest.TestCase):
    def test_the_engine_namespace_comes_from_the_rootless_child_pid(self):
        with tempfile.TemporaryDirectory() as runtime:
            (Path(runtime) / "dockerd-rootless").mkdir()
            (Path(runtime) / "dockerd-rootless/child_pid").write_text("437580\n")
            self.assertEqual(disk.engine_child_pid(runtime), 437580)
        with tempfile.TemporaryDirectory() as runtime, self.assertRaisesRegex(AuditError, "rootless"):
            disk.engine_child_pid(runtime)


if __name__ == "__main__":
    unittest.main()
