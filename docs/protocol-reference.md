# Protocol reference (TQA protocol v1)

The frames the runtime, the gateway and clients exchange. The source of truth is the JSON Schema exported from
`tqa-types`, `crates/tqa-types/schema/tqa-protocol-v1.json` (ADR-0010); this page names every frame, event, error
code and type in it, and gate G-09-complete fails when it misses one or names one the schema doesn't have. The design
is [ARCHITECTURE §9](ARCHITECTURE.md#9-wire-protocol-tqa-protocol-v1); the HTTP API is `crates/tqa-gateway/openapi.json`.

## Bindings and rules
- **Runtime link:** one WebSocket per sandbox, dialled out by the runtime with its per-run token. **Client binding:**
  server-sent events down (`GET /conversations/{id}/events?stream=1&protocol=1`) and plain POSTs up.
- `seq` is assigned by the runtime per conversation and checked contiguous by the gateway; a gap closes the link or
  stream with `seq_gap`. On the client binding each event's SSE `id:` is its `seq`, and a reconnect with
  `Last-Event-ID` (or `after=<seq>`) replays from PostgreSQL, then continues live: every event exactly once (ADR-0005).
- Client inputs are exact (unknown fields and malformed values are refused); server frames ignore unknown fields, so
  additive changes keep older readers working. The runtime link carries a revision (ADR-0026).
- Frames have a size cap; `tool.output` is chunked; the stream sends a keep-alive comment every 15 s. Actions carry an
  idempotency key.
- **Retired** frames and codes (ADR-0035 removed approvals) are read only from histories written before it; a runtime
  that sends one is refused.

## Client frames (`ClientFrame`)
| Type | Binding | What it does |
|---|---|---|
| `session.init` | both | Open a conversation and ask for replay from `last_seq` |
| `user.message` | both | Send a message (`POST /conversations/{id}/messages`) |
| `run.cancel` | both | Ask a run to stop (`POST …/runs/{run_id}/cancel`) |
| `session.config` | link, gateway → runtime | The conversation's configuration: route, profile, environment, limits, network allow-list, last `seq` |
| `session.update` | link, gateway → runtime | A new route for the runs that follow |
| `workspace.request` | link, gateway → runtime | A workspace operation (list, read, stat...) the files panel needs |
| `workspace.cancel` | link, gateway → runtime | Cancel a workspace request |
| `screen.sign_out` | link, gateway → runtime | Sign the desktop's browser out everywhere (ADR-0039) |
| `screen.stored` | link, gateway → runtime | Whether the gateway kept an uploaded screenshot |

## Server frames (`ServerFrame`)
| Type | Binding | What it carries |
|---|---|---|
| `session.ready` | both | The sandbox's state and queue position; unsequenced, sent before replay and on each change |
| `event` | both | One durable event (below), always with its `seq` |
| `error` | both | A classified failure (`ErrorCode`, below), with a `seq` when it concerns one frame |
| `heartbeat` | link, runtime → gateway | Run supervisor liveness every 30 s; never stored or sent on |
| `workspace.reply` | link, runtime → gateway | One part of a workspace request's answer; never stored |
| `screen.upload` | link, runtime → gateway | A chunk of a screenshot for the gateway's store |
| `screen.sign_out.done` | link, runtime → gateway | The answer to `screen.sign_out`, once the browser has started again: `ok`, `reason`, and `ready` (the browser back and focused; absent: true) |
| `screen.reset` | link, runtime → gateway | A desktop run's end reset is over: `ok` (the profile emptied) and `ready` (the browser back and focused; absent: true) |
| `message.queue` | client | The messages waiting behind the task's run changed |
| `task.titled` | client | The model named the task |
| `team.changed` | client | The task's agent team changed (ADR-0034) |
| `screen.notice` | client | A screen notice the gateway stored (ADR-0039) |
| `host_key.used` | client | The run used a host key, once per run and key (ADR-0037) |
| `host_key.refused` | client | The proxy refused a keyed call; nothing reached the host |
| `permission.request` | retired | An approval request (ADR-0035) |

## Events (`Event`, inside `event`)
| Type | Meaning |
|---|---|
| `message.user` | The accepted user message, so history is complete from events alone |
| `run.started` | A run begins |
| `context.loaded` | What the run loaded into the model's context: instruction files, skills, skipped skill folders |
| `step.started` | A step begins, with its `attempt`; a retried step supersedes the earlier attempt's output |
| `text.delta` | A visible text fragment |
| `reasoning.delta` | A reasoning fragment |
| `reasoning.state` | Opaque provider reasoning state, kept in stream order (ADR-0027) |
| `tool.call` | A tool call is prepared |
| `model.completed` | The attempt's whole response arrived |
| `tool.start` | A tool starts |
| `tool.output` | An ordered output chunk, what the person sees |
| `tool.end` | A tool reaches its end state; a failed call carries its code and, for a refused destination, the target |
| `tool.result` | Exactly what goes into the model's context, after capping |
| `step.completed` | Every call of the step has a result; with the step's duration |
| `compaction` | Older turns were replaced by a structured summary |
| `usage` | Token usage |
| `budget.warning` | A run budget reached its warning threshold |
| `provider.retry` | A model attempt failed and another follows after back-off; a text-protocol retry names what was wrong (`fallback_problem`), never the reply's content |
| `route.fallback` | The run moved to another route after its route failed |
| `decision` | A decision the engine made and the loop applied (ADR-0013) |
| `decision.fallback` | The loop set the engine's answer aside for the step |
| `mcp.server` | An MCP server's state for the session (ADR-0011) |
| `workspace.clone` | Progress of the first clone of a repository into the workspace |
| `workspace.full` | The workspace ran out of space |
| `environment.changed` | The environment's template or capabilities changed |
| `screen.capture` | A screenshot a `computer` call took, stored before it is journaled (ADR-0039) |
| `sandbox.state` | The sandbox's lifecycle state changed |
| `run.completed` | The run ended successfully |
| `run.cancelled` | The run stopped after a cancel |
| `run.failed` | The run stopped with a classified failure |
| `run.resumed` | A journaled run resumed at a complete step |
| `permission.resolved` | retired: an approval's answer (ADR-0035) |
| `permission.expired` | retired: an approval that expired |
| `run.parked` | retired: a run parked for an approval |

## Error codes (`ErrorCode`)
| Code | Meaning |
|---|---|
| `unknown_frame` | A frame kind the receiver doesn't know; resending it unchanged can't help |
| `invalid_frame` | A malformed frame; correct it first |
| `seq_gap` | A sequence gap; reconnect and replay |
| `unsupported_protocol` | An unsupported protocol version |
| `busy` | No capacity now; a later attempt may succeed |
| `egress_denied` | The network allow-list refused the destination |
| `restarting` | The gateway is restarting |
| `store_unavailable` | The database is briefly unavailable |
| `provider_error` | The model provider failed; it may recover |
| `budget` | A run budget is used up |
| `repeated_failure` | The same failure repeated; it needs a person |
| `no_progress` | The run stopped making progress |
| `fallback_parse` | On a route without native tool calls, the reply could not be read as a tool call, even after one retry |
| `io` | A disk or read failure |
| `internal` | An internal invariant failed |
| `sandbox_failed` | A sandbox could not start or register |
| `runtime_start_failed` | The runtime exited before it registered |
| `action_failed` | An accepted action failed to run |
| `workspace_quota` | The workspace is over its quota |
| `state_quota` | The conversation's runtime state is over its bound; an administrator must act |
| `decision_stop` | A decision engine stopped the run (ADR-0013) |
| `decision_engine_unavailable` | The task's decision engine is gone or disabled |
| `decision_engine_budget_spent` | The decision engine used its budget for the period |
| `reply_cut` | The model's reply hit its output limit while the run's own budget was left |
| `host_keys_unsupported` | The environment's runtime is too old to take host keys; the run goes on without them |
| `approvals_removed` | Written only by the upgrade, for a run parked for an approval when ADR-0035 removed them |
| `permission_binding` | retired: an approval binding that no longer held (ADR-0008, superseded) |

## Other types
The schema's remaining definitions, by area; their fields and bounds are in the schema.
- **Identifiers and positions:** `ConversationId`, `RunId`, `ToolCallId`, `WorkspaceId`, `Seq`, `QueuePosition`,
  `QueueReason`, `ActionHash`, `AttachmentPath`, `ErrorTarget`.
- **Session configuration:** `RouteSnapshot`, `WireProtocol`, `CacheHint`, `ReasoningSnapshot`, `ReasoningItem`,
  `ProfileSnapshot`, `Environment`, `RunLimits`, `ComputerLimits`, `NetworkSnapshot`, `NetworkEntry`,
  `WorkspaceSnapshot`, `WorkspaceClone`, `WorkspaceCloneStatus`, `SandboxBackend`, `SandboxState`, `SandboxSummary`,
  `SandboxRecovery`, `PermissionContext`.
- **Runs and tools:** `FinishReason`, `BudgetKind`, `CompactionSource`, `CompactionSummary`, `ToolStatus`,
  `ToolFailure`, `UploadedFile`, `LoadedInstruction`, `LoadedSkill`, `SkippedSkill`, `SkillSkipReason`, `SkillTier`,
  `FallbackProblem`, `ProblemName`, `ProblemPath`, `JsonType`.
- **Decision engines:** `DecisionEngineSnapshot`, `DecisionExposure`, `DecisionPoint`, `DecisionChoice`,
  `DecisionFallbackReason`.
- **MCP servers:** `McpServer`, `McpServerState`, `McpToolFilter`, `McpTransport`.
- **Workspace requests:** `WorkspaceOp`, `WorkspaceResult`, `WorkspaceErrorCode`, `ListItem`, `StatItem`,
  `EntryKind`, `ManifestEntry`.
- **Host keys:** `HostKeyGrant`, `HostKeyScheme`, `HostKeyOutcome`.
- **Agent teams:** `Brief`, `LeadBrief`, `TeamWake`, `WorkspaceMode`.
- **Computer use:** `CapturePhase`, `ScreenEvidence`, `ScreenNoticeKind`, `ScreenNoticeReason`, `ScreenPerson`,
  `ScreenRole`, `ScreenTarget`, `ScreenEnded`, `SignOutFailure`, `StoreRefusal`.
- **Retired Git push** (ADR-0037 §7): `GitPushAuthorization`, `GitPushFailure`, `GitPushOperationId`,
  `GitPushOperationRequest`, `GitPushReceipt`, `GitPushRelayError`, `GitPushTarget`.
