"""Firecracker guest probe: the image's entrypoint instead of the runtime, run by the guest init as the sandbox user.

It checks the sandbox contract from inside a real microVM (SBX-C-005, 006, 007, 016 on the firecracker backend) and
reports through the exit record: 0 when every check holds, 1 otherwise. Findings are printed as one JSON line on the
console, which the host test reads from the VM host container's log. PROBE_CHECK selects the suite.
"""
import ctypes
import errno
import json
import os
from pathlib import Path
import stat
import sys

FAILURES = []


def check(name, condition, detail=""):
    if not condition:
        FAILURES.append({"check": name, "detail": str(detail)[:200]})


def status_fields():
    return dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())


def privileges():
    fields = status_fields()
    check("uid", fields["Uid"].split() == ["1000"] * 4, fields["Uid"])
    check("gid", fields["Gid"].split() == ["1000"] * 4, fields["Gid"])
    for field in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"]:
        check(field, int(fields[field], 16) == 0, fields[field])
    check("no_new_privs", fields["NoNewPrivs"].strip() == "1", fields["NoNewPrivs"])
    check("seccomp", fields["Seccomp"].strip() == "2", fields["Seccomp"])
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mount.argtypes = [ctypes.c_char_p] * 3 + [ctypes.c_ulong, ctypes.c_void_p]
    result = libc.mount(b"tmpfs", b"/tmp", b"tmpfs", 0, None)
    check("mount_refused", (result, ctypes.get_errno()) == (-1, errno.EPERM), ctypes.get_errno())


def processes():
    commands = {}
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                commands[int(entry.name)] = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode().strip()
            except OSError:
                pass
    check("private_pid_namespace", not any("tqa-vm-init" in command and "--stage2" not in command
                                           for command in commands.values()), commands)
    check("pid_one_is_the_entrypoint", 1 in commands, commands)


def filesystem():
    for path in ["/usr/probe-write", "/etc/probe-write", "/probe-write"]:
        try:
            Path(path).write_text("x")
            check("read_only_root", False, path)
        except OSError as error:
            check("read_only_root", error.errno in (errno.EROFS, errno.EACCES), (path, error.errno))
    for directory in ["/tmp", "/workspace", "/run/tqa/state"]:
        target = Path(directory, "probe-write")
        try:
            target.write_text("x")
            target.unlink()
        except OSError as error:
            check("writable", False, (directory, error.errno))


def token():
    path = Path(os.environ.get("TQA_RUNTIME_TOKEN_FILE", ""))
    check("token_file_named", str(path) == "/run/tqa/token", path)
    info = path.stat()
    check("token_mode", stat.S_IMODE(info.st_mode) in (0o400, 0o600) and info.st_uid == 1000, oct(info.st_mode))
    secret = path.read_bytes()
    check("token_present", len(secret) > 0)
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                check("token_not_in_environ", secret not in (entry / "environ").read_bytes(), entry.name)
            except OSError:
                pass
    check("token_not_in_cmdline", secret not in Path("/proc/cmdline").read_bytes())


def storage():
    mounts = [line.split() for line in Path("/proc/self/mountinfo").read_text().splitlines()]
    workspace = [entry for entry in mounts if entry[4] == "/workspace" or entry[4].startswith("/workspace/")]
    check("one_workspace_mount", len(workspace) == 1 and "rw" in workspace[0][5].split(","), workspace)
    devices = []
    for directory, _, files in os.walk("/dev"):
        for name in files:
            path = Path(directory, name)
            try:
                if stat.S_ISBLK(path.lstat().st_mode):
                    devices.append(str(path))
            except OSError:
                pass
    check("no_block_devices", not devices, devices)
    for name in ["null", "zero", "urandom"]:
        try:
            with open(f"/dev/{name}", "r+b") as stream:
                stream.read(1)
        except OSError as error:
            check("character_devices_usable", False, (name, error.errno))
    for device in ["/dev/vdd", "/dev/vde", "/dev/vda"]:
        try:
            with open(device, "rb") as stream:
                stream.read(1)
            check("devices_unreadable", False, device)
        except OSError as error:
            check("devices_unreadable", error.errno in (errno.EACCES, errno.ENOENT, errno.EPERM), (device, error.errno))


def names():
    check("no_resolver", Path("/etc/resolv.conf").read_text().strip() == "", Path("/etc/resolv.conf").read_text())
    hosts = [line.split() for line in Path("/etc/hosts").read_text().splitlines() if line.strip()]
    check("hosts_only_loopback_and_gateway", all(entry[1] == "localhost" or len(entry) == 2 for entry in hosts), hosts)


def egress():
    # SBX-C-004 and SBX-F-004 from inside the guest: only the gateway's address on its configured port is reachable.
    import socket

    gateway, bystander = os.environ["PROBE_GATEWAY"], os.environ["PROBE_BYSTANDER"]

    def tcp(host, port):
        try:
            with socket.create_connection((host, port), timeout=3):
                return True
        except OSError:
            return False

    check("gateway_link_port_reachable", tcp(gateway, 8080))
    check("gateway_other_port_blocked", not tcp(gateway, 9999))
    check("other_address_blocked", not tcp(bystander, 8080))
    check("public_address_blocked", not tcp("203.0.113.7", 443))
    query = b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x03com\x00\x00\x01\x00\x01"
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp:
        udp.settimeout(3)
        try:
            udp.sendto(query, (gateway, 53))
            udp.recvfrom(512)
            check("udp_dns_blocked", False)
        except OSError:
            pass
    check("gateway_name_from_hosts", socket.gethostbyname("tqa-gateway") == gateway)
    try:
        socket.getaddrinfo("example.com", 443)
        check("no_external_resolution", False)
    except OSError:
        pass


def durability():
    # ARCHITECTURE §4.5 (defence in depth for background writes): the guest writes dirty data back within a second,
    # and the workspace's ext4 journal commits every second.
    for name in ("dirty_expire_centisecs", "dirty_writeback_centisecs"):
        with open("/proc/sys/vm/" + name) as value:
            text = value.read().strip()
        check(name, text == "100", text)
    with open("/proc/mounts") as mounts:
        workspace = [line.split() for line in mounts if line.split()[1:2] == ["/workspace"]]
    check("workspace_commit_1s", len(workspace) == 1 and "commit=1" in workspace[0][3].split(","), workspace)


def flush_cost():
    # PERF-013: what `sync_before_result` adds per tool result. A small file write, timed without and with syncfs on
    # the workspace, 50 times each, interleaved; then a 64 MiB write and its flush, 5 times.
    import ctypes
    import time
    libc = ctypes.CDLL(None, use_errno=True)
    workspace = os.open("/workspace", os.O_RDONLY | os.O_DIRECTORY)
    samples = {"plain": [], "flushed": []}
    for index in range(50):
        for series in ("plain", "flushed"):
            started = time.perf_counter()
            with open("/workspace/flush-{}-{}".format(series, index), "wb") as target:
                target.write(b"x" * 4096)
            if series == "flushed":
                check("syncfs", libc.syncfs(workspace) == 0, ctypes.get_errno())
            samples[series].append(round((time.perf_counter() - started) * 1e6))
    # After a large tool output (an install, a build tree): the flush writes it back first.
    samples["flushed_64mib"] = []
    for index in range(5):
        started = time.perf_counter()
        with open("/workspace/flush-large-{}".format(index), "wb") as target:
            target.write(b"x" * (64 << 20))
        check("syncfs_large", libc.syncfs(workspace) == 0, ctypes.get_errno())
        samples["flushed_64mib"].append(round((time.perf_counter() - started) * 1e6))
        os.remove("/workspace/flush-large-{}".format(index))
    summary = {}
    for series, values in samples.items():
        values.sort()
        summary[series] = {"p50_us": values[len(values) // 2], "p95_us": values[int(len(values) * 0.95)]}
    print("tqa-probe: flush " + json.dumps(summary), flush=True)


def listening():
    # SBX-C-001: nothing listens inside the sandbox; the runtime dials out.
    listeners = []
    for table in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            rows = Path(table).read_text().splitlines()[1:]
        except OSError:
            continue
        listeners += [row.split()[1] for row in rows if row.split()[3] == "0A"]
    check("nothing_listens", not listeners, listeners)


PERSISTED = Path("/workspace/persisted/state.txt")


def persist_write():
    # SBX-C-002, first boot: a file, a directory and their modes, left for the next VM on the same image.
    PERSISTED.parent.mkdir(mode=0o750)
    PERSISTED.write_text("survives the VM\n")
    PERSISTED.chmod(0o640)
    os.sync()


def persist_read():
    # SBX-C-002, second boot on the same workspace image: content, owner and permissions are intact.
    check("persisted_content", PERSISTED.read_text() == "survives the VM\n")
    for path, mode in ((PERSISTED, 0o640), (PERSISTED.parent, 0o750)):
        info = path.stat()
        check("persisted_mode", stat.S_IMODE(info.st_mode) == mode, (str(path), oct(info.st_mode)))
        check("persisted_owner", (info.st_uid, info.st_gid) == (1000, 1000), (str(path), info.st_uid, info.st_gid))


def limits():
    # SBX-C-003: memory beyond the guest is OOM-killed inside the sandbox and the sandbox survives; CPU time over a
    # burst stays within the VM's vCPUs.
    import subprocess
    import time
    # Grows in touched 64 MiB chunks, so the kernel's OOM killer acts (one large request would only be refused).
    grow = "chunks = []\nwhile True:\n    chunk = bytearray(64 << 20)\n    chunk[::4096] = b'x' * len(chunk[::4096])\n" \
           "    chunks.append(chunk)"
    # Started the way the runtime starts every tool (tqa-tools tool_command): at the raised OOM score, so the killer may
    # take it. The probe itself, like the runtime, keeps the protected score it inherited from the init.
    as_tool = ["/bin/sh", "-c", "printf 500 2>/dev/null >/proc/self/oom_score_adj; exec \"$@\"", "sh"]
    hog = subprocess.run(as_tool + [sys.executable, "-c", grow], capture_output=True, timeout=120)
    check("oom_killed_child", hog.returncode == -9, hog.returncode)
    # The data behind the guest's memory reserve (tqa-vm-init SANDBOX_MEMORY_RESERVE).
    meminfo = dict(line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines())
    def cgroup(name):
        try:
            return Path("/sys/fs/cgroup", name).read_text().strip()
        except OSError:
            return "?"
    limit, peak = cgroup("memory.max"), cgroup("memory.peak")
    print("tqa-probe: memory " + json.dumps({"mem_total": meminfo["MemTotal"].strip(),
                                            "sandbox_max": limit, "sandbox_peak": peak}), flush=True)
    cpus = os.cpu_count()
    check("vcpus", cpus == int(os.environ["PROBE_VCPUS"]), cpus)
    burners = [subprocess.Popen([sys.executable, "-c", "import time\nend=time.time()+2\nwhile time.time()<end: pass"])
               for _ in range(cpus * 2)]
    started = time.monotonic()
    before = os.times()
    for burner in burners:
        burner.wait(timeout=60)
    wall = time.monotonic() - started
    after = os.times()
    used = (after.children_user - before.children_user) + (after.children_system - before.children_system)
    check("cpu_within_vcpus", used <= cpus * wall * 1.1, (used, cpus, wall))


def clock():
    # SBX-C-012: the guest keeps UTC; it reports its time, which the host brackets with its own.
    import time
    check("utc", time.localtime().tm_gmtoff == 0, time.localtime().tm_gmtoff)
    print("tqa-probe: clock {:.3f}".format(time.time()), flush=True)


def fill():
    # SBX-C-015: writing past the workspace image fails with ENOSPC inside the guest, and the sandbox survives.
    target = Path("/workspace/fill.bin")
    chunk = b"x" * (1 << 20)
    try:
        with open(target, "wb") as stream:
            while True:
                stream.write(chunk)
                stream.flush()
                os.fsync(stream.fileno())
    except OSError as error:
        check("enospc", error.errno == errno.ENOSPC, error.errno)
    target.unlink()
    check("space_back", os.statvfs("/workspace").f_bavail > 0)


def hold():
    # Keeps the VM running so the host can inspect the VMM process (SBX-F-003); the host destroys the sandbox.
    import time
    print("tqa-probe: holding", flush=True)
    time.sleep(300)


def busy():
    # SBX-C-009: CPU work bounded by count, then 128 MiB touched page by page and held while the host samples the
    # VMM's usage; the host destroys the sandbox.
    import time
    total = 0
    for value in range(60_000_000):
        total += value
    held = bytearray(128 << 20)
    for offset in range(0, len(held), 4096):
        held[offset] = 1
    print("tqa-probe: busy-held " + str(total % 7), flush=True)
    time.sleep(300)


SUITES = {
    "posture": [privileges, processes, filesystem, token, storage, names, durability, listening],
    "persist-write": [persist_write],
    "persist-read": [persist_read],
    "limits": [limits],
    "clock": [clock],
    "fill": [fill],
    "egress": [egress],
    "hold": [hold],
    "busy": [busy],
    "flush": [flush_cost],
}


def main():
    suite = os.environ.get("PROBE_CHECK", "posture")
    for step in SUITES.get(suite, []):
        try:
            step()
        except Exception as error:  # A crashed step is a failed check, never a pass.
            FAILURES.append({"check": step.__name__, "detail": repr(error)[:200]})
    if suite not in SUITES:
        FAILURES.append({"check": "suite", "detail": suite})
    print("tqa-probe: " + json.dumps({"suite": suite, "failures": FAILURES}), flush=True)
    sys.exit(1 if FAILURES else 0)


main()
