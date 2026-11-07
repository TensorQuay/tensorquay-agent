# ADR-0026: Runtime protocol revision

## Status

Accepted

## Date

2026-09-25

## Context

Protocol 1 grows by addition: `workspace.push`, `max_request_bytes` and `fallback_routes` were added to the frames
the gateway sends. The runtime parses its configuration strictly (unknown fields are refused), which is right at a
trust boundary: a runtime that silently ignored `workspace.push` or a request-size bound would behave differently
from what the gateway enforces. But a gateway newer than the runtime image then stalls silently. The runtime cannot
parse the first `session.config`, the link never becomes ready, and the user sees only "the agent stopped while
starting". Operators pin runtime images by digest, so upgrading the gateway without the runtime is a realistic
mistake.

## Decision

- **One integer.** `tqa-types` defines `PROTOCOL_REVISION`, raised whenever the gateway may send a field or frame an
  older runtime cannot parse, and a table `PROTOCOL_REVISIONS` with one row per revision (1 base,
  2 `workspace.push`, 3 `max_request_bytes`, 4 `fallback_routes`; later rows in the table below). The table length equals the revision, and each
  row names the schema property it adds, so a raise without a row does not compile and a row without a property
  fails its test.
- **Floor = current revision.** `MIN_RUNTIME_REVISION = PROTOCOL_REVISION`. The gateway never builds a reduced
  configuration for an older runtime: that would be a second code path at the trust boundary, and each new field
  would need a judgement about whether leaving it out is harmless. Gateway, runtime and web ship together from one
  compose file. A newer runtime with an older gateway stays accepted.
- **Handshake.** The runtime sends `TQA-Protocol-Revision: <n>` on the link upgrade request; absent means 1. The
  gateway refuses a runtime below the floor before any frame (HTTP 426, `runtime_too_old`, with `needs` and `has`),
  retires the sandbox with that reason, and shows the task a plain notice. The comparison is one function in
  `tqa-types` (`check_runtime_revision`), shared by the gateway and the sandbox.
- **Early warning.** The runtime image carries the label `org.tensorquay.protocol-revision`. The Docker backend's
  `check_runtime_image` reads it with the existing bounded inspection, so readiness and setup can refuse a configured
  image that is too old before any task starts. An unlabelled image counts as revision 1.
- **Fallback.** A runtime that still cannot parse `session.config` (a bug, or a gateway without the check) sends one
  unsequenced `unsupported_protocol` error naming the field path and its own revision, closes the link and exits
  instead of stalling.
- **Rule.** A change that makes the gateway send something an older runtime cannot parse raises
  `PROTOCOL_REVISION`, adds its table row and changes the Dockerfile label in the same commit (ARCHITECTURE §15).

## Consequences

- Version skew fails at once with a message that says what to update, instead of a silent stall.
- Every addition to the gateway's frames forces a runtime upgrade, even when it would have been harmless; accepted
  for simplicity. A later ADR may lower the floor; the check is a `<` comparison against it.
- Custom runtime images must keep the label; each runtime target provides it with the binary (note of 2026-10-29).

## Links

ARCHITECTURE sections 4 and 15; TEST-PLAN PROTO-U-008, RT-I-019, SBX-U-001, SBX-C-017, GW-I-050.

## Revisions after 4
The authoritative list is `PROTOCOL_REVISIONS` in `tqa-types`; this table mirrors it for readers.

| Revision | Adds | Decided in |
|---|---|---|
| 5 | `reasoning.state` events with their opaque item in the runtime history | ADR-0027 |
| 6 | The route's reasoning budget table for protocols that take a thinking budget | ADR-0027 |
| 7 | MCP servers (`ClientFrame.mcp`) and the route's cache hint (`RouteSnapshot.cache_hint`) in session.config; the workspace request and cancel frames on the link (`workspace.request`, `workspace.cancel`); `PermissionRule.tool` and `SessionGrant.tool` also accept MCP tool names (`mcp.<server>.<tool>`) | ADR-0011, ADR-0031, ADR-0030 |
| 8 | The agent profile in session.config (`ClientFrame.profile`: name, instructions, tools, skills); the environment's template version (`Environment.version` and `environment.changed`'s version); policy rules on the wire up to 192 (128 from the install, 64 from a profile) | ADR-0012, ADR-0032 |
| 9 | The decision engine in session.config (`ClientFrame.decision_engine`: name, relay path, `timeout_ms`, exposure, and optionally the `points` it answers), sent only to runtimes announcing 9; the `decision` and `decision.fallback` events; the `decision_stop` error code and, gateway to clients only, `decision_engine_unavailable`; the gateway's revision (`ClientFrame.gateway_revision`) and the policy's origins (`PermissionRule.origin`, `PolicySnapshot.policy_origin`) in session.config, sent only to runtimes announcing 9, and who denied a call (`tool.result`'s `denied`), reported only to gateways announcing 9. The floor stays 8 (see the note below) | ADR-0013, T7 |
| 10 | The `reply_cut` error code; the Git push failure `key_use_unrecorded`; the route snapshot's `first_response_secs`; a network entry's bare `*` host; the `data` capability; the queue reason `own_sandbox`; a failed `tool.end`'s `failure` (code and target). Raises the floor to 10 | Several; see `PROTOCOL_REVISIONS` |
| 11 | The run's host keys on the `user.message` the gateway delivers (`ClientFrame.host_keys`: variable, host, port, path prefix, scheme, user name and the run's placeholder, never the value) the install CA in session.config (`ClientFrame.host_ca`) and the keys of a run in progress, newly minted, for its resume (`ClientFrame.run_host_keys`), all sent only to runtimes announcing 11 (`HOST_KEYS_REVISION`); the `host_keys_unsupported` notice, gateway to clients only. The floor stays 10 | ADR-0037 |
| 12 | The lead's brief on an agent team member's first turn (`ClientFrame.lead_brief` on `user.message`: the member's role, workspace mode and the lead's brief), journalled on `message.user` (`Event.lead_brief`) and given to the model after the gateway's frame inside the untrusted-content marker; sent only on a member's turn, and a member whose runtime announces less than 12 is refused at the link. The floor stays 10 (see the note below) | ADR-0034 |
| 13 | The gateway's wake of an agent team reader (`ClientFrame.team_wake` on `user.message`: the reader's place in the log, with new team messages after it; it names no writer), journalled on `message.user` (`Event.team_wake`) so a client shows the gateway's notice, never the person's words; the turn's text is unchanged for the model. A team's lead or member whose runtime announces less than 13 is refused at the link (`TEAM_WAKE_REVISION`), and a task linked below 13 is never offered the lead; the floor stays 10 | ADR-0034 |
| 14 | The Desktop: an environment may declare the capability `desktop` (a graphical desktop the agent operates with the `computer` tool), up to seven capabilities; a queue position's reason may be `desktop_sessions` (a run waiting for a free Desktop under `desktop.max_sessions`). A conversation whose environment declares `desktop` is refused at the link below 14 (`DESKTOP_REVISION`); the floor stays 10 | ADR-0039 |

`workspace.reply` (runtime to gateway) needs no revision: the gateway is never older than its runtimes.

## Note 2026-09-28: revision 9 leaves the floor at 8
The technical lead's decision for ADR-0013's revision. Raising the floor would rebuild every runtime image, template
and microVM root filesystem for a field that only conversations with a decision engine use. So `MIN_RUNTIME_REVISION`
stays 8 and the gateway sends `decision_engine` only to a runtime announcing 9 or later
(`DECISION_ENGINE_REVISION`). A runtime below 9 serving a conversation whose profile names an engine is refused at
the link exactly as one below the floor is (426, `runtime_too_old`, `needs` 9), its sandbox retired with a notice that names the engine's revision; it is never run without the engine. Every other conversation keeps linking revision-8 runtimes. This is the
one reduced configuration so far: a single optional field, absent unless an engine is named, checked in one place
(`link/endpoint.rs`).

## Note 2026-10-01: revision 11 leaves the floor at 10
The technical lead's decision, from ENGINEERING-GUARDRAILS §8 (an addition keeps the older version working for a
release). A revision-10 runtime refuses a `user.message` or `session.config` with fields it doesn't know, so the
gateway keeps the revision each runtime announced with its link and sends `host_keys` and `host_ca` only to one at
`HOST_KEYS_REVISION` (11). A revision-10 runtime runs without keys; when the turn's sender has keys the task could
use, the task's streams get the unsequenced `host_keys_unsupported` notice, so the person is told in plain words
rather than seeing calls go out without a key.

## Note 2026-10-02: revision 12 leaves the floor at 10
The technical lead's decision, as for revision 9. Only an agent team member's first turn carries `lead_brief`, and a
brief must never reach a model as the person's own words: so instead of a reduced configuration, a member whose
runtime announces less than `LEAD_BRIEF_REVISION` (12) is refused at the link exactly as a decision engine's task is
(426, `runtime_too_old`, `needs` 12), its sandbox retired with a notice. Every other task keeps linking runtimes from
the floor, 10, up. The runtime writes the brief into the journal entry's text, inside the untrusted-content marker
(source `lead`) after the gateway's frame, and keeps `lead_brief` beside it, so a client shows the brief from the lead
and never a person's message (RT-I-039, CORE-U-078).

## Note 2026-10-05: revision 13 leaves the floor at 10
The technical lead's decision, as for revision 12. The page pass found a helper's page showing the gateway's wake
sentence as the person's own message: the wake was a plain `user.message`. Revision 13 marks it (`team_wake`: the
reader's place in the log, after which new team messages wait), the runtime journals the marker on `message.user`, and
a client words the notice itself ("New team messages"). It names no writer: what waits may be the helpers', the
person's and the gateway's notices together, and the team's log already shows who wrote (PROTO-U-035). A wake reaches
both a team's lead and its members, so any conversation that is or was a team's lead or member is refused at the link
below `TEAM_WAKE_REVISION` (13), as a member below 12 was (426, `runtime_too_old`, `needs` 13), its sandbox retired
with a notice. A task becomes a lead mid-run, so the role a run asks for (`GET /runtime/teams/role`) offers the lead
only to a runtime linked at 13 or above (GW-I-214). Every other task keeps linking runtimes from the floor, 10, up.
Agent teams are off in v1.0, so no release carries a team below 13 (GW-I-214, RT-I-044).

## Note 2026-10-05: revision 14 leaves the floor at 10
ADR-0039 (accepted) brings the Desktop. A runtime below 14 has no `computer` tool and can't parse `desktop` in its
environment, so a conversation whose environment declares `desktop` is refused at the link below `DESKTOP_REVISION`
(14), as a team's below 13 is (426, `runtime_too_old`, `needs` 14), its sandbox retired with a notice (GW-I-223). Every
other task keeps linking runtimes from the floor, 10, up. The Desktop template is new in this revision, so no release
carries a Desktop image below 14.

## Note 2026-10-29: the label goes with the binary
The runtime binary is each image's last layer (c17, REL-U-011), so a change to the program alone reuses every
package layer. The label moves with it: every stage that copies the binary sets it, every copy equal to the
constant, and `runtime-base` sets neither (SBX-U-001). Images built from `tqa-runtime:base` still inherit it.

## Planned
- A gateway-initiated `run.stop{run_id, reason}` that the runtime ends with `run.failed{code: workspace_quota | state_quota}`, so the history records why a run over its workspace or state quota ended. Until then the gateway cancels such a run with `run.cancel` and explains it only in its own typed notice (`workspace_quota`, `state_quota`).
