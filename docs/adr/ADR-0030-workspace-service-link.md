# ADR-0030: A workspace service over the runtime link for microVM workspaces

## Status

Proposed

## Date

2026-09-26

## Context

On the Firecracker backend a workspace is an ext4 block image that the gateway must never mount or read while a VM
holds it (ARCHITECTURE §4.5): "the gateway reads workspace files only through the runtime inside the sandbox". Today
the files API, uploads, message attachments and run changes answer `409 files_unavailable` there (GW-I-057). Fork
needs no runtime: it copies the image as an opaque file (§4.6, GW-I-064).

The runtime link carries typed JSON frames with a size cap (§9), and until now only one-way ones: the gateway delivers
actions and configuration, the runtime sends events. A files request needs an answer, so the link needs its first
request/reply exchange. The runtime also needs the same manifest walker the gateway uses for run changes on Docker,
so that both backends produce identical snapshots.

## Decision

1. **A workspace service in the runtime** answers gateway requests about the workspace its own sandbox mounts:
   `list`, `read`, `stat`, `upload`, `manifest` and `blobs`. Paths are confined with the tools crate's workspace path
   policy (symlinks resolved against `/workspace`, `..` refused, `.tqa/` hidden at the root). The HTTP endpoints,
   their authorisation, limits and response shapes do not change; on Docker the gateway keeps reading the directory.
2. **Request/reply on the link** (protocol revision 7, ADR-0026):
   - the gateway sends `workspace.request { request_id, op }` and `workspace.cancel { request_id }`;
   - the runtime answers with `workspace.reply { request_id, part, last, result }`, where `result` is a payload or
     `error { code, message, retryable }`;
   - `request_id` is gateway-assigned per link and never reused on it;
   - the gateway bounds pending requests per link, applies its I/O timeout and the HTTP request's cancellation to
     each, sends `workspace.cancel` on either, and fails every pending request when the link drops;
   - a reply for an unknown or finished id is dropped and counted;
   - replies are unsequenced and never stored, forwarded to clients or put in events;
   - a `blobs` reply is one `blob_part` per chunk, each blob's chunks contiguous and in the requested order. A runtime
     that can produce none of the requested blobs (their files changed, or it never listed them) answers a single
     `error` part with `not_found`, which the gateway reads as "no copies". Any other error fails the snapshot, which
     is logged and never holds a message back.
3. **Bounds** are named constants in `tqa-types` (`WORKSPACE_CHUNK_BYTES` 128 KiB of data per part, which fits under
   `MAX_FRAME_BYTES` once encoded; `MAX_WORKSPACE_STAT_PATHS` 64; `MAX_WORKSPACE_BLOB_HASHES` 256). Every size and
   count the HTTP layer enforces today comes from `[files]` configuration and is carried in the request, so the runtime
   has no second configuration. The per-link pending bound is gateway configuration.
4. **Trust rule.** The runtime is inside the sandbox and may be controlled by the model. It can only misreport the
   workspace its own sandbox already mounts, which the model can already change. The gateway still validates every
   reply as untrusted input: sizes, counts, UTF-8 names and paths without traversal, and the SHA-256 of every blob it
   stores. A reply can never answer another request or reach another workspace. No secret, mount or egress is added.
5. **Which runtime answers.** The runtime of the workspace's live sandbox. With none, the gateway starts the
   conversation's sandbox through the normal path (admission, fair queue, quota); it idles out as usual. It never
   starts a sandbox for a conversation the caller may not run: a read-only sharee is served by an existing sandbox or
   refused.
6. **Dependency.** The pure manifest walker (skip rules, records, hashing) moves from `tqa-gateway` into `tqa-tools`
   unchanged, and `tqa-gateway` gains the edge `tqa-gateway → tqa-tools` (ARCHITECTURE §3, gate G-07). Both sides then
   build snapshots with one implementation.

## Consequences

- Files, uploads, attachments and run changes work on the microVM backend with the same API and limits as on Docker.
- A files request can cost a VM start (about 140 ms p50, PERF-011) when no sandbox is live. That start counts against
  admission like any other.
- Runtimes below revision 7 are refused with `426 runtime_too_old` until rebuilt, as for every revision.
- The gateway links `tqa-tools`. That crate stays pure (ARCHITECTURE §3), so the gateway gains no host API through it.
- Snapshot sizes and contents for run changes cross the link. They are bounded by `[files]` snapshot limits, and blobs
  the gateway already stores are not sent again.
- The codecs, the gateway's reply validation and the service's path confinement are parsers and validators: 100 % lines and regions
  plus mutation testing (ENGINEERING-GUARDRAILS §2.2).

## Links

- ARCHITECTURE §3, §4.5, §4.6, §9, §12, §15; ENGINEERING-GUARDRAILS §1.4, §1.11, §8; DEV-GUIDELINES rules 7-10, 14, 22
- ADR-0006 (Firecracker), ADR-0011 (revision 7 is shared), ADR-0026 (protocol revision)
- TEST-PLAN GW-I-057, GW-I-064, WEB-E2E-021
