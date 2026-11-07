# ADR-0016: Activeness from lifecycle events, and sandbox admission

## Status

Accepted

## Date

2026-09-22

## Context

A sandbox holds memory and processes on someone's own machine, so it cannot stay alive after the work is done. The
question is how the gateway decides that a sandbox is still needed. Inferring it from traffic is the common shortcut
and it fails in both directions: a long tool call with no output looks dead, and a connected but idle process looks
busy. A reviewed implementation did exactly that, refreshing the idle timer on the connection's own keep-alive
replies, which meant no sandbox was ever retired.

## Decision

- A run is active from `run.started` until its terminal event. Those events come from the runtime's supervisor, which
  owns the loop task and therefore knows whether it is alive.
- The idle window is measured from the last accepted action or run lifecycle event. Socket traffic, pings, pongs and
  supervisor heartbeats never extend it, and a sandbox whose run is active is never idle.
- Heartbeats mean liveness only: a run that is busy but silent for three intervals is declared dead and its sandbox
  destroyed, with the run left resumable.
- Processes are not an input. A background process does not keep a sandbox alive, and the user's explicit keep-alive
  pin is the only way to hold one open.
- Admission divides a configured share of the host by the per-sandbox limits into slots, with a per-user cap. When no
  slot is free the gateway evicts the longest-idle warm sandbox, otherwise it queues the run with per-user fairness.
  A busy sandbox is never evicted before its maximum lifetime, and a pinned one is never evicted.
- A run waiting on a person or a channel is parked: the sandbox is destroyed and recreated when the answer arrives,
  so a slow approval never holds hardware.

## Consequences

- Retirement is decided from the same journal that drives resume, so it is testable without a live sandbox.
- A gateway restart reconciles its table against the backend's inventory rather than trusting memory.
- Idle rules must be tested with a window larger than the keep-alive interval, or the test proves nothing.

## Amendment: background processes stop when the command returns (1 Oct 2026)

A shell call owns every process it starts. When the command returns, the shell stops its process group and the
runtime, the sandbox's child subreaper, kills every other process that escaped it (a `setsid` child, a double fork)
before the call answers; the answer says that background processes were stopped. Only the MCP servers the runtime
itself started are left running. So a server or a watcher lives only within the command that started it: a model
that needs one starts it, uses it and gets its output in the same command. The keep-alive pin keeps the sandbox and its
workspace, not processes. This is what the shell and runtime already do; it is recorded so the tests and documents
stop promising more (TEST-PLAN TOOL-U-006, TOOL-U-008).

## Links

- ARCHITECTURE §4.4; ADR-0020; TEST-PLAN GW-I-010, RT-I-004, TOOL-U-008
