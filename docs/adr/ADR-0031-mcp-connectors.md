# ADR-0031: MCP connectors through the gateway proxy

## Status

Accepted; amended 2026-09-28 (a server's text reaches the model as untrusted content)

## Date

2026-09-27

## Context

ADR-0011 makes MCP the only plugin protocol, with two trust positions: stdio servers inside the sandbox without secrets, and
connectors outside it behind the gateway, with credentials in the vault. It decides the stdio tier and the revision-7 entry both
tiers share, and leaves the connector tier to this record.

PRD FR-9, FR-22 and FR-23 ask for remote MCP servers registered by an administrator, reached through the gateway with shared or
per-user credentials that are write-only. SEC-013 requires that a credential never reaches the sandbox.

Two facts shape the design:
1. **The runtime token is readable by any process in the sandbox** (ADR-0011 Consequences; ARCHITECTURE §12). Sandbox code can call
   the proxy directly, without the runtime, so whatever must hold for connector calls is enforced by the proxy.
2. **The connector URL is administrator input, and the remote server is not ours.** The proxy is new egress from the gateway, and a
   remote can redirect, rebind DNS or answer without bounds.

## Decision

1. **Endpoint and caller.** The proxy is `/mcp/{connector}` (MCP streamable HTTP: `POST`, the `GET` stream, `DELETE`) on the
   gateway's main listener, next to the relay, since both backends' sandbox egress reaches only that address.
   - It accepts only a runtime token, from the peer address bound to it (the relay's rule). Session cookies and personal API tokens
     are refused, so a stored credential is only ever used on behalf of a live sandbox.
   - An unmatched path or method under `/mcp/` answers the API's JSON errors, never the web application's page fallback.

2. **The proxy is the enforcement point.** On every request it checks, independently of the runtime:
   - **The conversation's connector set (fail-closed).** A connector is admitted only if the conversation's own set lists it and
     it exists and is enabled. Anything else, an unknown name included, is `403 connector_not_allowed`, so names can't be
     enumerated.
   - **The method:** `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call` or `notifications/cancelled`.
     Anything else is answered with JSON-RPC `-32601`, and bodies are parsed as JSON-RPC before anything is forwarded.
   - **The tool filter:** `tools/list` results are filtered by the connector's allow or deny list, and a `tools/call` for a
     filtered tool is answered with `-32602`.
   - **The gateway's own policy.** Every `tools/call` is classified with the gateway's `[permissions]` policy on the one tqa-tools
     engine (`ToolName::mcp`, `classify`), with the connector's class tightened by the tool's hints (Decision 7).
     - `deny` is refused.
     - `allow` is forwarded without a binding.
     - `ask` is forwarded only with an approval the gateway recorded (the `git_push` pattern, SEC-019):
       - the call travels alone (not in a batch) and names its journaled call in `Tqa-Run-Id`, `Tqa-Step`, `Tqa-Attempt` and
         `Tqa-Call-Id`;
       - the gateway loads that exact `tool.call` and requires its name to be `mcp.<connector>.<tool>` and its arguments to equal
         `params.arguments` (canonical JSON);
       - it then needs one of:
         - the conversation owner's **allow-once** for the call's prepared-action hash (`PreparedAction`), used by at most one
           proxied call: it is found first and consumed last, after the rate, credential and in-flight checks, with
           `UPDATE … SET proxied_at WHERE proxied_at IS NULL`, so a transient refusal never burns it and a replay or a
           concurrent twin is refused;
         - or the owner's **session grant for this same tool** (`grant_matches`: tool, class and scope).
       - A grant for another tool (such as `shell`), a runtime `permission.resolved`, or the token alone never runs an
         `ask`-class connector tool.
   - **Refusals** the proxy answers itself are JSON-RPC errors (`-32003`, HTTP 200) with a stable `data.code`: `tool_denied`,
     `approval_required`, `call_not_journaled` or `approval_missing`. They never reach the remote and cost no call.
   - **Headers are an allow-list:** only `Content-Type`, `Accept`, `MCP-Protocol-Version` and the session header go out, and only
     `Content-Type` and the session header come back. The runtime token, cookies, the `Tqa-*` headers and any inbound credential
     never reach the remote, and `Set-Cookie` or authentication challenges never reach the sandbox.

3. **Credentials.**
   - **Resolution order:** the user's own credential for (user, connector), then the connector's shared `secret_ref`, then none.
     The user is the conversation's owner.
   - **Required credentials:** a connector with `auth.required = true` and no credential fails with
     `424 connector_credential_missing`.
   - **A refused credential (13 Oct 2026):** when a call carried the user's own credential and the remote answers it
     `401` or `403`, the proxy answers `424 connector_credential_rejected` instead of relaying the remote's status, so
     the person is told to check their key rather than that the connector is unreachable. A shared credential's
     refusal stays the remote's answer: only an administrator can fix it.
   - **Schemes are data:** `none`, `bearer`, or `header` with `Authorization`, `X-Api-Key` or an `X-*` name of at most 64
     characters. **OAuth is out of scope for v1**, and static header credentials are the only scheme. OAuth would need an
     authorization-code flow, refresh and per-user consent, so it gets its own record.
   - **Origin binding.** A user's credential is bound to the connector's URL origin (`scheme://host[:port]`) when it is set, and
     counts only while the connector still has that origin. A connector repointed to another server, or deleted and re-created
     under the same name, never receives a user's earlier credential. Deleting a connector deletes users' credentials for it.
   - **Values** are read from the secret store for each call (a rotation applies at once) and held as `SecretString`. They never
     appear in logs, errors, events, metrics or URLs. A secret-store outage fails the call closed.
   - **Credential APIs:** per-user credentials are written through `PUT`/`DELETE /api/v1/connectors/{name}/credential` and listed
     only as `has_credential` (the git-credentials pattern). Shared ones go through the write-only secrets admin.

4. **MCP sessions are bound to a conversation.** The proxy never hands the remote `Mcp-Session-Id` to the sandbox.
   - It issues an opaque id mapped in memory to (conversation, connector, remote session, credential source).
   - An id presented by another conversation or connector, or after the credential source changed, is answered `404`, the spec's
     unknown-session reply, so the client re-initialises.
   - At most 8 sessions per conversation (the least recently used goes) and 10 minutes idle.
   - One `GET` stream per session, ended after 5 minutes without an event.
   - `DELETE` is forwarded and forgets the entry.
   - A gateway restart drops the map, which clients handle as `404`.

5. **Outbound connections** (new egress from the gateway):
   - `https` only, with TLS verification always on. Plain `http` and a private `ca_file` are allowed only in the file seed, for
     tests and on-premises servers, and the admin API refuses both.
   - **Pinned addresses:** the host is resolved once, and a loopback, private, link-local, shared, unique-local or otherwise
     reserved address (IPv4 embedded in IPv6 included) is refused unless the URL names that address as an IP literal. The client
     then connects only to the checked addresses, so DNS can't rebind a name to an internal address in between.
   - **Redirects** are never followed (`502 connector_redirect`), and the system proxy is never used.

6. **Bounds** (named constants unless marked as a limit key):

   | Bound | Value |
   |---|---|
   | Connectors | 64 |
   | A request | 1 MiB; a batch holds at most 32 messages |
   | One SSE event or JSON answer from the remote | 1 MiB; a larger JSON answer is `502 connector_answer_too_large`, and a larger event ends the stream with a JSON-RPC error |
   | Connect plus TLS | 10 s |
   | A call | the connector's `call_timeout_seconds` (default 60, at most 600); on expiry `504 connector_timeout` and `notifications/cancelled` upstream |
   | Concurrency | 4 per (conversation, connector) and 256 through the proxy; beyond that `429 connector_busy` |
   | Rate | the ADR-0028 limit key `mcp.calls_per_minute` (file: `[mcp] calls_per_minute`; default 60 per user, 1–6000, admin floor 1); one per message forwarded; beyond it `429 rate_limited` with `Retry-After` |

   When the sandbox drops a request (run cancelled, sandbox retired), the proxy drops the upstream request and sends
   `notifications/cancelled` for an in-flight call, as a bounded best effort.

7. **Connector registry (the ADR-0028 pattern).**
   - **Storage:** a `connectors` table seeded from `[[connectors]]` in `gateway.toml`, with keyed seed digests, tombstones,
     versioned rows, one settings-generation bump per write, `admin_audit`, and the connectors in the settings snapshot.
   - **Fields:**
     - `name`: `[a-z][a-z0-9_]{0,15}` with no `__` and no trailing `_`, unique across the stdio catalogue and the registry
       (`409 name_taken`);
     - `display_name`;
     - `url`: https, at most 2 KiB, with no user information, fragment or credential-like query keys;
     - `auth` and an optional shared `secret_ref`;
     - `class`: default `execute`, because the tools act with a stored credential; an administrator may set any class;
     - `tools`: an allow or deny list of at most 256 names;
     - `call_timeout_seconds`, `enabled` and `enabled_by_default`.
   - **Hints.** MCP annotations from the remote are untrusted, and classes are categories, not an order. So a hint moves a tool
     only between the classes it describes: `destructiveHint` moves `read` to `write`, and `openWorldHint` moves `read` or `write`
     to `network`. `execute`, `network` and `communication` never move.
     - One implementation, `tqa_tools::tighten`, serves the runtime client and the proxy.
     - The proxy records the hints of every tool in the `tools/list` answers it relays, per connector and tool. A hint once seen
       stays set.
     - A tool with no record is classed as if both hints were set.
   - **Admin API:** `/api/v1/admin/connectors` offers `GET`, and `PUT` and `DELETE` on `/{name}` with a `version` and an
     Idempotency-Key. File-only fields are refused.
   - **User API:** `GET /api/v1/connectors` lists the connectors with `auth.required` and `has_credential`.

8. **A conversation's connectors and delivery.**
   - A conversation stores its own connector set. It starts with the enabled connectors marked `enabled_by_default` (a profile's
     set once profiles exist, GW-I-025), within what its stdio servers leave of the runtime's bound of 8.
   - `PATCH /api/v1/conversations/{id}` `{connectors}` changes the set to distinct existing connectors, at most 8 together with the
     stdio servers. It is refused with `409 run_active` during a run.
   - Registry changes never widen an existing conversation. A disabled or deleted connector stays in the set but is left out of the
     next session at once, and the proxy refuses it.
   - The set reaches the runtime as the `gateway { connector }` entries of `session.config.mcp` (revision 7, ADR-0011), after the
     stdio servers. No URL is on the wire: the runtime uses its own gateway origin, `<origin>/mcp/<connector>`.

9. **Errors and observability.**
   - **HTTP error codes:**
     - `connector_not_allowed` 403;
     - `connector_credential_missing` and `connector_credential_rejected` 424;
     - `connector_busy` and `rate_limited` 429;
     - `connector_unavailable`, `connector_redirect` and `connector_answer_too_large` 502;
     - `connector_timeout` 504;
     - `store_unavailable` and `secret_store_unavailable` 503;
     - `payload_too_large` 413.

     The runtime client reports a failure in `tool.result` as `<code>: <sentence>`. The proxy sends its code in the
     `X-TQA-Gateway-Error` header as well as the body (GW-I-148); the client takes the code from the header only, and a
     refusal without it is `mcp_error`, so an answer the proxy forwards can't pose as one of the proxy's codes.
   - **Metrics:** `tqa_mcp_proxy_requests_total{connector,method,outcome}` with bounded labels.
   - **Logs:** a WARN line per upstream failure or policy refusal, with the connector, the method, the reason and the credential
     *source* (user, shared or none), never values or bodies.

10. **A server's text is untrusted content** (amended 2026-09-28). Connectors carry third-party text (an issue body, a page,
    a file) straight into a run, so everything an MCP server says reaches the model inside the untrusted-content block of
    ADR-0014 point 10, which the text can't close (SEC-034's marker). This covers connectors and stdio servers alike.
    - **What is marked:** a result's text (a result the server marks with `isError` too, which stays a failed result), a
      JSON-RPC error answer's message, and a stdio protocol error's detail. The block's source is `mcp`, with the
      registry's `server` and `tool` names as attributes. A stable code stays outside the block (`<code>: <block>`).
    - **What isn't:** the runtime's and the proxy's own failures (`<code>: <sentence>` from HTTP errors, timeouts, a
      crashed server), which carry no server text.
    - **Where:** the runtime marks at the one place every MCP call returns (the registry tool). The proxy relays a
      remote's answers verbatim, so it doesn't mark them; it keeps its refusal code its own instead. A remote's error
      answer with `-32003`, the code of the gateway's refusals, is rewritten to a plain `-32603` without its `data`,
      and its message kept, so a remote can't pass for a gateway refusal.
    - Capping a long result keeps its head and its tail (CTX-E-004), so both tags survive.

## Consequences

- An administrator adds a service without project code, and the core ships no connectors.
- SEC-013 holds by construction: the sandbox sees only the runtime token, and only the gateway's upstream request carries the
  credential.
- The runtime client must send the four `Tqa-*` headers with every `tools/call` to a `gateway` entry; without them an
  `ask`-class call is refused. This is a client contract, not a protocol frame, so revision 7 is unchanged.
- A remote that echoes the credential back in a response would expose it to the sandbox. This is accepted: the proxy doesn't
  rewrite remote bodies, and administrators register only trusted servers.
- OAuth-only MCP servers can't be connected in v1.
- A server's answers reach the model marked, so they can't pose as instructions, though their content still informs the
  model (Decision 10).
- A gateway restart makes clients re-initialise their MCP sessions.
- Tests:
  - GW-U-036 to GW-U-039, GW-U-042 to GW-U-044 and GW-U-046 (validators, JSON-RPC and header filters, SSE framing, sessions,
    rate, call context, session entries);
  - TOOL-U-027 (hints);
  - GW-I-024 (credentials) and GW-I-067 to GW-I-073 (delivery, registry, proxy, sessions and bounds, rate, the connector set,
    approvals);
  - SEC-024 (connector SSRF);
  - SEC-040 (a server's text is marked and can't close its marker);
  - TOOL-I-018 and SEC-013 on the stack, and WEB-E2E-010's connector half.

## Alternatives

- **Give the sandbox a scoped credential.** Rejected: SEC-013, and no secrets in the sandbox.
- **Enforce the connector set and approvals only in the runtime.** Rejected: sandbox code holds the token, so a session grant for
  `shell` would silently grant every connector tool.
- **Connectors through the policy proxy (CONNECT) with header injection.** Rejected: that needs TLS interception to inject a
  header, and the proxy could not see MCP methods or tools.
- **Pass the remote `Mcp-Session-Id` through.** Rejected: one conversation could drive a session authenticated with another
  user's credential.
- **Trust annotations to set the class.** Rejected: they come from the remote and could loosen a tool's rules.
- **OAuth in v1.** Deferred (Decision 3).

## Links

- PRD FR-9, FR-22, FR-23; ARCHITECTURE §4.2, §12, §15; ADR-0011, ADR-0026, ADR-0028
- TEST-PLAN: the IDs under Consequences; SEC-019 for the binding pattern
