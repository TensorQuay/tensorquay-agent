# ADR-0021: Sandbox networking

## Status

Accepted

## Date

2026-09-22

## Context

Egress control is only real if a sandbox has no route around the policy proxy. A mode that sets proxy variables on a
normal bridge network lets any process connect directly and ignore them. External DNS is an exfiltration channel of
its own.

## Decision

Every sandbox joins an internal network with no route out. The gateway container is attached to that network and is
the only peer: runtime link, relay, MCP proxy and policy proxy. Allowing egress sets the proxy variables and never
changes the network. Sandboxes have no external DNS; the gateway name comes from the engine's embedded DNS or a static
entry, and the policy proxy resolves allowed names. Firecracker applies the same rules with per-VM nftables. A
development-only mode with the gateway on the host uses a non-internal network, warns at start, and is never a default.

## Consequences

Deployments run the gateway as a container on the sandbox network. The sandbox backend creates internal networks only,
except in the explicit development mode. SEC-016 and SEC-018 test the rules.

## Links

- ARCHITECTURE §4.2 and §4.3; ADR-0004; TEST-PLAN SEC-016, SEC-018
