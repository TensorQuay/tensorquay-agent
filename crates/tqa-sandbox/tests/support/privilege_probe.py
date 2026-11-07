"""Check mount refusal and expose a stable process snapshot to the host witness."""
import ctypes
import errno
import json
import os
from pathlib import Path
import sys


def privilege_status():
    fields = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())
    for field, expected in {
        "Uid": ["1000"] * 4,
        "CapEff": ["0000000000000000"],
        "CapBnd": ["0000000000000000"],
        "NoNewPrivs": ["1"],
        "Seccomp": ["2"],
    }.items():
        assert fields[field].split() == expected, field + " violated the privilege contract"
    for field in ["CapEff", "CapBnd"]:
        assert int(fields[field], 16) & (1 << 19) == 0, field + " permits CAP_SYS_PTRACE"


def mount_refused():
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mount.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p,
                          ctypes.c_ulong, ctypes.c_void_p]
    libc.mount.restype = ctypes.c_int
    result = libc.mount(b"tmpfs", b"/tmp", b"tmpfs", 0, None)
    assert (result, ctypes.get_errno()) == (-1, errno.EPERM), "mount was not refused"


privilege_status()
mount_refused()
pids = sorted(int(path.name) for path in Path("/proc").iterdir() if path.name.isdigit())
assert 1 in pids and os.getpid() in pids, "container init and probe must be visible"
print(json.dumps(pids), flush=True)
# Keep this process alive until Docker's host-side process count has been sampled.
assert sys.stdin.readline() == "release\n", "host witness did not release the probe"
