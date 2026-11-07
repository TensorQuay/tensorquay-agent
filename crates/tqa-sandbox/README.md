# tqa-sandbox

`SandboxBackend` is the gateway's disposable execution boundary: create, read its network address, wait for runtime registration,
sample resources, destroy, and list. `reconcile` compares known and live handles without I/O. This crate
implements Docker; `BackendKind::Firecracker` reserves the future backend identity. The only production
workspace dependency is `tqa-types`. There is no gateway, runtime image, write lease, reaper, or policy proxy here.

## Contract

`wait_ready` checks runtime liveness throughout registration, including while the gateway's readiness lookup is
pending. A stopped runtime returns `RuntimeExited { code }` at the next bounded state observation (100 ms polling
by default); its diagnostic contains only the signed exit code. Missing containers still return `NotFound`, and
live unregistered containers still time out. `FakeSandboxBackend::exit` scripts the same failure without removing
the handle from reconciliation; destroy still cleans it up.

`SandboxSpec` binds a conversation and workspace to a mandatory SHA-256 image digest, CPU millicores,
positive memory bytes, a process limit, explicit egress policy, runtime endpoint, short-lived runtime
credential, non-secret environment, and TTL. The gateway owns authorization, workspace provisioning,
the write lease, and expiry. Docker rejects block images and nonempty `disk` requests; workspace quotas
must be provisioned separately and are not silently claimed as enforced.

Host directories must be absolute, non-root directories without parent traversal. Creation resolves
symlinks and revalidates the canonical directory. The gateway must own the directory and its ancestors
so another host process cannot replace them between validation and the daemon's mount operation. Bind
paths refer to the daemon host; local validation assumes a shared filesystem, as on a local Engine or
Desktop. Named volumes must already exist and be writable by uid/gid 1000. Docker cannot add other
image-declared volumes: images declaring them are rejected.

The runtime credential uses `SecretString`; specification Debug and all backend errors redact it.
Creation writes it to a new 0600 file at `storage_root/sandboxes/<sandbox_id>/token`, inside private
0700 directories. The sandbox identifier is its unique Docker name; the returned handle contains the
Engine's container ID. The file is mounted read-only at `/run/tqa/token`; only
`TQA_RUNTIME_TOKEN_FILE=/run/tqa/token` appears in the environment. The create body, labels, complete
inspect output and transport debug logs contain no credential bytes. Destroy removes the host file
and its directory; cleanup failures produce a warning without exposing the value. The token ID label
allows cleanup after a backend reconnect. Expiry and cleanup after externally removed containers
remain the gateway reaper's responsibility.

`storage_root` must already exist on the shared daemon filesystem. Workspaces cannot overlap its
`sandboxes` subtree. On native Linux, the backend assigns token-file ownership to uid 1000; it requires
that uid or permission to change ownership, and fails closed if assignment fails. The Linux CI job
uses root on its disposable runner to establish ownership; sandbox containers still run as uid 1000.
On a rootless daemon the containerised gateway (uid 1000) owns the file it creates, which is the sandbox's uid 1000.
Host-run callers cannot assign that subordinate ID and fail closed. The live suites run through
`tools/rootless-test.sh`, which enters the daemon's user namespace and refuses a rootful daemon; the same tests
pass there unchanged. Remapped (`userns-remap`) daemons have not been verified.
The observed Desktop installation presents the private bind file as uid 1000, verified by the live
readability test. No host file mode is relaxed to make credential delivery work.

Environment names containing `key`, `token`, `secret`, `password`, or `credential` are rejected
case-insensitively, as are runtime and proxy reserved names. Each inherited image environment key must
be explicitly supplied in `spec.env`. Docker also provides standard process defaults (`PATH`, `HOME`,
`HOSTNAME`); supply these explicitly when asserting an exact environment, as the tests do. Arbitrary
environment values must already be non-secret; key validation cannot recognize every credential value.

The `log` dependency enables `release_max_level_info`: release builds compile out debug and trace
messages from log-based dependencies. Debug builds retain full diagnostics and have a regression test
capturing Bollard at trace level. The gateway will additionally pin Bollard to `warn` in its subscriber;
that subscriber belongs to a later gateway task. Native tracing events remain available.

Limits are explicit: 64 environment entries, 128-byte names, 8192-byte values and runtime credential,
256 exact host rules, 2048-byte endpoints, 65,536 processes, and a positive TTL of at most one year.
`DockerBackendConfig` rejects unknown serialized fields and validates all bounds when connecting.
Defaults are a 30-second operation timeout, two-minute image pull timeout, 100-millisecond readiness
poll interval, and 4096 listed containers. One admitted creation at a time avoids racing network setup;
waiting admission is bounded by the pull timeout and observes cancellation.

Readiness checks both container liveness and the gateway's `ReadinessSource`, with cancellation and a
caller deadline covering even an unresponsive registration source. Read-only I/O is cancellable;
create/start mutations settle within their operation timeout before cancellation is observed, allowing
cleanup to own their result. Failed starts remove the container, credential file and any newly created
empty network.
Timeout recovery inspects owned resources before removing them. Daemon unavailability can prevent
cleanup; the error is returned and gateway reconciliation must retry after connectivity returns.

Destroy stops with a ten-second grace period, then force-removes; a missing container is success. It
preserves workspace data and the shared gateway network. An instance owner removes that network during
its own shutdown, after detaching the gateway. Listing and handle operations are scoped by
`tqa.instance` and conversation labels. Errors expose stable `sandbox.*` codes; only backend and timeout
errors are retryable. `FakeSandboxBackend` provides the same lifecycle boundary in memory, with an
explicit creation time, no stored specifications or credentials, and zero resource samples.

`address(handle)` verifies ownership and returns a running sandbox's address on the configured internal network.
The gateway uses that observed address when activating its reserved runtime token after creation. Missing, stopped,
foreign or unaddressed containers fail closed; the fake returns loopback for an existing owned handle.
`DockerBackend::image_present` checks a pinned image locally without pulling it, for administrator readiness reports.
`DockerBackend::check_runtime_image` is the runtime-image preflight. It runs the same inspection, without a pull,
and reads the `org.tensorquay.protocol-revision` label. An absent image fails `sandbox.image_not_found`. An unlabelled
image counts as revision 1. An image below `MIN_RUNTIME_REVISION` fails `sandbox.runtime_too_old`, carrying only the
needed and actual revisions, never the image name. A malformed label fails as an invalid image (ADR-0026).

## Hardening and egress

The pure `container_config(spec, network_name, created_at, token_file)` function constructs the posture. It uses
uid/gid 1000, a read-only root, a 16 MiB `nosuid,nodev,noexec` temporary filesystem, exactly one writable
workspace mount plus the read-only credential-file bind, dropped capabilities, no-new-privileges, explicit `seccomp=builtin`, CPU bandwidth,
memory with no extra swap allowance, and a process limit. It forbids host namespaces, privileged mode,
added devices and published ports, and sets restart policy to `no`. Labels contain only reconciliation
metadata; image selection and the digest label use the mandatory digest even when a tag moves.

Each instance owns one IPv4 bridge named by `network_prefix`. Existing ownership, bridge driver,
IPv6 setting, and network mode must match before reuse. The default `NetworkMode::Internal` has no
external route, regardless of egress policy. The gateway must join it; gateway URLs stay unchanged.

- `EgressMode::Internal` supplies no proxy settings.
- `EgressMode::Proxy` requires a separate credential-free HTTP(S) `proxy_url` and injects `TQA_EGRESS_PROXY_URL`,
  `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY`/`no_proxy` for the gateway hostname and loopback (`localhost`,
  `127.0.0.1`, `::1`, SBX-U-018: a task reaches its own local servers). The runtime validates the explicit
  proxy origin against its gateway host and configures shell children and `fetch_url` from that input.
  The gateway proxy enforces its host and port allow list. Direct connections remain confined by the
  internal network even when an application ignores the proxy variables.

Preflight reports routing isolation for both egress policies on the default internal network.
The explicit development-only `NetworkMode::DevHostGateway` uses an externally routed bridge;
preflight advertises no enforceable egress policy in that mode.

Preflight reports the daemon API version, Linux Engine versus Desktop, bridge driver availability,
whether user namespace remapping is enabled, and the advertised daemon seccomp default. It refuses
non-Linux engines and missing bridge/seccomp support. It does not claim that a configured custom or
unconfined daemon default is protective: every sandbox requests the built-in profile explicitly.

| Capability | Linux Engine documentation | Verified on macOS Docker Desktop |
| --- | --- | --- |
| Container filesystem and privileges | Docker exposes read-only root, uid, capabilities and no-new-privileges settings | Root writes fail; workspace and temporary writes succeed; uid 1000, empty capability sets, no-new-privileges, and denied mount syscall verified |
| Seccomp | Built-in profile filters syscalls; explicit `builtin` overrides an unconfined daemon default | Daemon default was unconfined; explicit profile produced `Seccomp: 2` in the container |
| Resource limits | Linux cgroups implement memory, process and CPU bandwidth controls | Live inspect matched all limits; an allocating child was OOM-killed while the sandbox survived; CPU throughput was not benchmarked |
| Internal bridge | An internal bridge isolates external routing | No default route; outbound public connection failed |
| User namespaces | Remapping is a separate daemon configuration | Remapping was not enabled; this crate does not change daemon settings |

The observed host ran Docker Desktop 4.37.2, Engine 27.4.0, API 1.47, with Linux arm64 containers.
Desktop runs its Engine inside a Linux VM and routes network and shared-file access through host/VM
components. These checks establish the container posture on that installation, not native-host kernel
isolation. Native Linux behavior is documented and covered by the Linux CI workflow; it was not locally
measured during the macOS verification.

Sources: [Docker run security and resource options](https://docs.docker.com/reference/cli/docker/container/run/),
[seccomp](https://docs.docker.com/engine/security/seccomp/),
[bridge networking](https://docs.docker.com/engine/network/drivers/bridge/),
[user namespaces](https://docs.docker.com/engine/security/userns-remap/), and
[Desktop networking](https://docs.docker.com/desktop/features/networking/).

## Firecracker backend

`tqa_sandbox::firecracker::FirecrackerBackend` implements `SandboxBackend` with one microVM per sandbox (ADR-0006).
Each VM runs inside its own container on the same (rootless-capable) Docker engine and internal network as Docker
mode, from the pinned VM host image (`images/vmm`). The container starts as its namespaced root only so the launcher
can create the VM's tap and load the per-VM rules. The launcher then executes Firecracker as the fixed container uid
10000 (`VMM_CONTAINER_UID`) with no capabilities; that uid is never the service account. The guest cannot reach the
launcher.

- **Configuration:** `FirecrackerBackendConfig` holds:
  - the storage root, the VM host image digest, the kernel directory (`vmlinux` and `manifest.json`) and the rootfs
    directory (`<image id>/rootfs.ext4` and `manifest.json`);
  - the internal network, the gateway's pinned address and 1 to 8 gateway ports;
  - the VMM's file uid as this process sees it, the VMM memory overhead, the new state image size, the timeouts.

  `validate()` checks every field without I/O.
- **Connecting** runs the host preflight (SBX-F-001), which stops at the first missing requirement with a reason that
  names the remedy:
  1. the VM host image;
  2. `/dev/kvm` actually opening read-write for the VMM uid inside a short-lived VM host container. An access check is
     not enough, because a device passed into a rootless container looks readable without being openable;
  3. a creatable tap;
  4. ACL-capable storage;
  5. a guest kernel that matches its manifest.
- **Artefacts:** the kernel and rootfs manifests are parsed strictly and bounded, and every recorded digest must be well
  formed. Files are verified by SHA-256 with a cache keyed by file identity. A rootfs below the protocol floor fails
  with `sandbox.runtime_too_old`.
- **Storage:** workspaces and state are ext4 block images (`WorkspaceSource::BlockImage`, `StateSource::BlockImage`);
  directories are refused. A missing image is created by a helper container running as the storage root's owner.
  Its root directory is owned by the guest runtime's uid 1000, so the runtime can make its state directory private.
- **Per-VM files:** the gateway keeps owning every per-VM file and adds one POSIX ACL entry for the VMM uid, with a
  matching mask:
  - read-only on the configuration device;
  - read-write on the workspace and state images.

  Other uids get `EACCES`.
- **Token:** the per-run token reaches the guest only through the read-only configuration device. That is one JSON
  document for the guest init (`tqa-vm-init`, schema version 1), zero-padded to 4 KiB blocks and at most 64 KiB,
  stored as a 0600 file in a 0700 per-sandbox directory (SBX-F-011). It never appears on the kernel command line, in
  the VM description, or in the container's environment, labels or command. Destroy, and any failed create, remove
  the directory.
- **Launch** (SBX-F-013):
  - the VM description: fixed paths, the fixed kernel command line, vCPUs from the spec (at most 32) and memory (at
    least 256 MiB and a whole number of MiB, so the guest can keep its memory reserve beside the sandbox), with the rootfs and the configuration device read-only and the workspace and state images
    read-write;
  - the container request: a read-only root, only the namespaced capabilities the launcher needs, `no-new-privileges`,
    only `/dev/kvm` and `/dev/net/tun`, and memory set to the guest's plus the overhead;
  - the same labels as Docker mode plus `tqa.backend=firecracker`.
- **Network** (SBX-F-014): a /30 tap link inside the container's network namespace. The per-VM nftables rules let the
  guest open TCP connections to the gateway's address on the configured ports only. UDP, DNS, other addresses and the
  container itself are dropped, and the guest is masqueraded behind the container's address. The internal network
  remains the second layer (ADR-0021).
- **Lifecycle** (SBX-F-017):
  - readiness, address, stats and destroy act only on containers with this instance's labels;
  - destroy is idempotent and never touches a same-named container of another owner;
  - stats report the VMM container, which includes the guest's vCPUs and memory. Guest process counts are not visible
    from the host.
- **Host setup:** one root action, the `/dev/kvm` grant for the VMM uid; see `deploy/firecracker/README.md`.

Tests:
- `cargo nextest run -p tqa-sandbox -E 'test(sbx_f_)'` runs the unit tests and the backend on a fake engine.
- `--run-ignored all` adds the Docker and KVM checks. On a rootless daemon, run them through
  `tools/rootless-test.sh`. The checks are:
  - another uid gets `EACCES` on a granted file;
  - the ruleset loads with `nft -c` in the VM host image (set `TQA_VMM_IMAGE` to a built `images/vmm` tag);
  - the KVM guard fails with the preflight reason on an unprepared host (SBX-F-015);
  - on a prepared host, the real preflight passes (`TQA_VMM_IMAGE`, `TQA_FC_KERNEL_DIR`).

## Tests and image provenance

Run the configuration, validation, reconciliation, readiness, and fake-backend tests without Docker:

```sh
cargo nextest run -p tqa-sandbox
```

Integration tests are explicitly ignored in the default run. Opt in with both `--run-ignored all` and
`TQA_DOCKER_TESTS=1`; invoking an ignored test without the flag fails with an error. The client uses
`DOCKER_HOST` or its default socket, not the Docker CLI or CLI context discovery. For a Desktop context, select its endpoint
in the shell first; the backend itself continues to use only Bollard:

```sh
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
TQA_DOCKER_TESTS=1 cargo nextest run -p tqa-sandbox --run-ignored all -E 'not binary(runtime_image) and not binary(runtime_recovery) and not binary(manual_daemon) and not binary(egress) and not binary(runtime_posture)'
TQA_DOCKER_TESTS=1 cargo llvm-cov nextest -p tqa-sandbox --run-ignored all -E 'not binary(runtime_image) and not binary(runtime_recovery) and not binary(manual_daemon) and not binary(egress) and not binary(runtime_posture)'
```

The `egress` suite owns a gateway, PostgreSQL, scripted model, test upstream and runtime. It requires
an immutable `TQA_GATEWAY_IMAGE` digest containing the policy proxy and a freshly built Workstation
runtime. All credentials are generated fixture values. The deterministic cases use a local HTTP/TLS
upstream with a generated test CA, plus TCP/UDP positive controls outside the sandbox network. Those
assertions need no public upstream. The shared Compose bootstrap pulls its pinned helper image, so
Docker registry access is required even when the application images are already present.

```sh
TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE="$GATEWAY_IMAGE_DIGEST" TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test egress --run-ignored all --retries 0 -E 'not test(public_tcp)'
```

SBX-C-004 verifies the relay, a verified TLS request through `HTTPS_PROXY`, the upstream's observed
gateway peer, proxy 403/407, and blocked direct TCP/UDP to a controlled peer outside the sandbox
network. SEC-001 reads an injected README, tries shell and `fetch_url` exfiltration, and checks the
durable denied tool results beside the gateway's denial notice and log. It also checks that provider,
database and bootstrap credentials never enter the container environment, inspect data or transcript.

Public-IP evidence has a separate required opt-in. It first proves TCP/443 and DNS/UDP connectivity
from the upstream container, then requires both operations to fail from the sandbox to the same IPs.
DNS failure or an unavailable public service fails the positive control; it never counts as isolation.
The default TCP host is `example.com`, and the DNS host is `dns.quad9.net`. Administrators may choose
other reachable public hosts with `TQA_EGRESS_PUBLIC_TCP_HOST` and `TQA_EGRESS_PUBLIC_DNS_HOST`.

```sh
TQA_DOCKER_TESTS=1 TQA_PUBLIC_NETWORK_TESTS=1 TQA_GATEWAY_IMAGE="$GATEWAY_IMAGE_DIGEST" TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test egress --run-ignored all --retries 0 -E 'test(public_tcp)'
```

The runtime suites require built images. `runtime_image` checks admission on both Minimal and
Workstation; `runtime_recovery` uses Workstation for its Python task matrix. Its scripted gateway runs
on the host, so the driver explicitly selects the development host gateway network mode:

```sh
TQA_DOCKER_TESTS=1 TQA_RUNTIME_TEST_IMAGE=tqa-runtime:dev cargo nextest run -p tqa-sandbox --test runtime_image --run-ignored all --retries 0
TQA_DOCKER_TESTS=1 TQA_ALLOW_DEV_HOST_GATEWAY=1 TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test runtime_image --test runtime_recovery --test runtime_posture --run-ignored all --retries 0
```

SBX-C-008 verifies owned running and stopped containers, excludes another instance's labels, and
removes an orphan's credential directory while preserving bound containers. SBX-C-013 destroys an
actual runtime during a shell call, validates its retained WAL, and resumes on the original mounts.
E2E-RESUME-005 uses the long demo's write/read/search/test/fix/verify sequence for arithmetic, stable
deduplication, word counts, CSV totals, and bounds checking. Each task loses a container once during
a model stream and once during a shell call. Explicit barriers select the cut; assertions check the
unacknowledged journal tail, contiguous sequences, one resumed run, final tests, and a side-effect
ledger. The default SBX-D-003 test injects environment loss around an actual runtime using the shared
runtime fixture; gateway scheduling and database reconciliation have separate gateway tests.

The `manual_daemon` binary is excluded from ordinary Docker and gate-evidence runs. Its ignored
`desktop_restart_recovers_a_running_sandbox` test supplies the real-daemon variant of SBX-D-003 on
Docker Desktop for macOS. **It restarts Docker Desktop and interrupts every container on the host.**
Run it only in a coordinated maintenance window, with the second explicit opt-in:

```sh
TQA_DOCKER_TESTS=1 TQA_ALLOW_DEV_HOST_GATEWAY=1 TQA_DOCKER_RESTART_TEST=1 TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test manual_daemon --run-ignored all --retries 0
```

The manual test observes daemon unavailability, waits for the restart command to finish, and uses the
same persistent recovery driver. It never treats a missing opt-in or Docker failure as a passing skip.

Tests use `tianon/toybox:0.8.14`, pinned to the multi-platform digest
`sha256:8fe8776eca4b6dabdee10f3522ae0ab9c51dbac70aaff9dfd377eac548f8e7f7`.
[Toybox is 0BSD](https://raw.githubusercontent.com/landley/toybox/master/LICENSE);
[musl is MIT](https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT).
The [published image build](https://raw.githubusercontent.com/tianon/dockerfiles/master/toybox/Dockerfile)
constructs the Toybox filesystem in a build stage and copies it into a scratch final image. Build-stage
tools are not distributed as the final container filesystem. No third-party source or image is vendored
here. The moved-tag test creates and deletes a local metadata-only derivative of that same pinned image.

The image's default shell exits without input. Tests therefore use the approved
`test_command_override` to keep it alive without allocating stdin. Creation logs a warning when this
test-only override is set; production gateway configuration must leave it unset. A separate, isolated
test helper initializes each test volume's permissions; production sandbox creation never runs as root.
Toybox's numeric uid lookup needs a passwd entry, so uid assertions read the kernel's process status.
The pure and live SBX-D-001 tests share one hardening table; the live adapter obtains writable-mount
status from inspect's explicit `RW` field because the Engine omits `ReadOnly: false` in HostConfig.

Fixtures catch assertion panics and clean their labelled containers, instance network, named volume,
and moved image tag on success or failure. They never prune unrelated Docker resources. All required
integration cases passed on the verified Desktop installation without a platform skip.

The `runtime_posture` suite applies the production backend to the actual Workstation image without a command
override. It attempts root filesystem writes and mount syscalls, compares the private PID namespace's process
count with Docker's host-side view, checks the absence of capabilities, and inventories mounts and block devices,
and proves another owned workspace and host paths are inaccessible from inside (SEC-014).
The egress stack's `confinement` case exercises SEC-002 and TOOL-S-017 through real model tool calls: file-tool
traversal, absolute paths, symlinks, malformed paths and protected git writes fail with durable typed results.
Shell writes to absolute workspace and temporary paths and the runtime's own state mount succeed; root and token
writes fail, and outside canaries remain unchanged. Run these cases with the same immutable image pins as egress.

The `workspace_clone` suite reuses that owned stack with a local TLS Git server. The ordinary runtime establishes
the sandbox's proxy binding. A test-only executable then calls the same workspace initializer in separate fixture
directories inside that sandbox, with the fixture CA explicitly trusted. It checks default-branch, branch and tag
contents, full history, a denied host and an unchanged existing workspace. The origin must observe every Git request
from the gateway's address; removing the forced proxy makes the contract fail. Native runtime tests separately cover
`session.config`, journal ordering, restart, reconnect during cloning and the failed pending run.

```sh
docker build --target clone-probe -f images/runtime/Dockerfile -t tqa-runtime:clone-probe .
TQA_CLONE_PROBE_IMAGE="$(docker image inspect --format '{{.Id}}' tqa-runtime:clone-probe)" TQA_DOCKER_TESTS=1 TQA_GATEWAY_IMAGE="$GATEWAY_IMAGE_DIGEST" TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation cargo nextest run -p tqa-sandbox --test workspace_clone --run-ignored all --retries 0
```

The probe is a fixture executable, not a runtime startup mode. Its explicit image target is separate from both
production targets. The Git origin is local; no public repository or repository credential is required.

Dependencies: Bollard provides typed Engine transport with Rustls; Tokio and tokio-util provide bounded
I/O and cancellation; futures handles Engine streams; async-trait exposes object-safe async boundaries;
serde/serde_json encode the transport contract; secrecy redacts the runtime credential; url validates
endpoints; thiserror provides stable typed errors; tracing records bounded lifecycle metadata.
Development dependencies tempfile and proptest provide isolated workspaces and configuration properties.
The runtime, context, and testkit workspace crates supply actual process supervision, WAL validation,
and scripted gateway/model fixtures for recovery tests; no new external testing package is required.
