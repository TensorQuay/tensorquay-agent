# Firecracker backend: administrator guide

The Firecracker backend runs each conversation's sandbox as a microVM (ADR-0006). Each VM runs inside its own
container on the same rootless container engine that the Docker backend uses. Firecracker runs there as a fixed
unprivileged uid, and the VM's tap device and network rules live in that container's own network namespace. The
backend passes the same sandbox contract as Docker mode (TEST-PLAN §11). This guide covers a Linux host with KVM.

## 1. Requirements

- An x86_64 Linux host with hardware virtualisation (VT-x or AMD-V) and the `kvm` and `tun` modules.
- A **rootless** Docker engine for the service account, with the `cpu`, `memory` and `pids` cgroup controllers
  delegated to that account, and a subordinate uid range of at least 10000 ids (`/etc/subuid`).
- A storage root on a filesystem with POSIX ACLs (ext4, xfs or btrfs). Every per-VM file stays owned by the gateway,
  which grants the VMM access through one ACL entry per file.
- One sandbox backend per install. A gateway runs either Docker or Firecracker sandboxes, never both on one sandbox
  network. Both backends label their containers with the same instance, so a Docker backend's reconciliation would
  treat Firecracker VM hosts as orphans. A conversation never changes backend.
- Host kernels 5.10, 6.1 and 6.18 are the ones Firecracker tests. Others work on a best-effort basis, and `--check` warns.

## 2. Host setup: the one root action

Firecracker runs as container uid and gid 10000. Under a rootless engine they map to a subordinate host uid and gid:
each subordinate range's start plus 9999. That uid and gid, and only they, need `/dev/kvm`. `host-setup.sh` grants
both (`setfacl -m u:<uid>:rw,g:<gid>:rw`). The group entry is what holds: logind's uaccess reset deletes every *user*
ACL entry of the device on each change event, and never touches group entries. With only the user entry, a VM that
started between a reset and the re-grant failed with `Permission denied` from KVM (seen on 2026-09-28 as one of 16
density VMs exiting within 120 ms).

| Command | Root | Effect |
|---|---|---|
| `host-setup.sh --check [--user NAME]` | no | Checks the host and prints one line per item. Each missing item comes with its remedy. Exit status: 0 when ready, 1 when something is missing, 2 on usage errors |
| `sudo host-setup.sh --apply --user NAME` | yes | Installs and enables `tqa-kvm-acl.service` and `tqa-kvm-acl.timer` in `/etc/systemd/system`, applies the grant now, removes a legacy `/etc/tmpfiles.d/tqa-kvm.conf`, then runs `--check`. Idempotent; rewrites units that differ |
| `sudo host-setup.sh --revert --user NAME` | yes | Disables and removes both units and the legacy file, and removes both ACL entries |

`NAME` is the account that runs the rootless engine. `--check` covers these items:
- hardware virtualisation;
- `/dev/kvm` and `/dev/net/tun`;
- the subordinate uid range;
- the live ACL;
- both units, with exactly the content `--apply` writes, and the timer, which must be enabled (and active, on a real host);
- the delegated cgroup controllers.

It warns without failing on an untested host kernel and on a legacy tmpfiles grant.

**Why a unit and a timer.** `/dev/kvm` carries logind's `uaccess` tag, so logind rewrites the device's ACL for the
seat user whenever the seat's sessions change: at boot, and each time a desktop or greeter session opens or closes. That
discards any other grant, including a tmpfiles or udev one. The oneshot unit is ordered after logind and the display
manager, and the timer re-applies the grant one minute after boot and every 30 seconds after that. After a session
change, VMs therefore cannot open `/dev/kvm` for up to 30 seconds. A sandbox started in that window fails, and the
startup preflight refuses and names the missing access. Nothing degrades silently.

On a host where desktop sessions change often, the window matters. Removing the `uaccess` tag from `/dev/kvm` ends it,
but it also changes the desktop user's own KVM access, so it is the host owner's decision. `kvm-no-uaccess.sh` (`--check`, `--apply`,
`--revert`) installs that change once the owner chooses it. Removing the tag is not enough on its own: udev tags are
sticky, and on systemd 255 the seat's `TAG=="uaccess"` rule still matches them, so its ACL reset runs on every `change`
event of `/dev/kvm`, which KVM sends for each VM created or destroyed. The script therefore installs two rules:
`72-tqa-kvm-no-uaccess.rules` removes the current tag, and `74-tqa-kvm-acl.rules` runs the grant from
`tqa-kvm-acl.service` in the same event, after the reset. Run `host-setup.sh --apply` first, then this script again
whenever the unit's grant changes (its rule copies the grant). With the group grant, the re-grant and the timer are
safety nets only. `--check` verifies both rule files, reads the device's `CURRENT_TAGS`, and uses `udevadm test` (which runs nothing)
to confirm udev schedules the grant after the reset. The Firecracker KVM test
suites retry their preflight once after 35 seconds for the same reason.

## 3. Artefacts: the VM host image, the guest kernel and one rootfs per template

Build these once per release, from the repository root, as the service account (no root):

```sh
# The VM host image: Firecracker (release checksum verified), the guest init, the launcher and the rootfs builder.
docker build -f images/vmm/Dockerfile --target vmm -t tqa-vmm .
docker image inspect -f '{{.Id}}' tqa-vmm          # the ID for [firecracker] vmm_image

# The pinned guest kernel into <storage.root>/kernel (vmlinux plus manifest.json).
docker build -f images/rootfs/kernel/Dockerfile --target kernel \
  --output type=local,dest=<storage.root>/kernel images/rootfs/kernel

# One rootfs per template image, into <storage.root>/images/<image id>/.
python3 images/rootfs/build_rootfs.py --image <template image ID> --vmm-image tqa-vmm --out <storage.root>/images
```

Rebuild a template's rootfs whenever the template image or the VM host image changes, because the rootfs carries the
guest init. **A VM host image upgrade therefore means a rootfs rebuild for every template:** a stale guest init would
silently lack newer guarantees (for example the workspace write-back and the sandbox memory cap). The backend enforces
this when `[firecracker] vmm_image` is pinned by its image ID, as recommended. A rootfs recorded as built by another VM
host image is refused at create and reported as `outdated` in the template status. A registry-digest pin isn't compared,
because it need not be the ID the builder recorded. The builder refuses to overwrite an existing rootfs, so remove the
old `<image id>/` directory first.
A template without a verified rootfs cannot start, and the readiness check reports `image_present: false`.

## 4. Gateway configuration

Run the gateway in a container on the same rootless engine. That is the supported topology: every container there
shares one uid mapping, so helper containers create files the gateway owns, and container uid 10000 means the same
account for the gateway and the VMM. The gateway must have a **pinned address** on the internal sandbox network,
because every VM's rules allow exactly that address. A compose override:

```yaml
services:
  gateway:
    networks:
      sandboxes:
        ipv4_address: 198.51.100.10        # example: use a private subnet of your choice
networks:
  sandboxes:
    ipam:
      config:
        - subnet: 198.51.100.0/24
```

In `gateway.toml`:

```toml
[runtime]
backend = "firecracker"
image = "sha256:<the runtime template image ID>"

[sandbox]
network = "tqa-sandboxes"
gateway_url = "http://gateway:8080"      # a host name: a guest resolves it only through its static hosts entry

[firecracker]
vmm_image = "sha256:<the tqa-vmm image ID>"
gateway_address = "198.51.100.10"         # the pinned address above
```

The gateway README ("Configuration") lists every `[firecracker]` setting and its default. `vmm_file_uid` stays at its
default of 10000 in the supported topology. A gateway run directly on the host sees the VMM as the subordinate range
start plus 9999 and must set that number. The backend also refuses to start in that case, because its storage helpers
would create files it does not own.

## 5. Start and verify

The gateway refuses to start when any requirement is missing, and it names the missing one:

| Reason in the startup error | What to do |
|---|---|
| the VM host image is not present (build images/vmm) | build it and set `[firecracker] vmm_image` to its ID |
| the sandbox network does not exist … / is not internal … | start the gateway on its internal sandbox network first |
| /dev/kvm does not open for the VMM uid … | run `host-setup.sh --check`; wait out the logind window, or apply the setup |
| /dev/net/tun is not available … | load the `tun` module |
| storage.root does not support POSIX ACLs | move the storage root to ext4, xfs or btrfs |
| the guest kernel does not match its manifest / … manifest is missing or malformed | rebuild the kernel into `<storage.root>/kernel` |
| storage helpers cannot create files this process owns … | run the gateway in a container on the same engine |
| the rootfs was built by an older builder; rebuild it (images/rootfs) | rebuild the template's rootfs (schema 2 records the image policy) |
| the rootfs was built by another VM host image; rebuild it with images/rootfs/build_rootfs.py | the VM host image changed: rebuild every template's rootfs with the new one as `--vmm-image` |

After start, an administrator's `GET /readyz?detail=1` reports `sandbox.backend: "firecracker"`, `backend_preflight`,
and `image_present`. The last one is true only when the runtime template's rootfs exists, matches its manifest and
meets the protocol floor. Creating a sandbox applies Docker's image policy as well: an image that declares volumes, or
sets an environment key that `[sandbox.env]` does not set, is refused, so set every key the image uses in
`[sandbox.env]` (as on Docker).

## 6. Operations

- **Storage:** a workspace is `<storage.root>/workspaces/<id>.ext4`, and a conversation's runtime state is
  `<storage.root>/runtime-state/<id>.ext4`. The backend creates both for the first sandbox. The gateway never mounts or
  reads them, and deleting a workspace deletes its image.
- **VM host memory:** each VM host's memory limit is the sandbox's memory plus `[firecracker] vmm_overhead_bytes`
  (default 512 MiB). The overhead holds the VMM and, above all, the host page cache the guest's disk writes pass
  through, which the same cgroup is charged for. While a guest writes fast, a few hundred MiB of that cache can be dirty;
  with too little overhead the VM host sits at its limit with nothing clean to reclaim, and when writeback lags the
  VMM is OOM-killed mid-write (the task then sees its sandbox stop). Measured during a workspace fill: 64 MiB left no
  clean cache, 256 MiB left 17 MiB, 512 MiB left 228 MiB. The limit is a bound, not a reservation: the cache is
  reclaimable host memory.
- **Workspace size:** a new workspace image (sparse) is `[workspaces] quota_bytes` plus room for ext4's own metadata,
  5% of the quota and at least 128 MiB, made without a root reserve. The space the sandbox can write is at least the
  quota: a 1 GiB quota makes a 1152 MiB image, 10 GiB makes 10752 MiB. Writing past it fails inside the sandbox with
  "no space left" and a `workspace.full` event; that is the hard bound on this backend. Between runs the gateway
  charges a workspace the image's allocated blocks, which include ext4's journal and tables, so a workspace can count
  a few tens of MiB more than its files and be refused new runs slightly before its files reach the quota.
- **Upgrading from `[firecracker] workspace_bytes`:** the setting is deprecated. It is still checked (64 MiB to 2 TiB),
  a start-up warning says it is ignored, and you can remove it. New images follow the quota; existing images keep the
  size they were made with. An existing image smaller than the quota fills up (and reports `workspace.full`) before the
  quota is reached; one larger than the quota is still held to the quota between runs.
- **Forks** copy the workspace image as a file: a reflink where the filesystem offers one, else a sparse copy. The
  image is never mounted. A fork taken while the source's VM runs is marked inconsistent, as on Docker.
- **Not yet available on this backend:** browsing workspace files, uploads and run changes. They answer
  `409 files_unavailable` until the runtime's workspace service exists (ADR-0030).
- **Memory, per VM:** the guest gets the sandbox's memory limit as its RAM (`[sandbox]` memory, or the environment's
  own). The VM host container may use that plus `vmm_overhead_bytes` (512 MiB by default, see **VM host memory**
  above), and the host kills the whole VM beyond it. Inside the guest, the sandbox (the runtime and every tool) is capped at the guest's memory
  less a 64 MiB reserve for the guest kernel, the init and the page cache. So when a tool exhausts memory, the guest's own
  OOM killer ends it, and the VM and the run continue. Tool processes run at a higher OOM score than the runtime,
  so the killer takes the tool, never the runtime (SBX-C-003). Budget each VM as its memory limit + `vmm_overhead_bytes` (512 MiB by default) on the host.
- **Host reboot:** VM hosts have no restart policy. On start, the gateway's reconciliation finds them exited, destroys
  them and marks their runs interrupted and resumable. Per-sandbox directories whose container is gone are swept, so no
  token outlives its VM. No taps, bridges or rules exist on the host network to clean.
- **Logs:** a VM host container's log is the guest console. It is a diagnostic only and is not forwarded to the gateway's logs.
- **Security boundary:** code that escapes a guest runs as the VMM's subordinate uid, with no capabilities, seccomp, a
  read-only root and only that VM's files visible (ADR-0006, "Escape boundary").

## 7. Tests

`python3 -m unittest deploy/firecracker/test_host_setup.py` (SBX-F-012) runs `--check` against fake host roots and
needs no root. The KVM suites are in `crates/tqa-sandbox/tests/firecracker_*.rs`; their requirements are listed in
`crates/tqa-sandbox/tests/firecracker/support.rs`.
`deploy/firecracker/stack.py smoke` (SBX-F-020) runs the whole product on this backend from the repository root: the
Compose stack with `deploy/compose/firecracker.yml`, the fake model, and one task in a microVM; it tears everything
down afterwards. `stack.py oom` (SBX-C-003), `contract.py egress` (SEC-001/SEC-006), `contract.py escape` (SEC-002), `contract.py full` (SBX-C-015), `contract.py destroy` (SBX-C-013),
`contract.py reconcile` (SBX-C-008) and `contract.py reboot` (SBX-F-007) run the remaining contract cases on the same stack. `tools/check.sh --kvm` runs all of these (G-13). It refuses to start unless `TQA_VMM_IMAGE`, `TQA_FC_KERNEL_DIR`,
`TQA_FC_ROOTFS_DIR` and the images under test are named: `TQA_GATEWAY_IMAGE`, `TQA_WEB_IMAGE`, `TQA_RUNTIME_IMAGE`,
`TQA_WORKSTATION_IMAGE` and `TQA_FAKE_MODEL_IMAGE` (a candidate's tag file names them all). A stack never falls back to
a bare tag, since the engine is shared.

`deploy/firecracker/start_repro.py` reproduces a rare VM host crash at start (exit 139 before the runtime registers;
`docs/evals/gate-8.md`): `product` starts a new task's VM about 500 times through the product, `bare` runs a clone
of one VM host about 500 times without the gateway. Each counts every start's end instead of stopping at the first
failure, and writes `summary.json` and the VM hosts' logs to `--out`. It needs a quiet KVM host for about an hour.
