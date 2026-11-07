# ADR-0020: Gateway-authoritative history and the runtime write-ahead log

## Status

Accepted

## Date

2026-09-22

## Context

The runtime must not lose events when it or its sandbox dies, and resume must be exact. An earlier draft kept the
journal inside the workspace and resumed from it. A workspace can be shared by several conversations and users, and
its content is writable by the agent's shell, so history kept there can be read or rewritten by another conversation's
agent. The earlier resume rule, "continue after the last complete step", also re-ran the model for a step whose
response had already arrived, which breaks approvals that were pending when a run was parked.

## Decision

History is authoritative at the gateway, append-only in PostgreSQL. The runtime writes each frame to a write-ahead
log in a per-conversation state directory outside the workspace, mounted only into that conversation's sandbox, before
sending it. On link registration the gateway reports the last stored `seq` and the runtime resends only later frames.
A new sandbox loads history from the gateway and takes only the unacknowledged tail from its log. The event
`model.completed` marks a fully received response, and the resume point follows ARCHITECTURE §5.5: next step, tool
phase, or a new model attempt.

## Consequences

No conversation can read or alter another's history. Parked approvals resume in the tool phase with the same calls.
Interrupted tools are reported to the model as interrupted rather than silently re-run. The runtime needs one extra
mount and one history endpoint.

## Links

- ARCHITECTURE §4.5 and §5.5; TEST-PLAN CORE-U-038, SEC-017
