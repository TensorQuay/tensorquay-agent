# Extensions

The agent's built-in tools cover files, commands, Git and the web. An installation adds more in three ways, and only
these: the gateway and the runtime load no plugins of their own (ADR-0011).

| Kind | Where it runs | Credentials | Set up in |
|---|---|---|---|
| Connector | A remote tool server (MCP), reached through the gateway | Held by the gateway, never seen by the agent | **Settings**, [Connectors](web-settings.md#connectors) |
| Tool server | A program inside the task's sandbox, speaking MCP over its standard input and output | None: it runs as the sandbox user, like `shell` | `[[mcp.servers]]` in `gateway.toml` |
| Skill | Instructions and helper files the agent reads on demand | None | A skills directory (`[skills]` in `gateway.toml`) |

Choose a connector for a service that needs a credential, such as an issue tracker. Choose a tool server for a local
program that needs none. Tools are not approved one call at a time: the guardrail is the sandbox and its
[Internet access](web-settings.md#internet-access) allow-list, which applies to extensions as it does to the built-in
tools.

## Connectors
A connector is set up on its settings page, and its credential is a shared secret that must allow it (see
[Who may use a shared key](web-settings.md#who-may-use-a-shared-key)); people may add their own credential instead. The
gateway sends the credential on each call, so the agent never holds it. See [Connectors](web-settings.md#connectors)
for the fields and [Keys](web-settings.md#keys-for-connectors) for people's own.

The gateway limits how many messages one person's tasks send through connectors: `calls_per_minute` under `[mcp]` in
`gateway.toml`, 60 by default. An administrator can change it without a restart on the [Limits](web-settings.md#limits)
page.

## Tool servers
A tool server is a program in the environment's image. The runtime starts it inside the sandbox for each session that
uses it, and offers its tools to the model as `mcp__<server>__<tool>`.

### Define one
Each server is one `[[mcp.servers]]` entry in `gateway.toml`. This example is illustrative, not a tested server:

```toml
[[mcp.servers]]
name = "docs"                     # shared with connectors' names; see below
command = "/opt/tools/docs-mcp"   # an absolute path in the image
args = ["--stdio"]
templates = ["workstation"]       # configured environments it runs in; empty means every one
tools = { allow = ["search", "open"] }
call_timeout_seconds = 30         # 1 to 600; 60 by default
enabled_by_default = false
```

- The name is 1 to 16 characters: a lowercase letter, then lowercase letters, digits or single underscores, not
  ending in one. No connector may have the same name.
- The command must be an absolute path shipped in the image, never under `/workspace` (the repository's files),
  `/run/tqa` or `/tmp`. Workspace files never start a server.
- No secrets: `env` takes plain settings only, because the server runs as the sandbox user, and anything it holds, the
  task can read. The gateway refuses an `env` name containing `key`, `token`, `secret`, `password` or `credential`,
  and `TQA_*` and proxy variables. A service that needs a credential belongs in a connector.
- The tool filter offers only the named tools (`allow`), or all but them (`deny`): one of the two, with 1 to 64
  distinct names.
- Every name under `templates` must be a configured environment, or the gateway refuses to start.
- An installation defines at most 8 servers, and one task holds at most 8 tool servers and connectors together.
- A server that crashes is restarted once; after a second crash it is off for the rest of the session, and the task
  carries on without it.

The gateway reads `gateway.toml` when it starts, so restart it after a change. Check the file first with
`tqa-gateway check` (see [Upgrading](README.md#upgrading)).

### Which tasks get one
A task gets the servers marked `enabled_by_default = true`, or those its profile chooses, within its environment. The
task's owner may switch catalogue servers on or off through the API for its next run; a task made from a profile that
sets its servers can only switch them off. On the Settings page, only administrators choose a profile's
**Tool servers** (see [Agent profiles](web-settings.md#agent-profiles)).

### Test one first
`tools/ext-check mcp stdio --template <image>` runs a server in a pinned image with no network, checks its handshake
and limits, lists its tools, and writes a report; with `--calls <file>` it also calls them. The
[catalogue of tested extensions](../catalog/README.md) explains how.
The catalogue lists servers and templates that passed, each with the `[[mcp.servers]]` entry to copy. An entry records
one tested version; it is not a security review.

## Skills
A skill is a folder with a `SKILL.md` and optional helper files. The agent sees each skill's name and description, and
reads the full skill with the `skill` tool when it needs it (ADR-0029). Skills come from two places: an installation
tier that an administrator provides, read-only, and each person's own tier, which needs no setup. Setting up the
installation tier is in the deployment README (`deploy/README.md`, "Skills"). A profile can narrow which skills are
offered: **All skills**, **No skills** or **Only these skills**.

## Decision engines
The full guide is [Decision engines](decision-engines.md), with its [API reference](decision-engines-api.md).
A decision engine adds no tools. It is a service of your own that the agent consults once per step, before it calls
the model and after the model answers, to narrow what the step may do: a cheaper route, fewer tools, or a stop
(ADR-0013). It speaks `decision/v1`, and the gateway relays to it with the credential, so the agent never holds it.
An outside multiple-choice service can also serve as one, choosing the model route before each step; the gateway
translates for it (see [A multiple-choice service](decision-engines.md#a-multiple-choice-service)).
An engine is defined in `gateway.toml`, on the [Decision engines](web-settings.md#decision-engines) settings page, or
through the administration API (`/api/v1/admin/decision/engines`). This example is illustrative:

```toml
[[decision_engines]]
name = "router"                        # a lowercase letter, then lowercase letters, digits, - or _; up to 64
display_name = "Policy engine"         # optional: what people see
url = "https://engine.internal"        # the gateway posts to <url>/v1/before_step and /v1/after_response
auth = { scheme = "bearer" }           # or "header" with header = "<name>", or "none"
key_ref = "secrets/decision-engine"    # a shared secret; the file allows the engine to use it
timeout_ms = 2000                      # 1 to 10000; 2000 by default
exposure = "none"                      # what the engine sees: "none", "calls" or "full"
```

- `exposure` decides how much of a task reaches the engine. With `none` (the default) it sees each call's name and
  id only; `calls` adds the calls' arguments; `full` adds the model's text and the conversation. The gateway refuses
  anything more, even if it is asked directly. Under every exposure the engine also sees the task's tools with their
  descriptions, so point `url` only at a service you trust.
- Every change of an engine's exposure is recorded in the administration audit with the old and new value. A task
  already running keeps the exposure it started with until its next session: after you narrow an engine, such a task
  consults it without success and the agent decides alone until then; after you widen one, the engine sees more from
  the next session.
- An engine that a profile or an open task names can't be deleted; disable it instead (`enabled = false`), and its
  tasks are refused or told, never run without it.
- `insecure_http = true` allows a plain `http` URL, and `ca_file` names a private certificate authority. Both are
  for the file only.
- A task uses an engine only when its profile names one in `decision_engine`; by default none does. Such a task
  needs this version's runtime image (see [Upgrading](README.md#decision-engines-need-this-versions-runtime-image-2026-09-28)). If the profile
  names an engine the installation no longer has, or has disabled, the task's next message is refused rather than
  run without it. If the engine goes while a task waits to start, the task is told and its sandbox stops; restore or
  re-enable the engine, and a message continues the task.
- Each consultation counts against the person's relay rate, and the **Usage** page shows it as `decision:<name>`.
- An engine that fails, answers late or answers with more than 64 KiB never stops a task on its own: the agent carries
  on as if there were no engine for that step, and records why.

### Test an engine first
A short reference engine to start from is in `examples/decision-reference`: one Python file, standard library only,
that sends routine steps to a cheaper route (see its README).

`tools/ext-check decision <engine base URL>` asks your engine what a task would ask, at both points and at each of the
three exposure levels, and checks each answer with the agent's own rules. A pass means the agent would apply the
answer; a failure names what the agent would record instead: `route_not_allowed`, `tool_not_allowed`, `schema`,
`invalid`, `unreachable` or `timeout`. An engine that depends on something a level leaves out fails at that level.
The report also gives the latency (p50 and p95, and the first consultation's on its own, since an engine must answer
without a warm-up).
- The engine must be on this machine (`localhost` or a loopback address) unless you add `--network`; a remote engine
  needs `https`.
- `--bearer-from-env ENGINE_KEY` sends the value of that environment variable as a bearer token. The value is never
  printed or written to the report, and the command line takes no credential itself. Only a bearer token can be
  sent: an engine whose key goes in another header (`auth = { scheme = "header" }`) is checked without its key, for
  example on a copy that accepts requests without one.
- `--timeout-ms` is the time for each answer, 2000 by default and at most 10000, as in the engine's settings.
- `--json <file>` also writes the report as JSON. The command exits 0 when everything passed and 1 when something
  failed.
