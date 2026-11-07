"""H5: the soak's storage root on a bounded tmpfs, and the disk_full fault.

The storage root is a host path that the gateway and every sandbox see through bind mounts, which the rootless engine
resolves in its own mount namespace. A tmpfs mounted at that path in that namespace (as this user; the engine's user
namespace makes it root there, no sudo) is therefore the one filesystem both sides write, bounded at a few MiB of
memory. The disk_full fault fills it from a throwaway, unprivileged helper container and frees it at the window's end.
Nothing loads the host.
"""
from pathlib import Path
import re

from soak_audit import require

FILLER = ".soak-disk-full"
MIN_MIB, MAX_MIB = 16, 1024
CONTAINER_USER = "1000:1000"


def _safe(path):
    text = str(path)
    require(text.startswith("/") and text != "/" and "," not in text and ":" not in text,
            "unsafe storage path for the bounded filesystem")
    return text


def engine_child_pid(runtime_dir):
    """The rootless engine's namespace holder (rootlesskit's child)."""
    path = Path(runtime_dir) / "dockerd-rootless" / "child_pid"
    require(path.is_file(), "no rootless engine namespace (dockerd-rootless/child_pid); disk_full needs rootless Docker")
    return int(path.read_text().strip())


def _enter(child_pid):
    return ["nsenter", "-U", "--preserve-credentials", "-m", "-t", str(int(child_pid)), "--"]


def mount_command(child_pid, path, mib):
    require(MIN_MIB <= int(mib) <= MAX_MIB, "the bounded storage must be {}–{} MiB".format(MIN_MIB, MAX_MIB))
    return _enter(child_pid) + ["mount", "-t", "tmpfs", "-o", "size={}m,uid=1000,gid=1000,mode=0700".format(int(mib)),
                                "tmpfs", _safe(path)]


def umount_command(child_pid, path):
    return _enter(child_pid) + ["umount", _safe(path)]


def _helper(image, path, script, memory_mib=32):
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", image), "the helper image must be pinned by its ID")
    path = _safe(path)
    return ["docker", "run", "--rm", "--network", "none", "--read-only", "--cap-drop", "ALL", "--security-opt",
            "no-new-privileges", "--pids-limit", "16", "--memory", "{}m".format(int(memory_mib)), "--user",
            CONTAINER_USER, "--mount",
            "type=bind,source={0},target={0}".format(path), "--entrypoint", "sh", image, "-c", script]


def fill_command(image, path, mib):
    """Writes zeros until the filesystem is full (the write's own ENOSPC ends it), then prints the space left. tmpfs
    pages are charged to the writer's memory cgroup, so the helper's limit covers the whole tmpfs plus its own use."""
    target = "{}/{}".format(_safe(path), FILLER)
    return _helper(image, path, "head -c {} /dev/zero > {} 2>/dev/null; df -B1 --output=avail {} | tail -1".format(
        (MAX_MIB + 1) * 1024 * 1024, target, _safe(path)), memory_mib=int(mib) + 32)


def free_command(image, path):
    return _helper(image, path, "rm -f {}/{}".format(_safe(path), FILLER))


class BoundedStorage:
    """Mount, fill, free and unmount, through `run(arguments) -> stdout` (the stack's own bounded command runner)."""

    def __init__(self, path, mib, child_pid, image, run):
        self.path, self.mib, self.child_pid, self.image, self.run = Path(path), mib, child_pid, image, run
        self.mounted, self.filled = False, False

    def mount(self):
        self.run(mount_command(self.child_pid, self.path, self.mib))
        self.mounted = True

    def fill(self):
        """Fills the storage; returns the bytes left, which must be none."""
        self.filled = True
        left = int((self.run(fill_command(self.image, self.path, self.mib)) or "0").strip().splitlines()[-1])
        require(left == 0, "the storage is not full after the filler ({} bytes left)".format(left))
        return left

    def free(self):
        self.run(free_command(self.image, self.path))
        self.filled = False

    def unmount(self):
        if self.mounted:
            self.run(umount_command(self.child_pid, self.path))
            self.mounted = False
