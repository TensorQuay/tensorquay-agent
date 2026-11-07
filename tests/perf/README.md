# Performance harnesses (TEST-PLAN §15)

The stack-level measurements live in `deploy/tests/perf_*.py`. Each one writes a result to `tests/perf/results/<date>/`
(git-ignored), holding the environment record TEST-PLAN §15.1 requires: host, kernel, CPU, Docker and Compose
versions, rootless, the commit and whether the tree is dirty, the image IDs, the load average, and IO pressure before
and after. A result without its environment is refused.

| ID | Harness | What it measures | Target |
|---|---|---|---|
| PERF-010 | `perf_start.py` | Docker sandbox start: a message accepted on a conversation without a sandbox, to its first `run.started`; 5 warm-ups, then 50 fresh sandboxes; the backend split from Docker's own timestamps. Then the returning task as a person sees it (`PERF-010-returning-task`): a task that already ran, its sandbox removed after a 10 s idle window as on the microVM backend, its next message accepted to `run.started` on a new container, same workspace, image warm; 5 warm-ups, then 50, same split | p50 ≤ 3 s, both |
| PERF-002 at another rate or on a disk database | `perf_002.py [--rate 1000] [--postgres disk]` | PERF-002's test at a recorded rate, on the given database or a throwaway PostgreSQL on a Docker volume with its defaults (the image `docker.yml` pins), removed afterwards; refuses a result that doesn't record fsync, synchronous_commit and full_page_writes (all on for disk) or whose per-event lag file (beside the result, its SHA-256 in it) is missing or changed | lag ≤ 100 ms p99 |
| PERF-011 | `perf_start.py --backend firecracker` | microVM start as a person sees it, on a prepared KVM host (the Firecracker stack's variables, `deploy/firecracker/README.md`): a message accepted to `run.started` for a new task (first boot, its images created) and for a returning task (its VM retired at idle after 10 s, the images kept), each new task then deleted; 5 warm-ups, then 50 pairs; the split from Docker's own timestamps and the runtime's "session registered" line in the VM host's log; also each sample's stream lag (not part of the total). Results `PERF-011-new-task` and `PERF-011-returning-task`; also a new task's first upload starting the VM (`PERF-011-upload-new-task`, 50 more), and the installation's very first VM start before any warm-up, one sample per stack (`PERF-011-first-start`) | p50 ≤ 2 s cold; the owner's 3 s recorded |
| PERF-020 | `perf_concurrency.py` | 16 live conversations of one user; the gateway capped at 2 vCPU / 4 GB (`deploy/compose/perf.yml`); 3 whole-load runs | no dropped events; gateway CPU p95 < 70 %; RSS < 1.5 GB |
| PERF-012 | `perf_io.py` | workspace I/O inside a sandbox (the Docker backend's options) against the host, on the same filesystem | every sandbox/host ratio ≥ 0.80 |
| PERF-012 (microVM) | `deploy/firecracker/perf_io_vm.py` | the same workload inside a task's guest, on its workspace image, as the fixture model's scripted shell calls (one run per round); the host side the same file and interpreter (the runtime image's python3) on a bind mount on the filesystem of the stack's storage; 5 warm-ups, then 50 alternating rounds of 64 MiB and 4096 random operations, each within the shell tool's 30 s deadline; on a prepared KVM host (the Firecracker stack's variables) | every guest/host ratio ≥ 0.70 |

Gate 9's numbers reproduce through `deploy/tests/gate9_run.py` (TEST-PLAN §15.1 item 5): one number alone on a
candidate, for example `python3 deploy/tests/gate9_run.py --candidate-tags <tag file> --ids PERF-010 --candidate-only`,
and `--dry-run` first to see each job's command, tree and images without starting anything.

Run them one at a time through the team's stack runner, from the repository root, for example
`TQA_DOCKER_TESTS=1 python3 deploy/tests/perf_io.py`. Two results can be compared with
`python3 deploy/tests/perf_common.py compare before.json after.json`. It refuses results from different hosts or
sessions, and flags a p50 or p99 more than 10 % worse.

## PERF-012 on a hybrid CPU
- **Hybrid cores skew the ratio.** Many current CPUs mix performance and efficiency cores; the reference host has
  8 performance cores at 5.4–5.5 GHz and 16 efficiency cores at 4.7 GHz. Unpinned, the host side and the sandbox side
  can land on different kinds of core. That skews the ratio by 10–40 % in either direction, which is more than the
  mount itself costs.
- **So both sides run on one core** by default: `--pin-cpu auto`, the last performance core. It comes from
  `/sys/devices/cpu_core/cpus` where the kernel reports core types, or else the fastest cores by maximum frequency.
  `--pin-cpu none` disables pinning, and `--pin-cpu N` picks a core. The chosen core is recorded in the result.
- **Both sides run the same interpreter** (the host's, mounted read-only in the sandbox). Each round records its
  version, and rounds from two interpreters are refused.
- **Random measures run 16,384 operations by default**, so each lasts seconds. At 4,096 they were noise-dominated:
  the random-read ratio moved from 1.25 to 0.81 across three runs.
- **Pinned, the sequential ratios read about 1.0** (1.03 write, 1.09 read on 2026-09-26), as a bind mount should.
