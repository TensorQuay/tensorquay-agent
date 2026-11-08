# ADR-0004: Egress enforcement: the policy proxy plus network rules

## Status

Accepted. Recorded on 2026-10-12 from what ships: the decision was taken with the architecture of 22 Sep 2026
(ARCHITECTURE §4.2 and §4.3) and built in Phase 3, but its record was never written. ADR-0021 (the network a sandbox
joins), ADR-0035 (the allow-list as the product's one guardrail) and ADR-0037 (host keys) build on it.

## Date

2026-10-12

## Context

The agent reads, writes and runs anything inside its sandbox (ADR-0035), so what it may reach over the network is the
boundary that matters. Proxy variables alone are advisory: any process can ignore them and connect directly, and
external DNS is a channel of its own. A name on an allow-list is not an address either: it can resolve to the host,
the cloud metadata service or the gateway's own networks, and it can be re-pointed between a check and a connection.

## Decision

1. **The network gives a sandbox one peer.** Each sandbox is on an internal network with no route out, and the
   gateway container is its only peer (ADR-0021): Docker mode creates one internal network per sandbox from the
   `[sandbox.network_pool]` subnet pool (`crates/tqa-sandbox/src/docker/sandbox_network.rs`); a microVM's tap device
   gets per-VM nftables rules that allow only the gateway's address (`rules.nft`,
   `crates/tqa-sandbox/src/firecracker/backend.rs`). There is no external DNS. Allowing egress sets `HTTP_PROXY` and
   `HTTPS_PROXY`; it never changes the network.
2. **The policy proxy decides every connection** (`crates/tqa-gateway/src/egress/`). It accepts a bounded `CONNECT
   host:port` or an absolute-form `http://` request (`head.rs`), identifies the calling sandbox by its transport
   address (`proxy.rs`), and checks the destination against the allow-list: exact hosts or `*.domain`, explicit
   ports, default deny, an empty list denying everything (`rules.rs`).
3. **The proxy resolves once and connects only to what it checked.** Every address a name resolves to is checked, and
   the connection goes to that checked set, never to a fresh lookup, so DNS rebinding can't move it (`proxy.rs`). A
   name-based rule never reaches loopback, private, link-local, shared, unique-local, unspecified, multicast,
   broadcast or the other special-purpose ranges, also when embedded in an IPv6 address; only an IP-literal rule for
   that exact address may (`rules.rs`, `guarded`).
4. **Internal rules are the one exception, and fail closed** (ADR-0035 amendment, 1 Oct 2026): an administrator may
   let a rule for one exact host name reach a private address, but never loopback, link-local, the engine's networks,
   the sandbox pool, the gateway's own or the host's addresses; while the host's addresses or the engine's networks are
   unknown, such rules refuse (`internal.rs`).
5. **A refusal names the host, port and reason** in the status line and a one-line body, with `Proxy-Status`
   (RFC 9209), so the agent can tell a blocked host from a broken server (`refusal.rs`); a denial notice reaches the
   task at most once per destination per minute (`notice.rs`).
6. **Keys never enter the sandbox.** For a host that holds a person's key, the proxy terminates TLS with the
   installation's CA, swaps the placeholder for the value only in a request to that host, and scrubs the value from
   answers (`keyed.rs`, `scrub.rs`; ADR-0037).

## Consequences

- The allow-list is enforced by the network, not by the agent's cooperation: SEC-018 (no bypass of the proxy), SEC-016
  (no DNS), SBX-C-004 (the egress policy), GW-I-012 (the proxy and its guarded ranges) and SEC-001 (an injected
  exfiltration blocked and shown) test it.
- Every allowed connection costs one proxy hop, and the gateway must run on the sandbox network; a development mode
  with the gateway on the host uses a non-internal network, warns at start and is never a default (ADR-0021).
- **Docker Desktop:** the same backend, internal networks and proxy run inside Docker Desktop's Linux VM. What is
  best-effort there is not yet written down: SBX-D-002 (a Mac, run by hand) expects its test output to state those
  limits, and it has not run for v1.0.
- **The microVM backend's keyed path** has its own case: SEC-047 and SEC-049 on the microVM
  (`crates/tqa-sandbox/tests/egress/host_keys_microvm.py`) passed on 12 Oct 2026 (TEST-PLAN §19a, the host-key entry).

## Links

- ARCHITECTURE §4.2, §4.3 and §10; PRD FR-10 (superseded), FR-15; ADR-0021, ADR-0035, ADR-0037
- TEST-PLAN SEC-001, SEC-016, SEC-018, SBX-C-004, SBX-D-002, GW-I-012
