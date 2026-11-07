# ADR-0005: Runtime event sequence and durable replay

## Status

Accepted

## Date

2026-09-22

## Context

The transcript, model messages, resume and usage must agree on one conversation history.
Runtimes and clients can disconnect independently, so arrival order alone cannot establish
that history or identify missing frames.

## Decision

The runtime assigns a sequence number per conversation and journals each event before sending
it. The gateway verifies contiguous sequence numbers and persists received events in PostgreSQL
with `(conversation_id, seq)` unique. A sequence gap closes the link or stream with `seq_gap`.
Client streams use `seq` as the event identifier and replay from `Last-Event-ID` before going live.

## Consequences

The event log is the source of truth and messages are a pure projection. Runtime recovery starts
from the last complete journalled step without assigning duplicate sequence numbers. Gateway
replay and the transition to live delivery must preserve order and prevent duplicates.

## Links

- [Architecture sections 5.0, 5.5, 9 and 10](../ARCHITECTURE.md)
- [Product requirements FR-1, FR-7, FR-16 and FR-17](../PRD.md#81-functional)
