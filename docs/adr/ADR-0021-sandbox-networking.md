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

## Amendment: the sweeps keep a verdict (17 Nov 2026)

The periodic removal of orphaned sandbox networks (and of stray containers, GW-I-191) reports what it found and what
it could not remove, by name. The gateway keeps the last two passes and serves a verdict: something that failed to go
in two consecutive passes, or was found again after a pass removed it, is left behind, named with the engine's error
code and the command that removes it by hand (the Sandboxes page design note, §3.6). The counts are in the readiness
detail and the metrics; the names only on the administrator's page. Named workspace volumes would join the same sweep
in a volume workspace mode; the gateway creates and labels none today, so no volume is counted.

## Links

- ARCHITECTURE §4.2 and §4.3; ADR-0004; TEST-PLAN SEC-016, SEC-018; the Sandboxes page design note
