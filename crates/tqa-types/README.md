# Responsibility

Wire protocol frames, event types, JSON Schemas, identifiers and error codes shared across the workspace.

# Contract

Implements the protocol-v1 skeleton from [ARCHITECTURE](../../docs/ARCHITECTURE.md) sections 3, 5.5,
9 and 11.1. The runtime link consumes these frames over an outbound WebSocket; client streams carry
server frames over server-sent events and client actions arrive through POST requests. This crate
contains no transport, persistence or recovery implementation.

`ToolCatalogBudget` measures borrowed model-facing definitions before registration or request construction.
Its inclusive limits are 256 tools, 64 KiB of serialized parameters per tool, and 1 MiB for the complete
JSON tools array, including names, descriptions, strictness, function wrappers and punctuation.
It counts encoded UTF-8 and escapes without retaining input; `ToolCatalogLimit` exposes only the exceeded bound.

`heartbeat` is an unsequenced runtime-to-gateway link frame. The active run's supervisor emits it;
the gateway never stores it or streams it to clients. `session.config` is a gateway-to-runtime,
link-only snapshot of the route, the network allow-list, workspace display and run limits, with no credentials or
upstream address. The gateway never accepts it from a client binding. Every snapshot rejects unknown
fields. Run limits are validated during deserialization and through `RunLimits::validate`: 1–500
steps, 1–86,400 active seconds, 1024–262,144 tool-result bytes and 1–16,777,216 output tokens per run.
Older configurations default to 131,072 output tokens. Route capacities are positive,
output fits the configured context, and relay paths stay under `/relay/r/`. Reasoning choices are
bounded, unique, and contain their default. A route whose protocol takes a thinking budget instead of an effort
also carries `budget_tokens`: exactly one budget of 1024–1,048,576 tokens per allowed value, each below the
route's `max_output`, which must then be set. Absent, the field is left out of the frame.

`workspace.clone` optionally names a public HTTPS repository and a branch or tag (`ref`). Omission
preserves existing sessions. `WorkspaceClone::validate` and deserialization enforce a URL within
4096 ASCII bytes, with a valid authority and repository path, without credentials, query, fragment,
controls or whitespace. Ref names are within 256 ASCII bytes and exclude revision expressions,
option prefixes, empty or hidden components and Git's reserved ref-name syntax. Nested clone fields
are exact. The schema enforces scheme, character and length bounds; consumers also use the shared
validator for authority semantics.

`GitPushTarget` accepts exactly a credential-free HTTPS clone URL, a short branch name
and a nonzero full lowercase 40- or 64-character object ID. Unknown fields and revision expressions fail.
The internal relay contracts bind that tuple to an `ActionHash` and `PermissionContext`, returning only
a 32-character lowercase operation ID with a positive lifetime of at most 120 seconds, or a same-commit
no-op. Receipts and failures are closed enums without upstream diagnostics or credential fields.
`canonical_action_bytes` encodes the bounded sorted JSON envelope the relay binds a push to;
hashing and authorization remain outside this crate.

The sequenced `workspace.clone` event carries `status` (`started`, `completed`, `skipped`, `failed`),
an optional stable error `code`, and fixed `message` text within 256 UTF-8 bytes without controls.
Status copy contains no repository URL or process output. Initialization precedes the first model run;
completed and skipped states allow execution, while failure leaves a classified failed run.

`user.message` and `message.user` carry `attachments`: at most 32 distinct upload paths `uploads/<name>` in
submission order, where the name is 1–128 bytes of ASCII letters, digits, `.`, `_` and `-` and does not start
with a dot. Both arrays reference the schema definition `AttachmentPath` and set `uniqueItems`;
`validate_attachments` applies the same rule to constructed values and reports the first failing item by
position only.

The sequenced `compaction` event records `through_seq`, `source` (`model` or `fallback`), `version` (exactly
`COMPACTION_VERSION`, 1) and a `CompactionSummary` with the fixed fields `goal`, `done`, `pending`, `decisions`,
`key_paths` and `open_errors`. Each list has at most 32 items, the goal and each item at most 512 characters,
and all strings together at most 16 KiB. The schema states the first two bounds, and deserialization and
`CompactionSummary::validate` enforce all three. Unknown summary fields are ignored and never kept. The
compaction constants live together in `limits.rs`.

`RouteSnapshot.max_request_bytes` optionally states the gateway's relay request body limit (1024 through
16 MiB). It is omitted when absent, and `request_budget()` then uses `DEFAULT_MAX_REQUEST_BYTES` (1 MiB, the
relay's default). `ImageMediaType` names PNG, JPEG, GIF and WebP and recognises them from a file header. The image
limits `MAX_IMAGES_PER_MESSAGE` (4), `MAX_IMAGE_BYTES` (5 MiB) and `IMAGE_TOKENS` (1600) are in `limits.rs`.

`session.config` and `session.update` carry optional `fallback_routes`: at most `MAX_FALLBACK_ROUTES` (3) route
snapshots with unique names that differ from `route`, omitted when empty. The sequenced `route.fallback { from, to,
reason }` event records a run switching routes; names are 1-256 bytes and the reason at most 128 bytes (ADR-0025).

The sequenced `context.loaded { instructions, skills, skipped }` event (FR-11) reports what the runtime loaded into
the instruction slot when a run started or resumed: instruction file paths with a truncation flag, skills with their
`SkillTier` and description, and skipped skill folders with a `SkillSkipReason` code. Each list holds at most
`MAX_CONTEXT_LOADED_ITEMS` (64); `ContextLoaded::into_event` bounds them. It is additive and needs no protocol
revision: the runtime finds skills at fixed mount paths, and clients hide it until the context panel.

`session.update { route }` replaces the runtime's route snapshot for subsequent runs. It uses the same validated
`RouteSnapshot` as `session.config`, has no sequence number, and emits no event. An active run keeps its own route.
This frame is gateway-to-runtime only; client bindings must reject it.

`session.ready` reports transient sandbox status before replay and whenever that status changes. It has
no sequence and is never stored. While `sandbox.state` is `queued`, the gateway may include
`queue: { ahead }`, counting waiting runs served before this one (0 means next). `QueuePosition::new`
and deserialization enforce an integer in `0..=10000`. The gateway omits queue metadata in every other
state. Absent or null positions deserialize to `None` and are omitted on serialization, preserving
legacy frames; future queue metadata fields are ignored.

Revision 9 has no policy and no approvals (ADR-0035). `session.config` carries `network`, the task's allow-list for
the prompt: at most 32 entries (`MAX_NETWORK_ENTRIES`) of a lower-case host name or `*.` and a domain, never an
address, with 1–16 distinct non-zero ports, and `more` (at most 256) counting what isn't listed (PROTO-U-026).
Stored histories keep loading: `permission.request` (keeping only its `seq`), `permission.resolved`,
`permission.expired`, `run.parked`, the sandbox state `parked` and the code `permission_binding` are retired,
read-only markers that parse the old payload and are never sent (PROTO-U-027). `approvals_removed` is the code the
gateway's upgrade writes when it ends a run that was parked for an approval.

The optional `environment` snapshot defaults to Minimal with `shell` and an empty digest; that legacy value is
omitted when encoding. Explicit snapshots use `Environment::new` (validation plus canonical ordering) or
`Environment::validate` (validation only). Templates match `[a-z][a-z0-9_-]{0,63}`. Zero through seven unique
capabilities are ordered `shell`, `python`, `node`, `build`, `browser`, `data`, `desktop` (revision 14, ADR-0039).
The digest is a lowercase SHA-256 pin, optionally prefixed by a registry reference and `@`, within 512 ASCII
bytes and without credentials. Only the legacy identity permits an empty digest.
`environment.changed` records the template and canonical capabilities; digest-only updates do not change that
event identity. Its fields use the same admission rules as the snapshot.

Durable history includes `message.user`, numbered `step.started` attempts, `model.completed`,
`tool.result` and `step.completed`. An older `step.started` without `attempt` defaults to 1.
`step.completed.duration_ms` records active step time and defaults to zero on older frames.
`budget.warning` reports steps, whole active seconds, output tokens or consecutive steps without
progress against the named limit. `usage.estimated` marks heuristic counts when provider usage is
absent; older usage events default to false.
`provider.retry` names the failed attempt and delay; its reason contains only an error code and
optional HTTP status, within 128 bytes, never a response body.
`model.completed` follows every delta and prepared tool call for that attempt, and carries a
provider-independent finish reason. Tool-result content is limited to 256 KiB of UTF-8 bytes;
the encoded frame still must fit the separate 256 KiB frame cap, including JSON overhead.

Client frames are exact: unknown fields, duplicate fields and invalid identifiers are rejected.
Server frames, nested events and sandbox summaries ignore additional fields so older receivers can
consume additive changes. Unknown frame or event variants are not silently discarded. Error codes
are a closed vocabulary. Removals and semantic changes require a new protocol version; the old version
remains supported for one release.

Call `ClientFrame::parse` or `ServerFrame::parse` at the wire boundary. Both reject frames over
256 KiB before JSON deserialization. Errors carry a stable code, a field named by serde when available,
or `$` for a root or unqualified type failure, plus a human-readable detail. Version negotiation is
separate: `check_protocol` accepts only version 1.

Protocol 1 grows additively, and client frames are strict, so a runtime cannot parse a field added after it
was built. `PROTOCOL_REVISION` (currently 6) counts those additions. `PROTOCOL_REVISIONS` lists each one with
the schema definition and property it added: 1 is the base, 2 `workspace.push`, 3 the route's `max_request_bytes`
4 `fallback_routes`, 5 `reasoning.state` with its item and 6 the reasoning `budget_tokens` table. A test ties the table
to the schema export, and the array's length is the revision, so a
raise needs a row. Any change the gateway sends that an older runtime cannot parse raises the revision in the same
commit. The runtime sends its revision in the `TQA-Protocol-Revision` header (`PROTOCOL_REVISION_HEADER`), and its
image carries it in the `org.tensorquay.protocol-revision` label (`RUNTIME_REVISION_LABEL`). `parse_revision` reads
either one: an absent value is revision 1, and anything but a canonical decimal is refused.
`check_runtime_revision` is the only comparison. It refuses a runtime below `MIN_RUNTIME_REVISION`, which equals
the current revision until an ADR lowers it, and accepts newer runtimes (ADR-0026).

`reasoning.state { protocol, route, model, bytes, item }` records one opaque provider reasoning item (ADR-0027).
- `item` is a `ReasoningItem`: JSON object text of at most `MAX_REASONING_ITEM_BYTES` (128 KiB), whose `Debug` form
  shows only its size.
- It is absent when the state exceeded `MAX_REASONING_ITEMS_PER_STEP` (16) or `MAX_REASONING_BYTES_PER_STEP`
  (512 KiB).
- `ServerFrame::for_clients` removes it: the events API and SSE never carry items, and only the runtime's history
  does.

Identifiers are validated through `TryFrom<String>`, `FromStr` and deserialization: 1–64 ASCII letters,
digits, underscores or hyphens. User-message limits count UTF-8 bytes. Messages allow at most 32 attachment identifiers.
Event timestamps use RFC 3339 syntax and calendar validation. JSON Schema's
`maxLength` counts Unicode scalar values, so consumers must also enforce the 64 KiB UTF-8 byte limit
on message text and the encoded-frame byte cap.

`SeqChecker` starts after the supplied sequence, preserves its state on a gap, and reports exhaustion
after accepting the maximum sequence instead of wrapping. `chunk_tool_output` splits UTF-8 output
at 64 KiB boundaries and emits one final empty chunk for empty output. `ToolOutputAssembler` permits
interleaved calls and requires explicit concurrent-call and per-call byte limits. Callers may pass
`RECOMMENDED_MAX_IN_FLIGHT_CALLS` (32) and `RECOMMENDED_MAX_TOOL_OUTPUT_BYTES` (16 MiB); these are
recommendations, not implicit defaults. Rejected chunks leave buffered state unchanged.

## Error codes

Each error frame carries its own `retryable` flag. `ErrorCode::retryable()` supplies these defaults:

| Code | Retryable | Reason |
|---|---|---|
| `unknown_frame` | no | The sender must use a supported frame kind. |
| `invalid_frame` | no | The input must be corrected. |
| `seq_gap` | yes | Reconnect and replay can restore continuity. |
| `unsupported_protocol` | no | A compatible version is required. |
| `busy` | yes | Capacity may become available. |
| `permission_binding` | no | Retired (ADR-0035): read from older histories only. |
| `egress_denied` | no | The network allow-list denied the destination. |
| `restarting` | yes | The service can recover after restart. |
| `store_unavailable` | yes | The store may recover. |
| `provider_error` | yes | The provider may recover. |
| `budget` | no | The run needs an explicit budget change. |
| `repeated_failure` | no | Intervention is required. |
| `no_progress` | no | The run needs a changed approach. |
| `io` | no | Disk or read failures require operator attention. |
| `internal` | no | An invariant failure requires investigation. |
| `sandbox_failed` | yes | Sandbox startup or registration failed; retry with a new action key after recovery. |
| `runtime_start_failed` | yes | The runtime exited before registering; repair startup, then retry with a new action key. |
| `action_failed` | yes | An accepted action failed to execute; retry with a new action key after recovery. |
| `approvals_removed` | no | A run parked for an approval when ADR-0035 removed them; a message starts a new run. |

An error may carry `target: { host, port }` so the client can write an explanation from structured data.
`ErrorTarget::new` and deserialization validate a host of 1 through 253 ASCII bytes and a port of 1 through
65535. Hosts are DNS labels of 1 through 63 characters with alphanumeric ends and internal hyphens, optionally
ending in a root dot, or bare IPv4/IPv6 literals. Convert Unicode names to IDNA A-labels before construction;
brackets, embedded ports, paths and credentials are rejected. Getters preserve the supplied host spelling.
Absent or null targets deserialize to `None` and are omitted on serialization, preserving legacy frames.
Unknown target fields are ignored. Schema validators must assert formats to validate IPv6 literals.

## JSON Schema

`schema/tqa-protocol-v1.json` is a deterministic draft-7 definition registry. Select a contract with
`$ref`, for example `#/definitions/ClientFrame`, and retain the document's definitions for resolution.
It includes all frames, events, error codes and identifiers. The root registry itself does not select
an instance type. Generated schema changes are reviewed with the source change.

Regenerate or check it from the repository root:

```sh
cargo run -q -p tqa-types --bin export-schema -- crates/tqa-types/schema/tqa-protocol-v1.json
cargo run -q -p tqa-types --bin export-schema -- --check crates/tqa-types/schema/tqa-protocol-v1.json
```

Check mode exits 1 with a unified diff on drift and leaves the file unchanged. Gate G-08 runs this
check locally and in CI. The web client generates its types from these definitions in a separate gate.

# How to test

Run from the repository root:

```sh
cargo nextest run -p tqa-types
cargo llvm-cov nextest -p tqa-types --fail-under-lines 95
cargo doc -p tqa-types --no-deps
```

Tests reference PROTO-C-001 and PROTO-U-003 through PROTO-U-007. Property tests run 1000 cases each
for events, client frames and server frames, using a fixed seed for reproducibility.
