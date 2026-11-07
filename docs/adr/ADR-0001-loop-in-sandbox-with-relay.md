# ADR-0001: Agent loop in the sandbox with a model relay

## Status

Accepted

## Date

2026-09-22

## Context

The agent executes untrusted tools and user code. Keeping the loop in the gateway would require
remote tool execution and would couple the loop to host services. Provider credentials must stay
outside the sandbox, and the same loop must work in containers and microVMs.

## Decision

Run the loop and tools together inside the sandbox. The runtime opens an outbound WebSocket link
to the gateway and sends model requests through its relay using a per-run token. The gateway
resolves the route, injects credentials and enforces model access and usage limits.

## Consequences

Tools execute locally and each sandbox contains its own loop. The gateway treats the runtime as
untrusted and never links the loop crate. Sandbox loss interrupts a run; the durable journal
supports recovery. Relay availability and sandbox egress enforcement remain gateway obligations.

## Links

- [Architecture sections 1–3 and 12](../ARCHITECTURE.md)
- [Product requirements FR-7, FR-13, FR-14 and FR-15](../PRD.md#81-functional)
