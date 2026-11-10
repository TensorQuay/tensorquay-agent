# ADR-0011: Extension model: MCP as the plugin protocol, no in-process plugin API, the two trust positions

## Status

Proposed

## Date

2026-09-26

## Context

Agents need tools beyond the builtins. An in-process plugin API would put third-party code inside the gateway or the runtime, with
their privileges and their lifetime. The Model Context Protocol (MCP) is a widely implemented, language-neutral protocol for tools, so
one client serves many servers. PRD FR-9 asks for two tiers: `stdio` servers inside the sandbox without secrets, and remote servers
reached through the gateway's MCP proxy. Both must be configured per install, per profile and per conversation, their tools must
appear under a namespace, and they must obey the same permission policy (ARCHITECTURE §15).

## Decision

1. **MCP is the only plugin protocol.** There is no in-process plugin API. An extension runs either **inside the sandbox without
   secrets** (a stdio server) or **outside it behind the gateway, with credentials in the vault** (a connector through the MCP proxy).
   There is no third place. This record covers the stdio tier; the connector tier follows with the gateway proxy.
2. **Configuration** narrows downwards. The install catalogue (`[[mcp.servers]]`) is the only place a command enters: an absolute
   path shipped by the sandbox template, never in the workspace, with non-secret environment, a default permission class, a
   tool filter and a call timeout. A profile selects a subset, and a conversation may switch catalogue servers on or off. The
   resolved list reaches the runtime in `session.config`, which is protocol revision 7 (ADR-0026), shared with the route
   snapshot's `cache_hint`. Servers defined by workspace files are out of scope until starting one goes through an approval.
3. **The runtime supervises each server:**
   - it runs as the sandbox user, in its own process group, with cwd `/workspace`, and without the runtime's `TQA_*` variables;
   - the handshake is bounded to 10 s, a message line to 1 MiB (a longer line fails the server), servers to 8, tools to 64 per server
     plus the global catalogue limits, and concurrent calls to 4 per server;
   - each call has a timeout and is cancelled with the run (`notifications/cancelled`);
   - stderr goes to a bounded ring and is never logged verbatim;
   - a crash fails the calls in flight with a typed error, the server is restarted **once**, and a second crash disables it for the
     session; failures are events, and the runtime and the builtins stay up;
   - a changed tool list applies at the next run boundary.
4. **Naming:** internally `mcp.<server>.<tool>` (the reserved namespace). Models see `mcp__<server>__<tool>`, folded to
   `[a-z0-9_]`, at most 64 characters, with a stable suffix on collisions, because model providers refuse dots in tool names. A
   builtin can never be shadowed.
5. **Permissions:** every MCP tool has a permission class, by default **execute**, since a stdio server is code running in the sandbox;
   an admin may lower it per server. The server's annotations are untrusted and may only tighten the class. There is one policy
   engine, and prepared-action binding and session grants apply unchanged.
6. **Client library: `rmcp`, pinned exactly** (3.4.1 at first, 3.9.0 since 9 Nov 2026; the official Rust SDK;
   Apache-2.0; MSRV 1.88), with only the `client` and
   `transport-async-rw` features. The runtime spawns the process itself, keeping its process group, environment, stderr capture and
   kill, and hands rmcp a bounded transport: a framed reader with `JsonRpcMessageCodec::new_with_max_length(1 MiB)` that ends the
   stream at the first framing error. The protocol version is pinned (`2025-11-25`) in one constant, and the client declares no
   capabilities, so there is no sampling and no roots.

   **Evaluation (2026-09-26, under 2 h), against the bounds above:**
   - the footprint is two new lockfile entries, `rmcp` and `chrono` (MIT OR Apache-2.0); `cargo deny` gives advisories, bans and licences ok;
   - a fake stdio server was listed and called at the pinned version;
   - a 2 MB response line closed the transport instead of being buffered;
   - per-request timeouts (`PeerRequestOptions::with_timeout`) and cancellable requests (a handle whose `cancel` sends
     `notifications/cancelled`) are public API.

   One finding to handle in our code: rmcp's default stdio transport is **unbounded** (`usize::MAX`), so the bounded construction is
   mandatory, and a test pins it.

   **Amended 9 Nov 2026: 3.9.0.** Three advisories against 3.4.1 (RUSTSEC-2026-0457, -0461 and -0462) concern code this
   build doesn't compile (the OAuth client and both Streamable HTTP transports), but `cargo deny` refuses the version, so
   the pin moved instead of three ignores. rmcp 3.9.0 adds `sha2` 0.11 and `libc` to its own dependencies, both already
   in the lockfile. What reaches this client: a `tools/call` reply with no `content` (only `_meta`) is no longer read as a
   result, so it is a `protocol_error`; a server's `ping` during the handshake is answered; a message with both `result`
   and `error` no longer decodes, so our reader ends the stream there (`protocol_error` at start, `Crashed` for a pending
   call); and a `tools/list` that our handshake timeout drops is cancelled with a notice. A crash mid-call is `Crashed`
   at once in both versions (TOOL-I-012). The bounded construction is unchanged, and its tests pass.

## Consequences

- Any MCP server works without project code, and the conformance suite (TOOL-I-012, RT-I-008, SEC-011) is the acceptance evidence
  for a server or a change.
- A stdio server runs as the sandbox user and could read the runtime token, exactly like `shell`. ARCHITECTURE §12 accepts this:
  the token is scoped to one sandbox, bound to its address, and egress goes only to the gateway. Credentialed services belong to the
  connector tier.
- MCP resources, prompts and sampling are not supported. A server's request for sampling is refused.
- Protocol revision 7 is needed, so older runtime images are refused with `426 runtime_too_old` until rebuilt.
- The rmcp dependency is pinned exactly and reviewed like any other; a future transport change that removes the bounded
  construction would fail its test.

- rmcp's own `AsyncRwTransport` buffers each stdio line (with no bound before 3.6.0; since then 16 MiB by default, and
  an over-long line is dropped while the connection stays open) and silently skips lines that aren't
  JSON, so the runtime never uses it; rmcp is given only our transport, a `FramedRead` with
  `JsonRpcMessageCodec::new_with_max_length` (1 MiB) that ends the stream at the first over-long or non-JSON line
  (TOOL-I-012 pins both as `protocol_error`). Any rmcp upgrade must keep that construction and re-run those tests.

## Links

- PRD FR-9; ARCHITECTURE §5.4, §8, §12 and §15; ADR-0026
- TEST-PLAN TOOL-I-012, TOOL-I-013, TOOL-I-018, RT-I-008, SEC-011, SEC-013, E2E-MCP-006
- The stdio-tier design was approved on 2026-09-26
