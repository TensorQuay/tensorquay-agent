"""Observe mounts, usable block devices and host-path refusal inside a runtime."""
import errno
import json
import os
from pathlib import Path
import stat
import sys


def mounts():
    entries = [line.split() for line in Path("/proc/self/mountinfo").read_text().splitlines()]
    workspace = [entry for entry in entries
                 if entry[4] == "/workspace" or entry[4].startswith("/workspace/")]
    assert len(workspace) == 1 and workspace[0][4] == "/workspace", "unexpected workspace mount"
    assert "rw" in workspace[0][5].split(","), "bound workspace is not writable"
    return [entry[4] for entry in workspace]


def block_devices():
    devices = []
    for directory, _, files in os.walk("/dev"):
        for name in files:
            path = Path(directory, name)
            if stat.S_ISBLK(path.lstat().st_mode):
                devices.append(str(path))
    assert not devices, "a block device is exposed to the runtime"
    return devices


def refuse_host_path(path):
    assert not path.exists(), "host workspace path exists inside the runtime"
    for mode in ["r", "w"]:
        try:
            with path.open(mode) as stream:
                if mode == "r":
                    stream.read(1)
                else:
                    stream.write("escaped")
        except OSError as error:
            assert error.errno == errno.ENOENT, "unexpected host path failure"
        else:
            raise AssertionError("host workspace path is accessible")


assert Path("/workspace/witness").read_text() == "bound-workspace"
workspace_mounts = mounts()
devices = block_devices()
refuse_host_path(Path(sys.argv[1]))
refuse_host_path(Path(sys.argv[2]) / "witness")
print(json.dumps({"workspace_mounts": workspace_mounts, "block_devices": devices,
                  "host_paths_refused": 2}))
