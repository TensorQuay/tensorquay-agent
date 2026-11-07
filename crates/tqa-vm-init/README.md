# tqa-vm-init

The guest init for microVM sandboxes (ADR-0006). The rootfs pipeline installs it as `/usr/sbin/tqa-vm-init`, and it
runs as the guest's PID 1. It has no workspace dependencies, and no crate depends on it (G-07). The crate forbids
`unsafe`: every system call goes through `nix`, `caps` or `seccompiler`.

**Inputs**, both parsed once into validated types with bounded, static error messages that never echo content:
- `/etc/tqa-vm/image.json`: the image's user, environment, working directory, entrypoint and command. The rootfs
  pipeline records it from the OCI image. The user must be a numeric, non-root `uid:gid`.
- `/dev/vdd`: the per-sandbox configuration device written by the host backend. It holds one JSON document padded
  with zero bytes, of at most 64 KiB. The document carries the per-run token, the runtime environment, the static
  host entries and the pids limit. The device is read once and never mounted. Only root can read it in the guest.

**Stage one** (PID 1, privileged) does the following in order:
1. mounts `/proc`, `/sys`, `/dev`, cgroup2, and tmpfs on `/tmp` and `/run`;
2. mounts the workspace (`/dev/vdb`) at `/workspace` and the state image (`/dev/vdc`) at `/run/tqa/state`, both
   `nosuid,nodev`;
3. writes the token to `/run/tqa/token` (mode 0400, owned by the sandbox user);
4. binds a hosts file holding only the given entries, and an empty resolver file, over the read-only root;
5. disables user namespaces and restricts kernel pointers and the kernel log;
6. creates the sandbox cgroup with `pids.max`. Out-of-memory kills stay per process;
7. starts stage two in a new PID namespace. The environment is the image's, then the host's, then the
   init-owned `TQA_RUNTIME_TOKEN_FILE` and `TQA_STATE_DIR`.

When stage two ends, stage one prints one console line, syncs, detaches the storage and resets the VM, and the VMM
exits.

**Stage two** (PID 1 of the sandbox's PID namespace) does the following:
1. mounts a private `/proc`, so only sandbox processes are visible;
2. joins the sandbox cgroup;
3. clears the ambient, inheritable and bounding capability sets;
4. sets the groups, gid and uid, and verifies that no effective or permitted capability survives;
5. sets `no_new_privs` and installs the seccomp deny-list;
6. executes the image's entrypoint.

The deny-list refuses, with `EPERM`, what the Docker default profile refuses to an unprivileged container: mounts and
the new mount API, namespaces, BPF, keyrings, module and kexec loading, `perf_event_open`, `io_uring`, clock setting,
swap, and so on. It also refuses `personality` values outside the permitted set. `ptrace`, `clone` and `exec` stay
available.

Tests: `cargo nextest run -p tqa-vm-init` (SBX-F-016). The mount, privilege and boot behaviour is verified inside a
real VM by SBX-C-005…007 and SBX-C-016 on Firecracker.
