# ADR-0006: Firecracker backend: per-VM rootless confinement, rootfs pipeline, pinned kernel and VMM

## Status

Accepted (2026-09-26), on the condition stated under Consequences

## Date

2026-09-26

## Context

Phase 8 adds microVM isolation behind the same `SandboxBackend` trait and `SandboxSpec` as the Docker backend, and
it must pass the same SBX-C and SEC suites (FR-13, M7). ARCHITECTURE §4.3 assumed the Firecracker jailer. The jailer
must be started as root: it creates device nodes in its chroot, enters cgroups and changes uid. An unprivileged
gateway would therefore need a permanent root helper, reachable through a host socket, plus a host bridge, host
nftables rules and `ip_forward`. That is a large standing root surface on every install.

The reference deployment already runs a rootless container engine for the Docker backend. Measured on such a host:
- a rootless container with a namespaced `CAP_NET_ADMIN` can create a tap device and load an nftables ruleset in
  its own network namespace;
- `mknod` fails there, so the jailer cannot run inside it;
- unprivileged user namespaces may be blocked by the host security module, so the backend cannot create namespaces
  itself.

## Decision

1. **One VM per sandbox, each inside its own container** created through the same engine API, network and labels as
   a Docker-mode sandbox. The container runs from a pinned `tqa-vmm` image (`images/vmm/`) with:
   - a read-only root and a private tmpfs;
   - all capabilities dropped except the namespaced `CAP_NET_ADMIN`, `CAP_SETUID`, `CAP_SETGID` and `CAP_SETPCAP`, which
     only the launcher uses (to create the tap, load the rules and become the VMM uid);
   - `no-new-privileges`, the engine's default seccomp profile, and cgroup limits for CPU, memory and pids;
   - only this VM's files mounted: rootfs, kernel, workspace image, state image and the config device.
2. **Firecracker never runs as the container's root.** The launcher sets up the tap and the rules, then execs
   Firecracker as the fixed container uid 10000 with an empty bounding set and `no_new_privs`. Firecracker also
   installs its own seccomp filters. Under a rootless engine that uid maps to a subordinate host uid that owns
   nothing on the host. The per-VM files it opens stay owned by the gateway, which adds one POSIX ACL entry (with a
   matching mask) for that uid on each of them: read-only for the configuration device, read-write for the workspace
   and state images. The storage root must support ACLs, and the preflight checks it.
   **The launcher's window, stated plainly:** from container start until it execs Firecracker, the launcher runs as the
   container's namespaced root, which is the service account on the host under a rootless engine. It runs only a
   fixed script from the read-only image, takes no input from the guest (the VM is not started yet, and afterwards
   the per-VM rules drop everything the guest sends to the container itself), and ends by becoming the VMM uid with
   no capabilities.
3. **The only host root action** grants `/dev/kvm` read-write to that subordinate uid and its matching gid,
   persistently, through a oneshot systemd unit ordered after logind plus a timer that re-applies it, because logind
   resets the device's ACL at seat setup (`deploy/firecracker/host-setup.sh --apply`). The group entry is what holds:
   logind's reset removes user entries only, on every change event of the device (amended 2026-09-28). `--check` needs no root, and `--revert`
   removes the grant. There are no host taps, bridges, nftables rules, sysctls or root services.
4. **Network (ADR-0021 unchanged):**
   - the guest has a /30 link on a tap inside the container's network namespace;
   - nftables there allows only the gateway's address and ports, drops everything else (UDP and DNS included), and
     masquerades the guest behind the container's address on the internal network;
   - the internal network remains a second layer with no route out;
   - the guest has no resolver: init writes a hosts file with the gateway entry only.
5. **Devices:**
   - the rootfs, read-only;
   - the workspace ext4 image, read-write, sized to the quota;
   - the state ext4 image, read-write;
   - the workspace and state images with `cache_type: "Writeback"` (amended 6 Oct 2026, the technical lead, after
     PERF-012's microVM half): the guest's flush becomes an `fsync` of the image on the host, so a task's `fsync`
     survives a host crash as on the Docker backend's bind mount. Firecracker's default, `Unsafe`, offers the guest no
     flush, which risks the guest filesystem as well as the last writes. The read-only rootfs and config device, and the
     exit record (read while the host is up), keep the default. The engine stays `io_engine: Sync`; `Async` (a
     developer preview in v1.17.0) is only an experiment for now (SBX-F-039);
   - a read-only config device (one JSON document of at most 64 KiB, zero-padded) with the runtime token and non-secret settings.
   The token never appears on the kernel command line, in the VMM configuration, in the container's environment or
   labels, or in inspect output (ADR-0018). The guest init copies it to a tmpfs file readable only by the sandbox
   user.
6. **Guest init** is a new crate, `tqa-vm-init`: a small static binary with no workspace dependencies, installed
   into the rootfs as PID 1. Before it execs the unchanged runtime with the same paths and variables as Docker mode,
   it:
   - mounts the devices;
   - applies the pids limit in a guest cgroup;
   - enters a private PID namespace;
   - drops to uid 1000 with no capabilities and `no_new_privs`;
   - installs a seccomp deny-list equivalent to the Docker default profile.
   The runtime does not filter itself, so Docker mode is never double-filtered.
7. **Rootfs pipeline, OCI image to ext4:**
   - it runs without host root, inside a pinned builder container, so image ownership is preserved;
   - it adds only the guest init and the image's process configuration (user, environment, working directory,
     entrypoint), which an exported filesystem does not carry;
   - `mkfs.ext4 -d` runs with a fixed UUID, hash seed and time;
   - a digest manifest records the source image identity, the protocol-revision label, the rootfs and its content
     tree digest, the init and runtime binary hashes, and the builder; the kernel build writes its own manifest;
   - the backend refuses a rootfs or kernel whose hash differs from its manifest.
8. **Pins:**
   - Firecracker v1.17.0 (x86_64 release archive, Apache-2.0), the first release with official support for 6.18
     guest kernels. The image build verifies the archive against the release's published checksum file, and both
     hashes are recorded in the VM host image and the kernel manifest;
   - the guest kernel is Linux 6.18.54 longterm, built locally from the kernel.org archive (verified by SHA-256) with
     Firecracker's 6.18 guest configuration plus a committed fragment;
   - the kernel is GPL-2.0-only, and is an aggregated artefact that is never linked;
   - whether a release ships a kernel binary is a Product Owner decision; until then it is built locally and not
     distributed.
9. **Snapshots** (SBX-F-008, PERF-011 restore) remain a stretch goal. They need the VMM API and a fresh token after
   restore.

## Consequences

- **Escape boundary, stated plainly:**
  - code that escapes the guest through KVM or Firecracker runs as an unprivileged subordinate host uid;
  - it has no capabilities, seccomp-filtered, in a private mount namespace on a read-only root, where only this
    VM's own images are visible;
  - it cannot read other workspaces, the gateway's files or the engine socket, because they are neither mounted
    nor owned by that uid;
  - it can reach the gateway's address on the internal network, as the guest already can;
  - this is at least the Docker-mode boundary with KVM in front of it, and it matches the jailer's model
    (dedicated uid, chroot, cgroup, seccomp) without a root launcher.
- A container engine is required on Firecracker hosts, and VM teardown is container removal. Taps and rules vanish
  with the network namespace, so a host reboot leaves nothing to clean except exited containers, which
  reconciliation removes.
- `stats` reports the host view: vCPU time and the resident guest memory. Guest process counts are not visible from
  the host.
- An idle VM reserves its memory until destroyed, so idle windows are shorter (ARCHITECTURE §4.4).
- If the gateway's address changes while VMs are running, their rules go stale and the dead-agent path recreates
  them. Deployments may pin the gateway's address.
- Known risk: host kernels outside Firecracker's tested host matrix (5.10, 6.1, 6.18) are supported on a best-effort
  basis. `host-setup.sh --check` warns, and PERF results record the host kernel.
- Known risk: Firecracker v1.17.0 has no patch release yet. The pin moves to v1.17.x when one ships; v1.16.x does
  not support 6.18 guest kernels.
- ARCHITECTURE §4.3 follows this record. SBX-F-003 checks the VMM posture: it runs as a non-root uid distinct from
  the service account, in a private mount namespace on a read-only root, with no capabilities, no_new_privs,
  seccomp and cgroup limits.
- **Conditional acceptance.** The Product Owner accepted the rootless per-VM design on condition that it works. If
  KVM access, networking, performance (PERF-011) or the SBX-C and SEC suites fail on this backend, the evidence
  goes back to the Product Owner, and the jailer with a root launcher (the first rejected alternative) is
  reconsidered.
- The dependency-direction table gains `tqa-vm-init` with no workspace dependencies.

Alternatives rejected:
- the jailer with a root helper: a permanent privileged API on every host;
- the jailer inside the container: it cannot create device nodes there;
- Firecracker as the container's root: under a rootless engine that is the service account, which owns the engine
  socket and every workspace;
- the metadata service for the token: it is reachable by any guest process over the network, and the token is
  visible through the VMM API.

## Links

- PRD FR-13, M3, M7, NFR-1; ARCHITECTURE §4.3, §4.4, §4.5 and §12; DEV-PLAN Phase 8
- ADR-0018, ADR-0021; TEST-PLAN §11.3 SBX-F-001…015, SBX-C-005, SBX-C-007, PERF-011
