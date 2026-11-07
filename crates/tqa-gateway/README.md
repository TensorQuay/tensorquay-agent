# tqa-gateway

Authenticated HTTP control plane, PostgreSQL event journal and runtime WebSocket link. Dependencies point to
`tqa-types`, `tqa-llm` and `tqa-sandbox`; the gateway does not link the agent core. It owns the sandbox manager, transparent model relay and encrypted secret store. Fair scheduling and the full reaper are later work.

## Configuration

Run `cargo run -p tqa-gateway -- gateway.toml`; the argument defaults to `gateway.toml`. Configuration is exact TOML:
unknown fields, credentials in TOML, invalid origins, path traversal and out-of-range limits fail startup. The
configuration file is limited to 64 KiB; model registries use `tqa_llm::Registry` and its 1 MiB cap.
See [gateway.toml](../../deploy/examples/gateway.toml) and [models.toml](../../deploy/examples/models.toml).

```toml
listen = "127.0.0.1:8080"
public_origin = "http://localhost:8080"
models_path = "deploy/examples/models.toml"
session_ttl = 86400
event_payload_cap_bytes = 65536
stream_keepalive = 15

[storage]
root = "/var/lib/tqa"

[runtime]
backend = "docker"
```

`listen`, `public_origin`, `models_path` and `storage.root` are required. The example shows the base defaults;
`runtime.image` must specify an immutable pin before creating a sandbox: `name@sha256:<64 lowercase hex digits>`
for a registry image, or `sha256:<64 lowercase hex digits>` for an image built on the same host. Tags alone are
rejected. Local IDs are checked against Docker's image ID and are never pulled; registry references retain their
manifest-digest verification. Both forms participate in administrator readiness checks.
Durations are seconds. Session TTL accepts 60–2678400; keepalive accepts 1–300; inline event payload cap accepts
1–262144 bytes. Storage must be absolute, non-root, traversal-free and writable by the service account.
Existing storage directories must deny group/other access; new directories use mode 0700 and blobs use mode 0600.
Workspaces, event blobs, runtime state and runtime credentials have separate directories under the storage root.
See [the one-box deployment](../../deploy/README.md) for the image, private key/seed mounts and Docker networking.

`[runtime] backend = "firecracker"` selects the microVM backend (ADR-0006; host setup in
[the Firecracker guide](../../deploy/firecracker/README.md)). It requires a `[firecracker]` section, and a Docker
install refuses one:

```toml
[firecracker]
vmm_image = "sha256:<the images/vmm image ID>"   # pinned like runtime.image
gateway_address = "192.0.2.10"                   # the gateway's pinned address on [sandbox] network
# kernel_dir = "<storage.root>/kernel"           # vmlinux and its manifest
# rootfs_dir = "<storage.root>/images"           # one <image id>/ rootfs per template
# gateway_ports = [8080]                         # default: the port of [sandbox] gateway_url, plus the policy proxy's
# workspace_bytes = 8589934592                   # deprecated and ignored (still checked, 64 MiB to 2 TiB): a new
                                                  # workspace image is sized from [workspaces] quota_bytes
# vmm_file_uid = 10000                           # the VMM's uid as the gateway sees it
# vmm_overhead_bytes = 536870912                 # the VM host's memory above the guest's, for the VMM and the host
                                                  # page cache of the guest's disk writes (64 MiB to 8 GiB)
```

`[sandbox] gateway_url` must name the gateway by host name, because a guest resolves it only through a static host
entry. On this backend the workspace and the runtime state are ext4 images, `workspaces/<id>.ext4` and
`runtime-state/<id>.ext4`, which the backend creates for the first sandbox. The gateway never creates a workspace
directory, and it never mounts or reads those images. The startup preflight refuses to start without the VM host
image, the internal sandbox network, `/dev/kvm` access for the VMM uid, a tap device, ACL-capable storage and a
kernel matching its manifest.

Optional `[skills]` names the install tier of read-only skills, an operator directory that every Docker sandbox
mounts at `/skills/install`. Each conversation also mounts its **owner's** own tier, `<storage root>/skills/users/<user
id>/`, at `/skills/user`. The gateway creates it empty and private on first use; a user who shares a workspace never
exposes their tier to the sharer's conversations. Both mounts are read-only.

```toml
[skills]
root = "/srv/tqa-skills"   # absolute; must exist; bound into the gateway container at the same path
```

The root is refused at load when it is relative, traverses, or overlaps the storage root. At startup the gateway resolves
the root and each protected location (the storage root and the directories of the install key, the seed file and the
model registry), then refuses any overlap in either direction, so a symlink cannot hide one. Keep secrets and gateway
configuration out of the tree: Compose binds files by different paths inside the gateway container, so an install
directory placed inside the skills root is invisible to this check (deploy/README.md, "Skills"). The sandbox side
resolves each tier again before mounting. The microVM backend mounts no skills yet; a Firecracker install checks the root and
leaves both tiers empty.

Optional `[limits]` overrides set global resource ceilings, not per-user policy:

`password_seconds` bounds one password check or hash, its wait for one of the `password_workers` included; the
router gives sign-in, the first administrator, an invitation's acceptance and a password change that bound per
password operation plus `io_timeout`. Each worker has eight waiting places; a call past them gets `429 busy` at once.
Start-up's one hash waits up to 300 s whatever it says.

| Setting | Default | Allowed |
| --- | ---: | ---: |
| `database_connections` | 16 | 1–64 |
| `io_timeout` (seconds) | 10 | 1–300 |
| `body_bytes` | 1048576 | 1024–16777216 |
| `ingestion_bytes` | 16777216 | 262144–16777216 |
| `broadcast_capacity` | 1024 | 1–65536 |
| `action_capacity` | 64 | 1–4096 |
| `live_conversations` | 4096 | 1–65536 |
| `login_addresses` | 10000 | 1–100000 |
| `password_workers` | 4 | 1–16 |
| `password_seconds` | 30 | 1–300 |
| `page_size` | 1000 | 1–1000 |
| `idempotency_hours` | 24 | 1–8760 |
| `idempotency_purge_seconds` | 60 | 1–3600 |
| `token_reservation_secs` | 120 | 1–600 |

Environment-only credentials are `TQA_DATABASE_URL` and optional `TQA_BOOTSTRAP_TOKEN` (32–1024 bytes). The process also requires `TQA_INSTALL_KEY_FILE`, and accepts an optional `TQA_SECRETS_SEED_FILE`; both are private mode 0600 files. It
does not read `DATABASE_URL`; SQLx build/test tools use that alias. JSON logs use a fixed `info` filter with `bollard`,
`hyper` and SQLx at `warn`. Request traces contain route templates and methods, never credentials or payloads.

Migrations run before serving. `/healthz` reports process health; `/readyz` verifies database access and each applied
migration's version, success and checksum. Metrics use a separate listener, `metrics_listen` (default
`127.0.0.1:9464`; set it to `false` to disable). The public listener returns 404 for `/metrics`.
The metrics listener exposes request counters/duration histograms, terminal run
counters, event lag histograms, `tqa_relay_queue_seconds{route}` (how long relayed requests waited for their model
route's limiter), `tqa_relay_throttled_total{route,scope}` (the relay's own throttles, `scope` being `user` or
`route`), `tqa_relay_unattributed_total{route,reason}` (relayed calls recorded with no run: `missing`, the call named
none, or `foreign`, the run it named isn't one of its task's) and `tqa_secrets_rotation_due` (shared secrets past their rotation reminder), with bounded labels: never a user
or conversation. A secret's optional reminder (`PUT /api/v1/admin/secrets/{name}/reminder
{rotate_after_days: 1-3650 | null, version}`, versioned and audited) makes it due once its value has not changed for
that many days: the admin list shows `rotated_at`, `rotate_after_days` and `rotation_due`, a WARN names each due
secret once a day (checked at start and hourly), and nothing stops working because a reminder is due.
Every read of a secret's value (a route's key by the relay or a probe, a connector credential by the MCP proxy, a host
key by the egress proxy) is recorded without the value: the reference, the consumer, the conversation, user and run, and the
outcome (`ok`, `absent`, `unavailable`), counted in one row per such key. Records go through a bounded queue
(`[secret_store] read_audit_queue`, default 1024) to one writer that commits them in batches. No key is used without
its row: a read that found a value waits for its batch to commit, a full queue makes reads wait rather than dropping
records, and a read whose row can't be written in time fails with `key_use_unrecorded` ("The key could not be used:
its use could not be recorded. Try again.") and counts `tqa_secret_reads_unrecorded_total`. `GET /api/v1/admin/secrets/{name}/reads` shows a shared secret's
reads to administrators; `GET /api/v1/connectors/{name}/credential/reads` shows
users the reads of their own credentials (all newest first, `limit` 1-100 and `before` to page). Read records are
kept `read_audit_days` (default 30, 1-365) and administration audit rows `admin_audit_days` (default 365, 1-3650), swept
hourly.
A shared secret is used only by the routes and connectors it is bound to: `PUT /api/v1/admin/secrets/{name}/consumers
{consumers: [{kind: route|connector, name}], version}` (versioned, audited) sets them, and the admin list shows them as
`consumers`. A route or connector saved through the API naming an existing secret not bound to it is refused
`409 secret_not_allowed`; one naming a secret that does not exist yet binds it. A route or connector from the files
(models.toml, `[[connectors]]`) binds its secret when seeded, and the upgrade bound each secret to the consumers naming
it then. A read for a consumer the secret is not bound to fails closed (`secret_unavailable`, logged by name). Users'
own credentials are never bound: only their owner's calls use them. Shutdown cancels streams and links, joins owned tasks and
closes the database pool. SIGINT and SIGTERM both initiate this shutdown.

`GET /readyz?detail=1` requires an administrator and adds backend preflight, network mode, image presence and secret-store checks. No dependency credentials or host paths appear in the report.

## Authentication and HTTP contract

[openapi.json](openapi.json) describes the public operations and is checked against the route table.

- Bootstrap accepts `{email,password,token}` at `POST /api/v1/auth/bootstrap` and creates the first administrator
  once, including concurrent attempts. The configured token becomes unusable once any account exists.
- Login accepts `{email,password}` at `POST /api/v1/auth/login`. The 256-bit random `tqa_session` cookie is stored
  as SHA-256; it uses `HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age=session_ttl`, and `Secure` for HTTPS origins.
  `/api/v1/auth/me` returns metadata; `/api/v1/auth/logout` revokes the session.
- Passwords require 12 characters and at most 1024 UTF-8 bytes; the normalized email and a small common-password
  list are rejected. Argon2id v19 uses 19456 KiB, two passes, one lane, 32-byte output and a random 16-byte salt.
  Verification uses the library's constant-time comparison and bounded blocking workers.
- Workspaces (task 066a, FR-30). A task binds to one workspace for its whole life, and its sandbox mounts that
  workspace's directory (`workspaces/<workspace id>`). The task's journal and runtime state stay per task.
  `POST /api/v1/conversations` takes an optional `workspace_id` (a workspace you own or hold a `write` share on);
  without one, a new empty workspace is created with the task's own id, so the directory is the same as before.
  Migration 0026 turned every existing task's directory into a workspace of the same id, so no files moved.
  - `/api/v1/workspaces`: list (own and shared, with `access`), create (`{name, profile?, template?}`: an empty
    workspace; `git_url` was removed and is `400` with reason `removed`), get, rename, delete, and
    `/shares/{user_id}` (owner only). `read` shares see the workspace, read its files and fork it; `write` shares
    may also bind their own tasks and run on it. Sharing never shares tasks or transcripts. Everyone else gets 403.
  - Administrators list metadata only (`?all=true`): no names, no files.
  - Delete (owner): refused with `409 workspace_in_use` while a bound task has a sandbox, and while active tasks are
    bound unless `?with_conversations=true` and they are all the owner's (then they are deleted too). The host
    directory is removed; symbolic links in it are removed, never followed.
  - The write lease (task 066b, migration 0027). A workspace is mounted read-write by at most one sandbox: the one
    holding its lease (`workspaces.lease_conversation_id`). The lease is taken with the sandbox record, renewed by
    runtime heartbeats and released with the record. Another task's message is still accepted (202) and waits with
    `queue.reason = "workspace"`. When the holder has no active run and is not pinned, it is retired for the waiter
    at once. A holder whose run is active is never interrupted, unless its lease lapsed (no heartbeat for
    `[workspaces] lease_ttl_seconds`, default 90), in which case it is presumed dead and handed over.
    `[workspaces] busy_policy = "refuse"` answers `409 workspace_busy` instead of queueing while the holder runs.
  - Fork (task 066c, migration 0029). `POST /api/v1/workspaces/{id}/fork {name}` needs any access (a read share
    included). It answers `202` with the new workspace, owned by the caller, in state `forking`. The copy runs in
    the background and never waits for the lease. The source is walked one directory descriptor at a time with
    `O_NOFOLLOW`: links are copied as links, never followed, and other special files are skipped. Each file is
    written under a temporary name and renamed. The fork becomes `ready` with its size, or `failed` with nothing
    kept when it would exceed the quota or 1,000,000 entries. A fork of a workspace whose sandbox was live is marked
    `fork.consistent: false`. A workspace still `forking` (or `failed`) cannot be forked, bound, run on or deleted
    (`409 workspace_not_ready`). A fork running when the gateway stopped is marked `failed` at start-up.
  - Quota: `[workspaces] quota_bytes` (default 10 GiB, 1 MiB-1 TiB). A workspace is measured (allocated blocks, and
    at least each file's length, so a sparse file counts in full; no links followed) whenever a sandbox lets go of it,
    and after a fork; `size_bytes` and `size_measured_at` are in the workspace view. A message for a workspace over its
    quota is refused with `409 workspace_quota`, and nothing is queued.
  - Live quota on Docker (N4): `[workspaces] live_check` (default on), `check_seconds` (default 30, 5-600) and
    `margin_percent` (default 10, 1-100; the margin is kept within 256 MiB-4 GiB). While a run is in progress its
    workspace, and its conversation's runtime state directory (`[sandbox] state_bytes`, default 1 GiB, 16 MiB-1 TiB),
    are measured with the same bounded walk on the blocking pool, at most two at once gateway-wide: every
    `check_seconds`, every 5 s from four fifths of the limit, sooner when it grows fast (never sooner than 2 s, nor
    than ten times the last walk took). Past the quota, open streams get one retryable `workspace_quota` notice per
    run and the run continues. Past the quota plus the margin (or the state past `state_bytes`), the run is cancelled
    (`run.cancel`), streams get a non-retryable `workspace_quota` (or `state_quota`) notice, and the size is recorded
    so new messages get `409 workspace_quota` at once; a runtime that has not ended the run two `io_timeout`s later
    loses its sandbox, without a replacement. Nothing in the workspace is deleted. This bound is best effort: a fast
    writer can overshoot by about its write rate times the interval. Firecracker's block image is its hard bound and
    is not measured during a run.
  - Retention: `[workspaces] retention_days` (default 0 = keep for ever, up to 3650). When on, an hourly sweep
    deletes workspaces unused for that long with no active task bound and no lease, rechecking each under its row
    lock; their files are removed.
- Team accounts (task 063). Roles are `admin` and `member`; accounts are disabled, never deleted. Disabling deletes
  the account's sessions and revokes its tokens at once, and later logins fail like a wrong password. The last
  enabled administrator cannot be demoted or disabled (`409 last_admin`). The user list shows each account's `last_active_at`
  (its last authenticated request, written at most once a minute). Administrators see other users' task
  metadata only: never titles, events or files.
  - Invitations: `POST /api/v1/invitations {email, role}` returns a single-use token once (the join page is
    `/join/{token}`), valid for `limits.invitation_hours` (default 168, 1-720). The token is bound to the email and
    stored only hashed. Public `POST /api/v1/invitations/lookup` and `/accept` share the login limiter; every
    unusable token gets the same `400 invitation_invalid`.
  - Personal API tokens: `POST /api/v1/tokens {name, expires_in_days}` (1-365, required) returns
    `tqa_pat_` + 43 base64url characters once; only the SHA-256 and the last four characters are stored. A token
    acts as its user on every `/api/v1` operation. A request with `Authorization` is authenticated only by that
    token: cookies are ignored, and no `Origin` is needed, because a bearer header is never an ambient credential.
    Token and user administration (`/api/v1/tokens`, `/api/v1/users`, `/api/v1/invitations`) need a session: a
    token gets `403 session_required`. Runtime tokens never authenticate `/api/v1`, and personal tokens never
    authenticate `/runtime` or `/relay`. After 30 failed token authentications in a minute from one address, the
    address gets `429 busy` for the rest of the minute. `last_used_at` is written at most once a minute.
- Login lockout (SEC-009): ten attempts per minute per transport address, successful or not. Further attempts in
  that minute, including one with the right password, get `429 busy`. At most `limits.login_addresses` addresses
  are tracked at once, and a new address beyond that also gets `429 busy`. Forwarded-address headers are not
  trusted.
- A request body over its route's limit (`limits.body_bytes`, or `relay.body_bytes` for the relay) is refused from
  its `Content-Length`, before any parsing, with `413 body_too_large` (SEC-012). A body of exactly the limit is
  accepted. `relay.body_bytes` defaults to 16 MiB (1024-16777216), enough for screenshots sent to vision models.
  Every `session.config` and route `session.update` carries it as `route.max_request_bytes`, so the runtime keeps
  its model requests within it (task 071).
- A route in models.toml may name its `display_name` (GW-I-216): the seed stores it as the route's display name, never
  in its record, by the routes API's rule (1-80 characters on one line, no surrounding spaces; otherwise the gateway
  doesn't start). It is part of the seed's digest only when set, so a route without one is never seeded again for it,
  and a file without one leaves an administrator's name as it is.
- A model route may declare `concurrency` (requests in flight, streams included) and `requests_per_minute`
  (LLM-U-012). The relay holds one limiter per route across every conversation and user. Requests wait in arrival
  order, for at most `relay.queue_wait` seconds (default 30, 1-120), with at most `relay.queue_max` waiting (default
  64, 1-10000). Beyond either bound, the answer is `429 rate_limited` with `Retry-After`, and the model is not
  contacted. A refused request has no usage row, and a request holds its permit until its response ends.
- Each user has one relay rate, shared by all of the user's conversations and routes: a burst of `relay.capacity`
  requests, refilled at `relay.refill_per_second`. Unset, they follow the runs one user may have live
  (`admission.per_user`): 60 requests and 1 a second for each, so a user driving 16 runs at once gets a burst of 960
  and 16 a second (at most 100000 and 10000). Beyond it, the answer is `429 rate_limited` with `Retry-After`.
- The relay marks its own refusals, the user's rate and a route's queue, with `TQA-Relay-Throttle: user` or `route`,
  logs each one, and never forwards that header from upstream. The runtime waits such a throttle out without
  spending its retry budget, so only cancellation and the run's wall clock bound the wait; a route throttle that
  asks for more than a minute, or refuses three times in a row, moves the run to a fallback route (ADR-0025). An
  upstream provider's 429 is relayed unchanged and counts against the budget as before.
- Conversation titles are 1-200 characters and never contain U+0000 (PostgreSQL text cannot store it): `400
  invalid_request`, not a store failure. Emails already refuse control characters.
- Every state change requires an exact `Origin` match. When absent, `Sec-Fetch-Site: same-origin` is accepted.
  Conflicting or duplicate origins are denied. Foreign-origin streams are denied.
- Conversation creation accepts `{title,model_route}`; the route must exist and its backend/model bindings remain
  fixed. Only safe metadata is returned. Administrators may list metadata with `?all=true`; another user's events
  and actions always require ownership. Soft deletion retains history and revokes the runtime token.
  Conversation IDs use the canonical lowercase, hyphenated UUID spelling returned by the API.
  `last_activity_at` is database transaction time, advanced only for a newly accepted user message or a newly
  inserted terminal run event. Duplicate replay, rejected actions and text fragments leave it unchanged.
  Existing rows start from `created_at`; `updated_at` continues to describe metadata changes. Pagination stays ID-based.
- `GET /api/v1/setup` is unauthenticated and returns only `{"required": bool}`, true while no account exists.
  Bootstrap returns the new administrator (201) and sets the same session cookie as login.
- `PATCH /api/v1/conversations/{id}` takes `{title?, route?}` (at least one) with `Idempotency-Key`, owner only. The
  title is trimmed and 1–200 characters; the route must be one the user may use, as at creation. Under the
  conversation's row lock, a route change during an active run is refused with `409 run_active` and nothing changes;
  a rename is always allowed. After the change commits, a live runtime link receives the link-only `session.update`
  with the new route snapshot (no credentials or upstream settings). The relay accepts only the stored route, and a
  runtime that misses the frame gets the stored route in its next `session.config`.
- MCP servers (ADR-0011). `[[mcp.servers]]` in gateway.toml is the install's catalogue and the only place a command
  enters: `name` (`[a-z][a-z0-9_]{0,15}`, no `__`, no trailing `_`; shared with connectors), `command` (an absolute,
  normalised path outside `/workspace`, `/run/tqa` and `/tmp`), `args` (at most 32 of at most 1 KiB), `env` (at most 32,
  the same rules as `[sandbox.env]`), `templates` (configured template names that ship the command), `class` (default
  `execute`), `tools` (`{allow = [...]}` or `{deny = [...]}`, at most 64 names), `call_timeout_seconds` (1-600, default
  60) and `enabled_by_default`. At most 8 servers; anything else stops the gateway at start. A conversation starts with
  the servers enabled by default, and `PATCH /api/v1/conversations/{id}` `{mcp_servers}` changes its set within the
  catalogue (distinct names, at most 8; `400` otherwise), refused with `409 run_active` during a run. Each session
  gets `session.config.mcp`: the conversation's catalogue servers (a server whose `templates` exclude the conversation's
  template is left out) as `stdio` entries, then its connectors as `gateway` entries (no URL: the runtime uses
  `<gateway origin>/mcp/<connector>`), at most 8 in all.
- A conversation's connectors (ADR-0031). It starts with the enabled connectors marked `enabled_by_default` (by name,
  within what its servers leave of 8), and `PATCH /api/v1/conversations/{id}` `{connectors}` changes the set: distinct
  existing connectors, at most 8 together with `mcp_servers` (`400` otherwise), refused with `409 run_active` during a
  run. The proxy admits no other connector. A disabled or deleted connector stays in the set but is left out of the
  next session at once, and the proxy refuses it.
- Connectors (ADR-0031). Remote MCP servers the gateway proxies to live in the `connectors` table, seeded from
  `[[connectors]]` in gateway.toml under the ADR-0028 seed-change rule (tombstones included) and administered through
  `GET /api/v1/admin/connectors` and `PUT`/`DELETE /api/v1/admin/connectors/{name}` (versions, Idempotency-Key, audit;
  administrators, session or token). A record: `name` (the shared MCP name rule; `409 name_taken` when a stdio server
  has it), `url` (https; no user information, fragment or credential-like query keys), `auth` (`scheme` none, bearer or
  header; `header` for the header scheme: Authorization, X-Api-Key or an `X-` name; `required`), `secret_ref` (the shared
  credential's secret name; `key_missing` says whether it has a value), `class` (default `execute`), `tools` (allow or
  deny, at most 256), `call_timeout_seconds` (1-600), `enabled`, `enabled_by_default`, plus `display_name` through the
  API. `insecure_http` and `ca_file` are file-only (`400 file_only` from the API). At most 64 (`409 connector_limit`).
  A user's own credential (`GET /api/v1/connectors`, `PUT`/`DELETE /api/v1/connectors/{name}/credential`, session
  only) is sealed in the secret store, bound to the connector's URL origin when set, preferred to the shared one, and
  deleted with the connector; `409 credential_not_used` for a connector without an auth scheme.
  The MCP proxy, `POST /mcp/{connector}` (runtime token from its own sandbox's address only), admits a connector only
  if the conversation's connector set lists it, passes only the tool methods and the connector's allowed tools, injects
  the credential (never the runtime token), reaches the remote only at checked addresses without following redirects,
  and answers within the connector's timeout: JSON of at most 1 MiB, or an event stream relayed event by event (each
  at most 1 MiB, `tools/list` results filtered). At most 4 calls from a conversation to a connector and 256 in all are
  in flight (`429 connector_busy`). Each message forwarded costs one from the conversation owner's
  `mcp.calls_per_minute` bucket (`[mcp] calls_per_minute`, default 60, 1–6000; an overridable limit), and past it the
  proxy answers `429 rate_limited` with `Retry-After` without contacting the remote. The gateway classifies every
  `tools/call` with its own `[permissions]` policy on the tqa-tools engine, using the connector's class tightened by the
  tool's recorded `openWorldHint`/`destructiveHint` (a tool with no record is read as having both). A `deny` is refused.
  An `ask` call must travel alone with `Tqa-Run-Id`, `Tqa-Step`, `Tqa-Attempt` and `Tqa-Call-Id` naming its journaled
  `tool.call` (same name and arguments), and carry either the conversation owner's allow-once for its prepared-action
  hash, used by one proxied call only, or the owner's session grant for that same tool. Refusals are JSON-RPC errors
  (`-32003`) with `data.code` `tool_denied`, `approval_required`, `call_not_journaled` or `approval_missing`, and never
  reach the remote. Other failures are `403`, `424`, `502` and `504` with `connector_*` codes.
  The remote's `Mcp-Session-Id` never reaches a sandbox: the runtime gets an opaque id bound to its conversation, the
  connector and the credential source (at most 8 per conversation, 10 minutes idle; otherwise `404`, and the client
  initialises again). `GET` relays the session's server stream (one at a time, 5 minutes idle) and `DELETE` ends the
  session; a `tools/call` the runtime abandons is cancelled at the remote.
- Environments (ARCHITECTURE §4.7). `[[templates]]` in gateway.toml declares each template: `name` (slug),
  `display_name`, `image` (a registry digest or a local `sha256:` pin; tags are rejected), `capabilities` (shell,
  python, node, build, browser, data), exactly one `default = true`, and optional `cpus` and `memory_mib`. Without any, the
  runtime image is one default `minimal` template with the shell capability. Configuration seeds the `templates`
  table at every start. `GET /api/v1/templates` lists name, display name, capabilities and the default flag (default
  first, then by display name), never image references. A conversation binds to a template and the pin it resolved
  to at creation (optional `template`; absent means the default, unknown is `422 unknown_template`). Every sandbox is
  built from that binding, with the template's resources, and `session.config` carries the environment.
  `PUT /api/v1/conversations/{id}/environment` `{template}` with `Idempotency-Key` switches while no run is active
  and no accepted message waits (`409 run_active` otherwise; `422 unknown_template` for an unknown name; the same
  template is a successful no-op). The current sandbox then retires; files and history stay, and the next action
  starts the new template on the same workspace and state.

## Journal and replay

Sequenced events, permission requests and errors must be contiguous from one. A transaction locks the conversation,
loads the last stored sequence, checks `SeqChecker` and commits the next frame before publication. A per-conversation
append permit orders commit and fan-out. Large frames spill to `blobs/<conversation>/<seq>` and read transparently.
A frame containing U+0000 (which PostgreSQL `jsonb` cannot store) takes the same blob path, whatever its size, and
replays unchanged. Each run lifecycle event also records its type in `events.lifecycle`, so run activity counts
blob-stored frames too; it is written from migration 0024 onwards (older blob-stored frames are not backfilled). Queued actions,
cached responses and session grants are stored as their exact JSON bytes
(`bytea`) for the same reason.
An ambiguous failed commit may leave an unreferenced blob: deleting it could corrupt an acknowledged database commit.

`GET /api/v1/conversations?cursor=&limit=100[&all=true]` lists tasks most recently active first (`last_activity_at`,
then `id`, descending) and returns `items` and `next_cursor`, an opaque cursor for the next page (null when the page is not
full). A cursor the gateway did not issue is `400 invalid_request` with details reason `cursor_invalid`. A task that gains
activity while a client pages moves to the top, so a later page may repeat or skip it; clients de-duplicate by id. This
replaced the oldest-first `after`/`next_after` paging before 1.0 (089); a request with `after` is refused.

`GET /api/v1/conversations/{id}/events?after=0&limit=100` returns `items` and `next_after`.
Use `?stream=1&protocol=1` for SSE, omitting `limit`; the optional `after` is an exclusive resume cursor. A single decimal `Last-Event-ID` takes precedence as the exclusive
resume cursor. Sequence values must fit PostgreSQL's signed bigint. The stream subscribes before capturing a
committed watermark, replays bounded pages through it, then drains live frames. Only the replay overlap is discarded.
Later gaps, duplicates or receiver lag close with `seq_gap`, allowing reconnect and durable replay.

SSE responses carry `Cache-Control: no-store`, `X-Accel-Buffering: no`, and periodic `: keep-alive` comments.
Durable frames use `id: seq`. An initial bound `session.ready` precedes replay. Session-ready and unsequenced errors
are ephemeral and have no replay guarantee. Supervisor heartbeats are never stored or streamed.
Fan-out is process-local: use one gateway for a live conversation. Independent gateway processes do not share it.

## Actions and runtime credentials

Admission (ARCHITECTURE §4.4, ADR-0016). `[admission] slots` (default `sandbox.max_live`) is how many sandboxes run
at once; `per_user` (default `slots`) caps one owner. Slots are not yet derived from the host's memory and cores;
set them for your machine.
- A message on a conversation without a sandbox is always accepted (202). If a slot is free it starts at once.
  Otherwise the least recently active idle sandbox (live, no active run, no queued work, not pinned) is retired,
  exactly like an idle retirement, and the message waits. Busy and pinned sandboxes are never evicted.
  An owner at its `per_user` cap has its own least recently active idle sandbox retired the same way, so with one
  slot a finished conversation makes room for that owner's next one without waiting for the idle window.
- Waiting work is served fairly: the owner with the fewest live sandboxes first, then the earliest accepted message.
  Owners at their cap wait without holding others back. Each waiting conversation's stream gets `session.ready` with
  state `queued` and `queue.ahead` (0 = next) whenever the position changes. When the conversation is admitted, its
  stream gets `creating` (no queue field) before the sandbox starts. A stream that replays history ends the replay with
  the current snapshot.
- `PATCH /api/v1/conversations/{id}` `{pinned: true}` keeps a conversation's sandbox through idle periods, up to
  `[sandbox] max_lifetime_seconds` (default 28800). The lifetime is a cap for every sandbox, busy or not (ARCHITECTURE
  §4.4, reaper rule 1): at the cap a run waiting for an approval parks, an idle sandbox is retired, and a busy one stops
  taking new work and gets 120 s to finish or park before it is retired busy; a replacement then starts at once and
  the run resumes from its journal. Each retirement at the lifetime logs `sandbox reached its max lifetime` and counts
  `tqa_sandbox_retired_total{reason="lifetime"}`.
- Parking (ARCHITECTURE §4.4). When a run has waited `[sandbox] park_after_seconds` (default 60, 1–86400) for a
  decision, the worker sends the runtime the link-only `session.park {request_id}` for the oldest open request. The
  runtime journals `run.parked` and exits. Only a stored `run.parked` retires the sandbox: a link that closes first is not a park (the runtime redials and
  replays it within 30 s; one that never does is left to the dead-agent rule, and its run stays resumable). A stored park
  stays `parked` even when its teardown fails under a loaded daemon: the teardown is retried in the background, and the
  sandbox and its admission slot are gone within 305 s (`PARKED_TEARDOWN_BOUND`); a sandbox the backend still lists after
  every retry keeps its slot counted, and is logged for the start-up reconciliation. A refusal
  (`permission_binding`), a run that goes on with that approval resolved, or a connected runtime that does not answer
  within the start-up bound keeps the sandbox, and the park is asked again only after a full delay. A decision the
  gateway has delivered closes the approval at once, and a pass that delivered anything never parks. The runtime's
  refusal is consumed by the link as the park's answer and is never published to the conversation's clients.
- When a parked run's approval expires, admission treats the run as waiting work, ordered from its expiry. It
  wakes the run with nothing to deliver; the resumed runtime records `permission.expired` and continues without the
  action. An expiry that passed while the gateway was down is handled by the first admission pass after start-up.
  The sandbox is retired and the stream gets `session.ready {state: parked}`; a new stream shows `parked` too. The
  approval stays open: a later matching decision is accepted as usual, admission starts a new sandbox, and the forward
  reaches the resumed runtime after registration. A parked run is unfinished: its route and environment cannot change,
  and start-up reconciliation does not restart it. A resumed run still waiting gets a fresh park delay.

The policy proxy always runs (ARCHITECTURE §4.2, ADR-0021, ADR-0028) on `[egress] listen` (default `0.0.0.0:3128`), a
second listener that is never published outside the sandbox network. Every sandbox gets `HTTP_PROXY`/`HTTPS_PROXY`
pointing at the gateway host and that port from creation, so the live allow-list alone decides, and a rule added later
reaches running sandboxes. Rules are an exact host or `*.domain` (its subdomains only), each with 1–16 ports; at most 256
rules; an empty list denies everything. The list lives in the database (`egress_rules`); `[[egress.allow]]` rules in the
file seed it under the seed-change rule, as model routes do: a file rule is written only when it is new or changed since
the last import, a rule dropped from the file is kept (and still applies), marked `removed`, and each seed write is
audited with actor `seed`. Administrators manage the list with `GET /api/v1/admin/egress` and `PUT`/`DELETE
/api/v1/admin/egress/{host}` (session or API token, `version` and `Idempotency-Key` as for secrets); a rule is keyed by its
host pattern, validated exactly as the file validates it, and stored lower-case without a trailing dot, with sorted
ports. A write answers only after the new list is live, so it applies from every sandbox's next new connection. With
`disabled = true` the list is shown but writes answer `409 egress_disabled`. Each new connection is decided
against the current list; a tunnel already open runs to its own bounds. Only `[egress] disabled = true` (no rules allowed
beside it) turns egress off: sandboxes are then internal-only.
**Upgrade (ADR-0028):** an install that ran without `[egress]` had internal-only sandboxes. It now runs the proxy and
attaches new sandboxes to it, but denies every destination until a rule is added. To keep sandboxes internal-only, set
`[egress] disabled = true`.
- The caller must be the sandbox bound to the connecting address by a live runtime token. Zero or several bindings
  give `407`; the runtime's own `Proxy-Authorization: Bearer` must resolve to the same binding.
- `CONNECT host:port` tunnels; absolute-form `http://` requests are forwarded in origin form with `Connection: close`
  and without proxy or hop-by-hop headers.
- The proxy resolves a name once and refuses if any answer is loopback, private, link-local (cloud metadata),
  shared, unique-local, unspecified or multicast, including IPv4 carried in IPv6 (mapped, IPv4-compatible, NAT64,
  6to4). Only a rule whose host is that exact IP literal exempts a reserved address: to reach an internal service,
  allow its literal address, never its name, because a name's DNS answer can be poisoned.
  It then connects only to the addresses it checked (no second lookup, so no DNS rebinding).
- A denial answers `403`, logs one WARN (conversation, host, port, reason), and puts an unsequenced `error`
  (`egress_denied`, structured `target {host, port}`) on the conversation's stream, at most once per destination per
  minute.

The Git push relay (task 065g, migration 0028) and the Git credentials' routes are removed (ADR-0037 §7, the second
release): a task clones and pushes with plain git and a key for the git host. `[git_push]` in the file stops the start,
naming `[egress] ca_file`. At its start the gateway copies each Git credential an earlier release stored into its
owner's host key and removes the original (migration 0088 removed those already copied); one whose remote isn't a key's
address is kept, unused, and `GET /readyz?detail=1` counts it as `git_credentials_kept` (GW-I-164). The tables go in
the next release.

`[permissions]` sets what every `session.config` carries: `policy` (`read-only`, `workspace-write` (default) or
`unrestricted`) and ordered `[[permissions.rules]]` (`tool`, `class`, `args_match = {pointer, pattern}`, `decision` =
`allow`, `ask` or `deny`; at most 128, first match, validated by the protocol's own rule checks, unknown keys
rejected). The default is the workspace-write baseline with no rules. `approval_timeout_seconds` (default 86400,
1–86400) is sent as `approval_timeout_ms`: approvals outlive parking, so a person has a day to decide. The runtime evaluates the rules; the
protected-path floor always wins.

Approvals (ARCHITECTURE §5.4). Storing a hashed `permission.request` records a binding (request id, `action_hash`,
`expires_at`, and its `session_grant` scope when present) in the same transaction; storing `permission.resolved` or `permission.expired` closes it. A request
whose context names another conversation is refused with `permission_binding` and closes the link; a request without
a hash (legacy) opens nothing. `POST /api/v1/conversations/{id}/permissions/{request_id}` takes `{decision,
action_hash}`. In the action's transaction it consumes an open, unexpired binding with the same request id and
hash, and only then queues the gateway-built `permission.forward`. An unknown request, a missing or mismatched hash,
reuse, a closed or expired binding, and `allow_session` for a request recorded without a `session_grant` are `400
permission_binding`, and nothing is forwarded. Public bodies are typed, so a client cannot submit a forward frame; a
decision-shaped frame on the runtime link is an unknown server frame.

Message, cancellation and permission actions require `Idempotency-Key`, scoped to the user and exact route. Reusing
an accepted key returns its original 202 body byte for byte; a different route conflicts, and a different canonical
body returns 422 `idempotency_key_reused`. Completed or failed records expire after `limits.idempotency_hours` (default 24) from acceptance; queued work is retained until it reaches a terminal status. A bounded sweep
runs every `limits.idempotency_purge_seconds` (default 60). Only accepted responses are cached. All response types
carry `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.

Acceptance persists the validated frame and execution status before returning 202. `X-TQA-Execution-Status` reports
`queued`, `delivered` or `failed`, including on replay. The manager starts or reuses one sandbox, waits up to 30 seconds
for registration, then delivers queued actions in database order. `delivered` means the gateway wrote the action to
the registered socket; it is not an agent execution acknowledgement. A disconnect after that write and before the
status commit can cause replay on restart, so this transport does not promise exactly-once runtime execution.
Startup or registration failure records `sandbox_failed` and emits an ephemeral error on the conversation stream;
a new attempt requires a new key. Full action queues or live-sandbox capacity return `429 busy` with `Retry-After`.

The `[sandbox]` defaults are `max_live=4`, `idle_seconds=600`, `ready_seconds=30`, `cpu_millicores=1000`,
`memory_bytes=536870912`, `pids=256`, `network="tqa-sandboxes"`, `network_mode="internal"` and
`gateway_url="http://gateway:8080"`. The runtime derives its link, history and relay endpoints from this HTTP(S) origin. Configure `runtime.image` with `repository@sha256:<digest>`;
`[sandbox.env]` explicitly declares any non-secret environment inherited by the image. Idle time advances only on
accepted actions and run lifecycle events. Pongs, heartbeats and other socket traffic never refresh it. A started or
resumed run prevents idle retirement until its terminal event starts a fresh idle window; journal replay restores
that protection after restart. Retirement checks the observed action/event positions transactionally before revoking
the token and destroying the sandbox. Startup destroys labelled orphans, revokes their credentials and resumes durable
accepted work. A run is interrupted when its latest lifecycle event is `run.started` or `run.resumed` and its
conversation has no sandbox record; startup finds these with one bounded query over the event store (at most 4096,
using the partial `events_run_lifecycle` index on `events.lifecycle`) and schedules a replacement on the same workspace and state
directory. A container that disappears while its manager runs is replaced the same way. A parked run's latest
lifecycle event is `run.parked`, so it is not treated as interrupted.

`create_budget_seconds` (default 120, 60 through 600; at least one container create's own bound) bounds one start's
engine work, the retries a busy engine needs included. Each container create (on microVMs, each image helper and the
VM host's start) keeps its own bound, cut to what is left of the budget, and is retried on a timeout only; a person
sees "Starting…" at most this long before the start fails. A create the engine finishes after its attempt gave up is removed by the next attempt, by name.
The retry itself is fixed, not configured: at most three attempts, waiting 1 s and then 2 s between them (plus up to a
quarter of each as jitter), and never past the budget, which is the bound a person feels.
`startup_budget_seconds` (default 80, 60 through 600) bounds how long start-up keeps trying the microVM backend's
connect on a busy engine (deploy/README.md); a value above 85 outlasts the gateway image's health check.

Clients see the sandbox state as unsequenced `session.ready` control frames, never as stored events: `ready` when a
runtime registers with no run, `busy` after a stored `run.started` or `run.resumed`, `idle` after a stored terminal
event, and `destroyed` (shown as sleeping) after retirement. A stream opened later reports the same state.

A failed action is never silent. When a sandbox cannot be created or started, or a restart interrupts its retirement,
every queued action of the conversation is marked failed with its wire code (`sandbox_failed`, or
`runtime_start_failed` when the runtime exits before registering), one WARN per action names the conversation, the
action route and a bounded, secret-free cause, and open streams receive a retryable `error` frame followed by the
`failed` state, all before cleanup. A stream opened later derives the same failure from the latest accepted action
and reports `failed` followed by the error frame; a later accepted action clears it. Sending again is a new action.

A transient store error during a runtime append is retried with back-off for at most 5 s. The link then closes with
`store_unavailable` without acknowledging the frame, clients receive one unsequenced `store_unavailable` notice, and
the runtime resends its unacknowledged write-ahead-log tail when it redials. While the store is unavailable the relay
answers `503 store_unavailable` with `Retry-After: 1`, never 401 or 403, because a runtime token can be neither
confirmed nor refuted. An outage longer than the runtime's model retry budget ends that run with a retryable provider
failure; the transcript stays contiguous and the next message runs normally after recovery.

Conversation creation never mints credentials. The trusted manager uses `Gateway::runtime_tokens()`:

- `mint(conversation_id, sandbox_id, peer_address)` returns a new 256-bit secret, storing SHA-256 and the instance binding.
  Replacing an instance invalidates and disconnects its predecessor.
- `reserve(conversation_id)` persists an inert hash with state `reserved` and an expiry. `bind(reservation, sandbox_id, peer_address)` consumes it exactly once to activate the credential; expired reservations fail closed. Both activation paths use the same binding implementation.
- `revoke(sandbox_id)` is idempotent and disconnects only the matching instance.
- `resolve(token, peer_address)` returns the active conversation/sandbox binding. Revoked, expired and legacy
  unbound rows cannot authenticate. Only the actual transport peer address is accepted; forwarded headers never supply it.

`GET /runtime/link` requires `Authorization: Bearer <runtime token>`. Registration rechecks credentials under a
database row lock to prevent a revoke/upgrade race. Its first outbound frame is the shared `ClientFrame::SessionConfig`, followed by queued actions. The route snapshot contains no upstream URL, credentials or vendor fields. The policy is `workspace_write`, the workspace display is `/workspace`, and the frame includes protocol 1 and the last stored sequence.

The `[run_limits]` table, when present, supplies all three fields: `max_steps` (1–500), `wall_clock_secs` (1–86400) and `tool_result_bytes` (1024–262144). Omitting the table uses 100, 1800 and 8192 respectively.

`GET /runtime/history?after=0&limit=100` uses the same runtime bearer and address binding and the same ordered `items`/`next_after` paging as client history. It cannot select another conversation.

A second link replaces the first with `restarting`. During managed startup, a token reservation authenticates nothing until the manager binds its hash to the
started sandbox and internal address. Initial dials can receive 401; normal bounded redial with back-off covers this
short interval without changing the token file. Runtime input
has a 16 MiB transport limit and stricter 256 KiB typed frame cap. Gaps close with `seq_gap`; unknown frame types
receive `unknown_frame`. Ping runs every 30 seconds; missing pongs close the link after 90 seconds. Supervisor
`heartbeat` frames update `Gateway::last_heartbeat`, a separate monotonic observation. Socket pongs never count as
supervisor liveness; run activeness is later work.

## Workspace files, uploads and run changes

The files module (`src/files/`, tasks 068a and 068c; FR-36, GW-I-045, GW-I-047, GW-I-048) lets people and programs see a
task's workspace through the gateway, never through the model. On Firecracker the gateway never reads the workspace
image: listing, preview, download, uploads and the attachment check go through the runtime's workspace service over the link
(ADR-0030; `src/files/linked.rs`, replies checked in `src/files/remote.rs`). A live sandbox that mounts the workspace
answers; without one, the task owner's request starts theirs through the ordinary admission, and anyone else gets
`409 sandbox_not_ready`. Uploads stream there in chunks the runtime appends to a hidden part file and names like
Docker (shared naming in `tqa-tools`); a message's attachment check can start the owner's sandbox before the action's
transaction. There `uploads_bytes` is advisory: the gateway counts `uploads/` from the runtime's listing, which code in
the sandbox can influence; the real bound on a microVM workspace is its block image's size. Run changes there come
from the runtime's manifest and the blobs the gateway asks for (`src/files/changes/linked.rs`): copies already stored
are not fetched again, requests are batched (64 hashes, 4 MiB), a text file larger than a batch is recorded without a
copy, and only bytes whose SHA-256 is their name are stored. Access
is workspace access (the owner, or a read
share; uploads need a write share); the administrator role grants no content. Cookie requests keep the exact-Origin
rule; personal tokens may read and upload.

- `GET /api/v1/conversations/{id}/files?path=&after=&limit=` lists one directory level, directories first; links are
  listed and never followed; `.tqa/` is left out.
- `GET .../files/content?path=` returns metadata and, for UTF-8 text, a preview up to `preview_bytes` (`truncated`
  beyond it; `binary: true` and no text otherwise). `If-None-Match` with the ETag answers `304`.
- `GET .../files/download?path=` streams the bytes as `application/octet-stream` with `Content-Disposition:
  attachment` and `nosniff`, up to `download_bytes`.
- `POST .../uploads?name=` stores the raw request body as `uploads/<sanitised name>` (`[A-Za-z0-9._-]`, no leading dot,
  at most 128 bytes; an existing name gets `-2`, `-3`, ... and is never overwritten). Over `upload_bytes` is `413
  upload_too_large`; over `uploads_bytes` in the workspace is `409 uploads_full`; nothing partial is kept. A message's
  `attachments` must name such files (`400 attachment_missing` otherwise).
- Paths are relative, at most 1024 bytes, without empty, `.` or `..` segments or NUL (`400 path_invalid`). They are
  resolved one `openat(O_DIRECTORY | O_NOFOLLOW)` per component from the workspace root, and the final open uses the
  walked directory's descriptor, so a link planted anywhere, or swapped in during the walk, is refused
  (`path_invalid`) and nothing outside the workspace is read.

Run changes (068c, migration 0030). Just before the sandbox manager delivers a user message, the gateway snapshots
the task's workspace: every regular file's size, change stamp and, up to `snapshot_text_bytes`, a SHA-256. Small UTF-8
files are copied, content-addressed, to `<storage>/changes/<workspace>/`, outside the workspace. `.tqa/`, `uploads/`,
`.git/`, `node_modules/` and generated caches (`__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`,
`.venv`) are left out. A run's changes are the difference between the snapshot taken before it
started and the workspace's next snapshot, or the live workspace for the latest run.
- `GET .../runs/{run_id}/changes` lists the added, modified and deleted files with line counts, and `live`, `running`
  and `truncated`.
- `GET .../runs/{run_id}/changes/diff?path=` gives one file's unified diff (3 lines of context, at most `diff_bytes`;
  binary or uncopied files have no text). A run with no earlier snapshot is `404 changes_unavailable`.
- Bounds: the walk counts every listed entry of any kind against `snapshot_entries`, records at most `snapshot_files`
  files, and stops at a deadline (three quarters of `snapshot_seconds` at delivery, the I/O timeout for a live
  comparison). Any bound marks the snapshot `truncated`. A failed or slow snapshot is logged and never holds the
  message back. At most one live comparison per workspace runs at a time, and its result is reused for 5 seconds. The
  last `snapshots_kept` snapshots per workspace are kept, with the copies they reference.

Optional `[files]` settings (bytes and seconds):

| Setting | Default | Allowed |
| --- | ---: | ---: |
| `upload_bytes` | 26214400 (25 MiB) | 1 KiB-1 GiB |
| `uploads_bytes` | 1073741824 (1 GiB) | `upload_bytes`-10 GiB |
| `preview_bytes` | 524288 (512 KiB) | 1 KiB-16 MiB |
| `download_bytes` | 104857600 (100 MiB) | `preview_bytes`-10 GiB |
| `upload_seconds` | 120 | 10-3600 |
| `snapshot_files` | 5000 | 1-100000 |
| `snapshot_entries` | 20000 | 100-1000000 |
| `snapshot_text_bytes` | 262144 (256 KiB) | 1 KiB-16 MiB |
| `snapshot_copy_bytes` | 33554432 (32 MiB) | `snapshot_text_bytes`-1 GiB |
| `snapshots_kept` | 20 | 2-1000 |
| `snapshot_seconds` | 30 | 1-600 |
| `diff_bytes` | 524288 (512 KiB) | 1 KiB-16 MiB |
| `link_requests` | 8 | 1-64 (microVM installs: workspace requests waiting on one runtime link at once; more answer `429 busy`) |

## Channels

A channel (ADR-0014) is an append-only log per channel with a contiguous `seq`: a post takes the next number by
incrementing `channels.last_seq` in its own transaction, so numbers never repeat or skip, even under concurrent posts or
after a refused one. `/api/v1/channels` lets people create channels (the creator leads), manage members (users and
conversations, as `lead`, `member` or `observer`), post with an `Idempotency-Key` and read by `seq` with a filter. A
channel the caller may not see answers `404`; seeing is not posting (`403 not_member`); observers read only (`403
observer`); a `decision` or a `claim` is never posted; a report needs a claim on its task (`403 not_claimant`); an ended
channel takes nothing more (`409 channel_ended`); a channel holds at most 32 members (`409 channel_full`) and takes at
most 5,000 messages in any 24 hours, posts and claims alike (`429 channel_rate` with `Retry-After`; the gateway's
decisions are never refused); deleting one is audited in the area `channels`. Runs use `/runtime/channels…` with their
runtime token from their sandbox's address (no Origin, like the relay): they list their conversation's channels, read
and post as the conversation, and membership is checked on every call, so a removal takes effect at once (`403
not_member`). A run's `Idempotency-Key` is scoped to its conversation, so two conversations of one owner may send the
same key (both first runs, calls numbered per turn) without colliding. A read with `wait` (1 to 300 s) is a long poll:
it subscribes to the channel's wake-up before it reads, so a post committed in between still ends the wait at once, and
it returns an empty page when the wait runs out. A claim (`…/tasks/{seq}/claim`, from the page or a run) locks the
channel row as a post does and is granted in order up to the task's limit (one, a number, or every voting member); a
claimant's retry gets its claim back (`200`). Decisions are the gateway's: a vote must name an open proposal and only a
person vetoes; the vote that decides a proposal (`majority`: more than half of the voting members; `human`: the first
person's accept or reject; a person's veto at once) is followed, in the same transaction, by the gateway's `decision`; a
sweep every 5 s posts `timed_out` for windows that closed, and ending a channel closes its open proposals. `GET
…/proposals/{seq}` gives the tally. `GET …/events?stream=1` streams the channel like a conversation: `channel.message`
frames with `id:` their seq, replay after `Last-Event-ID`, then live on the same wake-up as waiting runs, and
`channel.state` when the channel ends. The tests are GW-U-065, GW-I-032, GW-I-033, GW-I-101, GW-I-102 and GW-I-103.

## Tests

`cargo nextest run -p tqa-gateway` needs no Docker or database. Checked SQL metadata in `.sqlx/` permits offline
builds. Database tests use isolated `#[sqlx::test]` databases, owned HTTP servers, bounded I/O and fake runtimes.
They are always ignored by default and fail loudly when explicitly selected without the required environment.
To rerun whole suites against the microVM configuration (Gate 8's SEC reruns), set `TQA_GATEWAY_TEST_BACKEND=firecracker`:
every fixture then gets a `[firecracker]` section, and the recording backend validates specs as the microVM backend would.
Tests that inspect a Docker host directory (GW-I-030's mount checks) do not apply there.

Use an ephemeral PostgreSQL role with permission to create test databases:

```sh
export TQA_POSTGRES_PASSWORD="$(openssl rand -hex 32)"
docker compose -f deploy/compose/dev.yml up -d --wait postgres
export TQA_DATABASE_URL="postgres://tqa:${TQA_POSTGRES_PASSWORD}@127.0.0.1:55432/gateway"
export DATABASE_URL="$TQA_DATABASE_URL"
cargo nextest run -p tqa-gateway --run-ignored all -E 'not binary(compose)'
cargo llvm-cov nextest -p tqa-gateway --run-ignored all -E 'not binary(compose)'
docker compose -f deploy/compose/dev.yml down
```

The image is pinned by digest. Keep the generated password for later restarts of the same volume. SQLx reads
`DATABASE_URL` before entering the test body; each test also requires it to equal `TQA_DATABASE_URL`.
After changing migrations or queries, use SQLx CLI 0.9.0 and regenerate this crate's cache:

```sh
sqlx migrate run --source crates/tqa-gateway/migrations
cd crates/tqa-gateway
cargo sqlx prepare -- --all-targets
```

Tests cover authentication/ownership, blob spill, gaps, user-scoped idempotency, replacement/revocation, two-client
fan-out and 1000 frames across 20 seeded reconnects. A deterministic in-memory property test exercises replay overlap
and commit-before-publication. GW-U-022 compares OpenAPI operations with the definitions used by the actual router.

The separate ignored Compose contract requires `TQA_DOCKER_TESTS=1` and built gateway and web images; see [deployment tests](../../deploy/README.md#integration-contract).


### Web interface

An optional `[web]` table sets `upstream = "http://localhost:3000"` and `dev = false`.
The upstream must be an HTTP origin without credentials, a non-root path, query or fragment.
Without this table, the gateway serves its API only. With it, page and asset GET/HEAD requests
stream to the internal web server, with a two-second connection deadline and a thirty-second
response-head and body-idle deadline. Page writes return 405; gateway namespaces never reach
this upstream. Unavailable pages return a plain 502 response.

Cookie, Authorization, Proxy-* and incoming X-Forwarded-* headers are stripped. The gateway sets
X-Forwarded-Proto and X-Forwarded-Host from its configured public origin, preserves router headers,
and removes upstream Set-Cookie and hop-by-hop headers. Page responses retain upstream caching
and add nosniff, same-origin referrer policy and DENY frame policy. No account data is rendered
by the web server. Development hot reload at `/_next/hmr` requires `dev = true`; startup warns
when enabled. It allows at most 32 sockets, each owned by shutdown with a one-hour lifetime.

Model routes live in the database (`model_routes`, ADR-0028). At start the gateway imports `models_path` as a seed
under the seed-change rule: a route is written only when it is new or its record in the file changed since the last
import (a digest keyed by the install key), so an administrator's change survives a restart and an edited file route
still wins. Each seed write bumps the route's version and adds an `admin_audit` row with actor `seed`. A route dropped
from the file is kept, still served and marked `removed`; it is never deleted silently. A seeded item an administrator deletes (a route, an egress rule or a secret) stays deleted across restarts: the gateway keeps a tombstone with the item's seed digest (never a value), skips the item while the file's entry is unchanged, imports it again once the entry changes, and forgets the tombstone once the file stops listing the item. An item re-created through the API takes the tombstone's digest over, so an unchanged seed does not overwrite it. Stored routes pass the same
tqa-llm validator as the file, and a stored route it refuses stops the start.

Administrators manage routes with `GET /api/v1/admin/routes`, `PUT`/`DELETE /api/v1/admin/routes/{name}` and
`GET /api/v1/admin/routes/export` (session or API token; `version` and `Idempotency-Key` as for secrets). A route is
the whole models.toml record plus an optional `display_name` (1–80 characters) for the pickers. The name (1–128 bytes)
is immutable, because conversations store it. A write validates the whole resulting registry with the file's own
validator, so the API and the file refuse a route for the same reason (`details` carry the tqa-llm reason code), and it
answers only after the new registry is live, so the next request uses it. A route another route lists as a fallback
cannot be deleted (`409 route_is_fallback`). The list shows `key_missing` (the `key_ref` has no secret value),
`fallback_of`, and a `probe` state, which stays `never` until the list reads the stored probes (099c). The export is
models.toml without display names.

`POST /api/v1/admin/routes/{name}/probe` (administrator, session or API token; an `Idempotency-Key` is required) tests
a route the way a runtime will use it:
- It goes through the relay's own shaping, credential, limiter and decoder. It sends a streamed request with one tool,
  checks the tool call carries a nonce derived from the key, and completes the round trip, reading usage and reasoning.
- Its spend is bounded: two requests (a third, and at most a fourth, for the strict-string check on a route that
  sends strict tools), each capped at 256 output tokens on a route without `reasoning`, enough for a model that reasons
  unasked before its call. A route with a thinking budget table gets its smallest budget plus 64; any other route that
  declares `reasoning` gets 1024 (thinking counts against the cap), always within the route's `max_output`.
- It answers `200 {probe: {state, probed_at, route_version, observed}}` within 60 seconds, **including when the probe
  failed**: the failure is the observation, with a named code.
- It stores the latest result per route (`route_probes`) with the digest of the route document it tested, so a later
  edit can show it as stale. A probe never changes the route or its version, and is audited as action `probe`
  without values.
- A replay of the key returns the stored result without probing again.
- One probe of a route at a time (`409 probe_running`), at most one per route per 30 seconds and ten per administrator
  per minute (`429 rate_limited` with `Retry-After`). A route whose secret has no value is `409 key_missing`. None of
  these refusals contact the model.

Administrators override nine limits with `GET /api/v1/admin/limits` and `PUT`/`DELETE /api/v1/admin/limits/{key}`
(session or API token; `version` and `Idempotency-Key` as for secrets; ADR-0028): `admission.slots` and
`admission.per_user`, and the sandbox's `cpu_millicores`, `memory_bytes`, `pids`, `idle_seconds`,
`max_lifetime_seconds` and `park_after_seconds`, and `mcp.calls_per_minute`. The file's values seed them under the seed-change rule, and `DELETE`
resets one to the file's value. A write must meet the file's bounds, keep `per_user` within `slots`
(`limit_order`), and stay above floors that keep a sandbox usable: 64 processes, 256 MiB of memory in whole MiB (a
microVM's minimum), 100 millicores and a 10 s park delay (`limit_out_of_range`); a file value below a floor stays and
is shown as `below_floor`. An admission limit applies from the next admission pass, a sandbox limit to the next
sandbox created: a running sandbox keeps its CPU, memory, processes and the timers it was admitted with; the
connector rate applies from the next proxied call. The relay's
unset per-user rate follows the effective `per_user`. When a file edit leaves a stored value out of bounds (for
example `max_live` lowered below an administrator's `slots`), the start moves it into bounds, audits the change as the
seed's, logs a warning and shows `clamped_from` until the next write. Everything else in `[sandbox]`, `[admission]`,
`[limits]` and `[relay]` stays file-only.

`GET /api/v1/models` returns `{items: [...]}` for authenticated accounts. Every registered route
is currently available for conversation creation. Items are sorted by name and contain only
name, `display_name` (null when unset), `verified` (null until a route probe is recorded), model, protocol,
context_window, vision, native_tools and optional reasoning allowed/default values.

Conversation creation requires `Idempotency-Key`, with the same bounded ASCII format and owner-scoped key namespace
as message actions. Retrying `POST /api/v1/conversations` with the same canonical body returns the original 201 and
metadata, including the conversation id and timestamps. Reusing the key with another body returns
`422 idempotency_key_reused`; another route returns `409 idempotency_route`. The conversation and response cache
commit in one transaction, serialized by the owner/key advisory lock. A replay creates no workspace or additional
conversation. A failed transaction is not cached; ambiguous commit errors retain the workspace for a possible
committed conversation. The configured idempotency retention also applies to create responses.
