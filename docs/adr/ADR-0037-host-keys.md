# ADR-0037: Per-service keys: a key for a web address, used by a task without ever entering its sandbox

## Status

Accepted (the owner's decisions 1 to 6, 30 Sep 2026, all as recommended). To be built after candidate 13c.
Amended 30 Sep 2026: the gateway's contract with the web (see Amendment 1).
Amends [ADR-0023](ADR-0023-workspace-push.md), [ADR-0033](ADR-0033-built-in-secret-store.md) and
[ADR-0035](ADR-0035-network-guardrails.md) where they conflict (see Amendments).

## Date

2026-09-30

## Context

Today a task can use a credential only through a gateway path built for it: the model relay (route keys), the MCP
proxy (connector keys) and the Git relay (`git_push`, one push per journaled call). Anything else a task reaches
through the egress proxy (a REST API, a package registry, a plain `git clone` of a private repository) either works
without a key or needs the key inside the sandbox, which the product forbids (FR-19, ADR-0033).

The egress proxy (ARCHITECTURE §10) identifies the calling sandbox by its transport address and runtime token,
checks host and port against the task's network list, resolves DNS itself, refuses private addresses and then only
copies bytes: it never sees inside TLS. The secret store (ADR-0033) keeps values write-only in PostgreSQL, and its
read audit records reference, consumer, conversation, user, run and outcome.

The owner's rules for per-service keys:

| # | Rule |
|---|------|
| R1 | A run uses only its sender's own keys, plus shared keys the sender may use |
| R2 | No one can read a value: not a person, not the agent, not tool code in the sandbox |
| R3 | What a run holds works only in that run, from that run's sandbox |
| R4 | Every use is audited: who, which run, which host |
| R5 | Keys only for hosts on the task's network list; `git_push` gives way to plain git over HTTPS |
| R6 | **A member's key serves only that member's runs**: never another member's run, never shared, never leaked, in a shared workspace, a channel or a team run alike |

Alternatives considered:
- **The key in the sandbox** (an environment variable with the real value): forbidden (FR-19, ADR-0033). Rejected.
- **A per-host relay like the model relay** (`https://gateway/relay/h/api.github.com/…`): no interception, but every
  client and SDK needs its base URL changed, git needs URL rewriting, redirects and absolute links break, and TLS to
  the real name isn't possible. Rejected for general use; it stays the right shape for the model relay.
- **Opening TLS for every host:** needless plaintext in the gateway and breakage where no key is needed. Rejected.
- **Placeholders valid for the whole task:** a leftover background process would keep a key alive between runs; fails
  R3. Rejected.

## Decision

### 1. Host keys

A **host key** is a secret in the built-in store plus where and how it may be used:

| Field | Meaning |
|-------|---------|
| `ref` | its value in the sealed store (`secrets/host-<hex>`), never shown |
| `owner` | a member (personal key), or none (shared key, made by an administrator) |
| `host`, `port` | one exact host name, no wildcards; port 443 unless given |
| `path_prefix` | optional: only requests whose path starts with it (for git, one repository or organisation) |
| `scheme` | how the service expects it: `bearer`, `basic` (with a stored user name) or `header` (a named header) |
| `env` | the name the task sees it by, e.g. `GITHUB_WORK` |
| `users` | shared keys only: everyone, or listed members (bound like `secret_consumers`) |

- Values are **write-only**, as every secret is: set or replace, never read back, in the UI, the API or the seed file.
- **Only administrators create shared keys** (owner's decision 5). Members create personal keys.
- **A personal key belongs to its owner alone (R6):** it is never bound to anyone else, never offered to another
  member's run, and an administrator can delete it but can't use it or bind it. Personal keys are private to their
  owner: an administrator sees that they exist (their owner, web address and last use) and their audit rows, never
  their name or value (Amendment 1).
- **Names are unique** across a member's own keys and the shared keys they may use; a clash is refused with the name
  of the key that has it, except that a shared key clashing with a member's own key names no one (Amendment 1). For the same host, a member's own key is used before a shared one; the names never clash.
- `git_credentials` rows become personal host keys (`basic` or `bearer`, the host and path from the repository URL),
  named from the host and flagged "Check the name" once in the UI, without blocking use.

### 2. Which keys a run gets (R1, R5, R6)

The **sender** is the person whose message started the run (the author of the `message.user` it answers). A run's
keys are, at its start:
1. the sender's personal keys, plus the shared keys whose `users` include the sender;
2. minus any key whose host and port the task's effective network list doesn't allow (the same list the proxy
   enforces and the prompt shows);
3. minus deleted keys and keys of a disabled member.

A spawned agent's run (ADR-0034) gets at most its caller's keys, as its network list is at most its caller's. (The
second review's L9. Built as (12 Oct 2026): a helper's turn is sent as the team's person, the same sender as its lead's
run, so a helper's run gets its lead run's keys by construction and minting passes no caller; the filter stays
unit-tested. The end-to-end test is SEC-046's team case.) In a
shared workspace, a channel or a team run, each run gets **its own sender's** keys only (R6): a teammate's run on the
same workspace never receives them, and a team lead's run never receives its members' keys, nor they the lead's.

### 3. Placeholders, per run (R2, R3)

For each key the gateway mints a **placeholder**: `tqa_ph_` plus 32 random bytes, base64url. It keeps the hash of
the placeholder with the key, run, sandbox instance, conversation, sender, host, port, path prefix and scheme, in
memory and durably (a gateway restart during a run keeps them valid). Only the runtime receives the plaintext
placeholder, in the run's start frame (a new `host_keys` field: `env`, `host`, `scheme`, placeholder; a runtime
protocol revision, ADR-0026). For every process it starts in that run the runtime sets each key's `env` variable to
its placeholder, and a git credential helper for `basic` keys on git hosts (§7). The model's prompt lists key names
and hosts ("`GITHUB_WORK` for github.com/acme"), never a placeholder.

A placeholder is valid only while **all** hold: the request comes from the sandbox instance it was minted for; that
sandbox's current run is the run it was minted for; the run is still going (placeholders end with `run.completed`,
`run.failed`, `run.cancelled` and the sandbox's destruction); the host, port and path match the key **as it is
now** (an edit during the run moves it: the old address is then `wrong_host`); the key still exists, its owner is
active, the sender is still its owner or one of a shared key's people (taken off mid-run: `revoked`; the second
review's M2, 1 Oct), and the task's network list still allows the host. Anywhere else a placeholder is inert.

The task's network list, at minting and at each use, is the installation's allow-list as it is at that moment: a task
has no narrower list of its own today (the second review's M4). If one comes (a profile's list, say), both places take
it together.

Known limit (the second review's M3): at use, the proxy takes the run's sender to be the task's owner, for the audit
row's person, the notice's sender and who sees the key's name; minting takes the authors of the turn's messages.
They are the same person today, since only a task's owner posts to it. Before channel or team runs let anyone else
start a run, the proxy must take the sender from the placeholder instead.

### 4. The proxy opens TLS only for keyed hosts (owner's decision 2)

1. `CONNECT host:443` arrives; the proxy identifies the caller and checks the list, exactly as today.
2. **If the caller's current run holds no key for this host, the tunnel is opened as today, unseen.** Only a host
   the running task holds a key for is ever opened.
3. For a keyed host the proxy completes TLS with the sandbox using a leaf certificate for exactly that host, issued by
   the install CA (§6), and opens its own TLS connection upstream to the address it already checked, with SNI and full
   verification against the public roots. The install CA is never trusted upstream. A failed upstream verification is
   a refusal, never a fallback to a tunnel or to plain text.
4. For each request it checks that `Host` / `:authority` equals the CONNECT host, and that the request target is a
   path only (an absolute or authority-form target, which a server or a shared front may route by over `Host`, is
   refused before anything else; the second review's H1, 1 Oct), finds placeholders, validates each
   (§3) and its position (§5), writes the audit row (§8), and only then replaces the placeholder with the value and
   forwards the request, re-serialising the headers (never forwarding the client's bytes as they are). Bodies stream
   both ways without buffering. The first slice speaks HTTP/1.1 only (ALPN `http/1.1`, both ways) and passes no
   upgrades: `Upgrade` and `Connection` are hop-by-hop and dropped, so a WebSocket on a keyed host doesn't open (the
   second review's L8). HTTP/2 through hyper, and upgrades passed after the swap, are a later step (Consequences).
5. A request without a placeholder on a keyed host is forwarded unchanged. A placeholder over plain `http://` is
   refused: a value is never substituted into cleartext. The proxy reads a plain request's head for a placeholder,
   raw or percent-encoded; a body after it streams unread, so an inert placeholder there can leave in cleartext,
   never a value (the second review's L4).

A refusal is a plain `403` whose body says why in words ("this placeholder belongs to another run", "this key goes
only in the Authorization header"), and nothing reaches the host. A placeholder in a request body is refused the same way, counted and told as `wrong_position`, and never sent; because bodies stream, the request's head and the body before it may already have gone, and a swap of the same request's header placeholder stands, counted `ok` (the second review's M1, 1 Oct).

### 5. Headers only (owner's decision 3)

| Scheme | Accepted form |
|--------|---------------|
| `bearer` | `Authorization: Bearer <placeholder>` exactly |
| `basic` | `Authorization: Basic base64(<user>:<placeholder>)`; the user name must match the stored one if set |
| `header` | a named header whose whole value is the placeholder, e.g. `x-api-key: <placeholder>` |

A placeholder anywhere else (another header, the path, the query, the body) is refused, not swapped, so the agent
learns the rule and a mistake can't become a leak. Duplicates of the key's header are refused. Query-string and body
placement are not in this decision.

### 6. The install CA, rotated on demand (owner's decision 4)

At first start the gateway creates an install CA (ECDSA P-256) whose private key is a private file beside the install
key (0600, gateway-only). It issues leaf certificates on demand, one per keyed host (SAN exactly that host, 24 h,
cached in memory). **Rotation is on demand only:** an administrator's command makes a new CA, and both are trusted for
one sandbox lifetime. Only sandboxes trust the CA, never people's browsers or the host.

The runtime receives the CA certificate in `session.config` and writes one bundle (the image's system bundle plus the
install CA) to its state directory. Every process gets `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `PIP_CERT`,
`NODE_EXTRA_CA_CERTS`, `CURL_CA_BUNDLE` and git's `http.sslCAInfo` pointing at it; on Docker the bundle is also bound
read-only over the system path. Clients with their own compiled-in or pinned roots fail **on keyed hosts only**, with
a certificate error; the admin guide lists the settings.

**Response scrubbing.** On keyed connections the proxy replaces, in response headers and bodies, the exact value and
its common encodings (base64 of `user:value`, URL encoding) with the placeholder, with a streaming matcher and a
bounded window. It asks upstream for `Accept-Encoding: identity`; a compressed response is refused. This is defence
in depth, not a guarantee (see Security analysis, T1). What it scrubs is learned at swaps and shared by person and
address (the second review's H2, 1 Oct): a value any of a person's runs swapped is scrubbed on every keyed connection of
theirs to that host and port, from its first byte, whichever run, task or connection swapped it. It lives only in the
gateway's memory, bounded, and a key's change or deletion forgets its address. Known limit: a host that sends back a
value it stored earlier, before that person's first swap since the gateway started, isn't scrubbed; closing it would
mean reading every key's value when a connection opens.

**What the gateway sees.** Plaintext of keyed hosts' requests for the running task only. It logs no body and no
header value; the audit keeps the host, never the method or path.

### 7. Git over HTTPS; `git_push` and the Git relay are retired (owner's decision 1)

For a `basic` key on a git host the runtime configures a credential helper through `GIT_CONFIG_*`: it answers the
stored user name and the placeholder, with `credential.useHttpPath=true` when the key has a path prefix. Bearer git
hosts use `http.<url>.extraHeader`. Then `git clone`, `fetch` and `push` work as plain git, and the proxy swaps the
value. The `git_push` tool, the Git relay's routes, `git_push_operations` and the push setup are removed. What is
given up, knowingly: the binding of a push to one journaled call (SEC-019) and the relay's refusal of deletes, tags,
multiple refs and push options. The key's scopes and the git host's rules decide, as ADR-0035 §5 already chose for
branches.

**The web (2 Oct 2026).** There is no push action in the web to retire: FR-19's "push from the UI" was never built as a
button, and the web calls no push route. It renders `git_push` calls in transcripts (the tool card, its one-line
summary and the worded failures), and those stay through both releases so a past task still reads as it happened.
PRD FR-19 is restated to match (2 Oct 2026, PRD §12 item 22): git is a tool in the sandbox like curl or pip, a git
host an outside service like any other, its key in key management, and nothing in the web or the setup is specific to
git.

### 8. Audit (R4)

Every use writes a `secret_reads` row: `consumer_kind = 'host'`, `consumer = host:port`, `user_id` the sender,
`run_id` the run (always known: the placeholder names it), `conversation_id`, and an outcome: `ok`, or a refusal:
`foreign_run`, `foreign_sandbox`, `expired`, `wrong_host`, `wrong_position`, `not_allowed`, `absent` (and `revoked`,
`unrecorded`: Amendment 1). Rows are counted
as today (one row per key, host, run and outcome, with `count`, `first_at`, `last_at`). **The row is written before
the value is sent; if the write fails the request is refused** and nothing reaches the host (fail closed, as
`key_use_unrecorded` for model calls). A member sees their own keys' rows, and only their own use of shared keys; an
administrator sees every row, refusals included.

### 9. Where people manage keys

Settings › Keys, as sketched and reviewed (the team's UI sketch, 30 Sep 2026):
- **Mine:** each key's name (as the task sees it), web address and **last used** (when, and in which task), a quiet
  warning when the task's network list doesn't allow its host; Add, Replace, Reads and Delete. Adding is a Page: web
  address with an optional path, a name suggested from a built-in list of well-known hosts (else the host's first
  label, upper-cased), how the service expects it in plain words, and the write-only value.
- **Shared:** for administrators, the same plus **who may use** (everyone, or chosen people); for members a read-only
  list of the shared keys they may use, names and addresses only.
- Every refusal has plain words on the task's page and in the list's "last try" (the UI standard: no codes).

### 10. Not decided here (owner's decision 6)

- **Per-key method and path limits** (for example read-only GETs): later, on this same interception, each its own
  decision. ADR-0035 §2 point 5 stays open for the network list.
- Query-string or body placement, wildcard hosts, OAuth token exchange.

## Amendment 1 (30 Sep 2026): the gateway's contract with the web

Agreed between the gateway and web halves of the build, after the team's review of the plan.

- **Two more refusal outcomes** (§8): `revoked`, a placeholder whose key was deleted, or whose owner was disabled,
  during the run; `unrecorded`, the audit row could not be written, so nothing was sent (the fail-closed write of §8,
  named as `key_use_unrecorded` is for model calls). Both are refusals, audited where the store allows, and neither
  reaches the host.
- **A third, `unavailable`** (1 Oct, agreed with the web): the key's value could not be read from the secret store, so
  nothing was sent. A refusal like the others, in the audit rows, the task's key uses and `host_key.refused`. The web
  says, on the task's step, "{name} wasn't sent to {host}: the key store was unavailable. Try again in a moment."
  ("{name} 未发送到 {host}：密钥存储不可用。请稍后再试。") and, as a key's last try, "the key store was unavailable"
  ("密钥存储不可用").
- **A shared key may not take a name one of its members already uses for their own key.** An administrator's create,
  rename, or a change of **who may use** that takes such a member in is refused `409 name_taken_by_user`, in the same
  shape as `name_taken` but naming no one: not the member, not their key, not how many. The gateway checks it; only it
  sees members' keys. A member's own key clashing with a shared key they may use stays `name_taken` with the other
  key's identity, as §1 says.
- **Reserved names.** A key's name is `^[A-Z][A-Z0-9_]{0,63}$` and none of `PATH`, `HOME`, `USER`, `LOGNAME`,
  `SHELL`, `PWD`, `OLDPWD`, `TMPDIR`, `LANG`, `LANGUAGE`, `TERM`, `HOSTNAME`, `HTTP_PROXY`, `HTTPS_PROXY`,
  `NO_PROXY`, `ALL_PROXY`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE`, `PIP_CERT`,
  `NODE_EXTRA_CA_CERTS`, `NODE_OPTIONS`, `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, nor any name starting `LC_`,
  `LD_`, `GIT_` or `TQA_`: the environment a task's commands rely on or the runtime sets. One list in the gateway,
  named in its API description, so the web's pre-check says the same.
- **The administrator's view of members' own keys, least exposure** (§1): owner, host, port, path prefix, when it was
  set and its last use, never its name (the name a member gave their key is theirs), and never its value. An
  administrator can delete such a key and see its audit rows; nothing in an administrator's answer about it carries
  its name.
- **"Check the name"** on a key migrated from a Git credential is stored, and any successful save of the key clears
  it, the same name included.
- **Two releases** (1 Oct, ENGINEERING-GUARDRAILS §6 and §8) retire `git_push` and the Git credentials (§7). The
  first migrates the credentials into host keys, stops writing the old tables, takes `git_push` out of the catalogue
  and leaves the Git credentials API read-only, its writes answering `410 Gone` (`moved_to_host_keys`, pointing to
  Settings > Keys); the next removes the tables and the routes. No build holds a usable credential twice or keeps
  `git_push` without a way to add its key.
- **Nothing specific to git in the UI or setup** (the owner's decision of 2 Oct 2026, PRD §12 item 22) joins the same
  two releases. The first deprecates, and the web stops using: a profile's `repositories` switch (which granted
  `git_push`), and a git address on a new workspace or task (`git_url`, shown back as `source_url`) with the clone the
  workspace's first sandbox makes from it. They are still accepted and documented as deprecated. The second removes
  them with the relay, `git_push_operations` and the Git credentials' tables and routes. A task clones and pushes with
  plain git and the person's key, as any other HTTPS host.
- **The `git-` secret prefix** stays reserved for a person's secrets through the first release: the Git credentials'
  stored values (`secrets/git-…`) are still there, readable and deletable, and the reservation is what stops a shared
  route, connector or decision engine from naming one of them. The second release deletes those values first and
  then drops `git-` from the per-person prefixes (and the web's reserved-prefix copy), in the same step.
- **The task's frames** of a keyed call are live, unsequenced control frames (`host_key.used` once per run and key;
  `host_key.refused` with the outcome, at most once a minute per key and outcome), not journal events: the journal
  records the command. Each carries the run and the **step** that was running when the proxy saw the call (the run's
  latest `step.started` the gateway holds), so a use that arrives after its step ended still lands on its own step. A
  page opened later reads the task's key uses by run. Both frames, and each row of the task's key uses, carry the
  run's **sender** (§2: whose keys the run holds, not always the task's owner in a channel, a shared workspace or a
  team run). A key's **name** goes only to the sender's own streams and answers; every other viewer of the task gets
  none, and reads that the call used a key for its host. The name never leaves the gateway for anyone but the key's
  user.

## Amendment 2 (1 Oct 2026): a key for an exact IP address

Decided by the lead with the owner (1 Oct 2026), found while planning the stack suite (G9): the egress guard lets a task
reach a private or reserved address only through an allow rule for that exact address (ADR-0035), and §1 allowed a key
only for a host name. So a server reached by address, such as an internal git server, could use the Git relay (whose
`git_push.ca_file` trusted its authority) but no key, and retiring the relay would have left it with nothing.

- **The address:** a key's address may be an exact IPv4 or IPv6 address (`https://10.0.0.5`, `https://[fd00::5]:8443`),
  stored in canonical form without brackets (zeros compressed as the standard library writes them). Wildcards and
  ranges stay refused.
- **Only through that address's rule:** a key never makes an address reachable. The proxy decides a CONNECT to an
  address as before: only an allow rule for that exact address admits it, so a private address needs it too.
- **One address, one spelling:** the proxy compares a CONNECT's address, each request's `Host` (`[v6]` and `[v6]:port`
  included) and the allow rules' addresses in canonical form, so another spelling of the same address is that address
  and nothing else is.
- **Certificates:** the leaf the install CA issues for an address carries it as an IP subject name, and the proxy
  verifies the host's certificate against the address (`ServerName::IpAddress`), under the public roots and
  `egress.ca_file` (G9a).
- **The runtime:** a grant's host may be an address in canonical form (revision 11, unreleased, so in place); git's
  settings put an IPv6 address in brackets in their URLs.
- **Still open:** a *named* internal server (a name that resolves to a private address) is the owner's per-rule switch
  in ADR-0035's amendment of the same day, built next.

Tests: GW-U-102 (an address is one exact address in canonical form), GW-U-115 (rules compare an address by value),
PROTO-U-033, TOOL-U-041, GW-I-176.

## Amendment 3 (1 Oct 2026): where the install CA lives

§6 put the install CA beside the install key. A deployment mounts the install key and the secrets seed read-only
(`deploy/compose/docker.yml`), so the gateway couldn't make the CA there and didn't start; found by G9b's first stack
run, a defect of G5a (TEST-PLAN §19, GW-I-179). Decided by the lead: the CA lives in its own private directory,
`[host_keys] ca_directory`, by default `<storage.root>/host-ca` (0700, its files 0600), which no sandbox mounts; the
gateway refuses a place inside a sandbox mount or an existing directory others can read; `rotate-host-ca` reads the
same setting from the gateway's configuration file. The keys release has never shipped, so no install has CA files
anywhere yet and there is no compatibility path. Backups of the storage root now hold the CA's private key. Tests:
GW-I-179, GW-U-118.

## Amendment 4 (6 Oct 2026): the second release of §7

The second release removes the `git_push` tool's code, the Git relay and its routes, the Git credentials' routes,
`[git_push] ca_file`, a profile's `repositories` and `git_url` on a new workspace. A file that still has `[git_push]`,
or a request that still sends `repositories` or `git_url`, is refused naming it (`400` reason `removed`), never
ignored, since an ignored `git_url` would give a task an empty workspace where it expected a clone. A workspace an
earlier release made from a git URL is still cloned by its first environment, so `session.config`'s clone stays.

The credentials are removed without loss for an install that skips the first release: migration 0088 removes only the
originals already copied (`moved_at` set) and their sealed values; the start-up copy stays for this release and removes
each original once its key is committed. One that can't become a key (its remote isn't a key's address) is kept, unused,
and the readiness detail counts it (`git_credentials_kept`) so the administrator's Status page says what to do. The
relay's types stay in the protocol schema until a revision can take them out, and `GitPushFailure` still words an
interrupted `git_push` call in an old journal. The tables and those leftovers go in the next release. Tests: GW-I-164,
GW-I-136, GW-U-083, GW-U-114, WEB-U-265; retired: GW-I-014, GW-I-149, SEC-019, TOOL-U-020.

## Amendments to earlier records

**ADR-0023 (pushing from the workspace).** Superseded in full except its credential boundary, which moves to the
egress proxy:
- Status, lines 7 to 9 ("The credential boundary, the single-operation authorization bound to the exact journaled call,
  and the refusal of deletes, tags, multiple refs and push options stand"): only the credential boundary stands; the
  single-operation authorization and the relay's refusals go with the relay.
- Decision, lines 29 to 51 (the `git_push` tool, the Git relay and its single-operation authorization, the create
  rules and `push_decider_mismatch`): superseded by §7. The rule that only the run's sender's key is used stands, as R1.
- Consequences, lines 55 to 61 (push uploads, relay receipts, the UI push action on the same executor): void with
  the tool; a UI push, if built, uses plain git with a host key.

**ADR-0033 (the built-in secret store).**
- Decision 1, line 37 ("nothing reaches a sandbox"): stands; a placeholder is not a value.
- Decision 3, line 42 (read audit): extended with the `host` consumer kind, the refusal outcomes and the fail-closed
  write before use (§8).
- Amendment, lines 67 and 68 ("A Git push's key reads name the run of the journaled call that its setup verified"):
  superseded; a host key's reads name the run its placeholder was minted for.

**ADR-0035 (guardrails at the network boundary).**
- §2, lines 67 and 68 (the Git relay checks the push host and keeps its single-operation binding): replaced; the
  egress proxy checks every keyed host against the task's list at mint time and at every use.
- §2 point 5, lines 105 and 106 ("HTTP method and path rules, which need TLS interception at the proxy. That's a
  separate ADR"): this record is that ADR for **keyed hosts only**; the gateway now sees the plaintext of keyed
  hosts' requests for the running task, and nothing of any other host. Method and path rules stay later (§10).
- §4, line 152 ("Credentials never enter the sandbox"): stands; the sandbox holds inert, per-run placeholders.
- §5, lines 156 to 162 (the Git relay keeps its credential role and single-operation authorization): superseded by
  §7; plain git with a host key, and the git host decides.
- §6, line 168 ("the model relay, the Git relay and the MCP proxy add them"): now the model relay, the MCP proxy and
  the egress proxy for keyed hosts.
- Security analysis, line 212 (the push floor and the relay's branch rules, SEC-019): the credential stays in the
  gateway through the proxy's swap; SEC-019's binding to one call is given up (§7).

## Security analysis

Actors: the agent (possibly prompt-injected), tool code in the sandbox, a teammate, an administrator, a keyed host
(honest but may reflect data), a network attacker. Out of scope, as today: a compromised gateway or host operating
system, an administrator with shell access to the gateway.

| # | Threat | Mitigation | Residual |
|---|--------|------------|----------|
| T1 | The value comes back through the keyed host (echo, error page, introspection) | Response scrubbing of the value and its encodings; identity encoding | A transformed value, or a new credential the key can mint. The admin guide: narrow scopes, no admin tokens |
| T2 | A key used for more than intended on its host | The host's scopes and rules; `path_prefix` | Per-key method and path limits later (§10) |
| T3 | A placeholder copied out (shared file, transcript, model provider) and reused by a teammate, another task or a later run | Bound to run, sandbox instance and host; refused and audited elsewhere (R3, R6) | None known |
| T4 | A placeholder sent to a host the agent controls | Swapped only on a verified connection to the key's own host | None |
| T5 | Domain fronting | `Host` / `:authority` must equal the CONNECT host, and the request target is a path only | None |
| T6 | A network attacker impersonates the keyed host | Upstream TLS verified against public roots with SNI; gateway-side DNS; no fallback | As for any TLS client |
| T7 | Theft of the install CA key | Private gateway-only file; short leaf lifetimes; rotation on demand | Its holder could intercept sandbox traffic, never people's browsers |
| T8 | The gateway sees plaintext of keyed hosts | Keyed hosts of the running task only; no bodies or header values logged; host-only audit | An operator with shell access, as for the keys themselves |
| T9 | Request smuggling across the swap | hyper parsing, duplicate headers refused, headers re-serialised | Parser bugs: the header rule is fuzzed |
| T10 | Cleartext downgrade | A placeholder over `http://` is refused | None |
| T11 | A teammate reads or uses another member's key (R6) | Write-only values; personal keys minted only into their owner's runs, in shared workspaces, channels and team runs alike | None |
| T12 | An administrator uses a member's personal key | Personal keys can't be bound or used by anyone else | An administrator can make a shared key and bind it to themselves: audited, by design |
| T13 | A use without an audit row | Row written before the swap; a failed write refuses | None |
| T14 | Many keyed connections exhaust the gateway | Bounded intercepted connections per sandbox and in all; streaming | Busy tasks slow down |
| T15 | A placeholder reaches logs, events or the model | Inert outside its run and sandbox; redacted from gateway logs | Harmless by design |

## Consequences

- A task can call any allowed HTTPS service with a key, and use plain git, while no value ever enters its sandbox.
- Less special code: the Git relay and `git_push` go. More at the proxy: TLS on both sides for keyed hosts, the
  header rule, scrubbing, an install CA, and a new dependency to review under ADR-0017 (`rcgen` for leaf
  certificates).
- Clients with pinned or compiled-in roots fail on keyed hosts only; documented.
- Known limits of the first slice (the second review, 1 Oct): keyed hosts speak HTTP/1.1 only (ALPN `http/1.1`); a
  `header` key is refused (`scheme_not_supported`); upgrades such as WebSocket don't pass on keyed hosts (L8). A
  placeholder kept from an earlier run and sent to a
  host the current run holds no key for goes through a plain tunnel, not intercepted, so it isn't counted `expired`;
  no value moves (L5). A keyed request's or answer's body has no time bound of its own, only the head and the
  answer's start; a slow body holds a keyed connection, which the per-sandbox bound limits; and every CONNECT costs
  one database query to tell keyed from plain (L10). Someone removed from a shared key while a run of theirs is
  going may see the key's name once more, in that run's live `revoked` refusal: the run held the key from its start.
  A task's stored key uses judge the name when read, so its old rows lose it at once.
- Size: about 30 developer days with tests and a security review; a first slice can ship HTTP/1.1 only, `bearer` and
  `basic` only, Docker only (about 18), then HTTP/2, `header` and the microVM.
- **Upgrade:** a runtime protocol revision (the start frame's `host_keys`, the CA in `session.config`); older runtimes
  are refused at the floor (ADR-0026). `git_credentials` migrate to personal host keys; the audit's `git` rows stay
  readable as history. A task's agent loses the `git_push` tool and pushes with plain git.

## Documents this changes

- PRD FR-19 (git: plain git with a host key; no Git relay or single-operation authorization); a new requirement for
  per-service keys (R1 to R6), numbered by the PRD's owner.
- ARCHITECTURE §5.4 (the credential boundary through the egress proxy), §10 (the egress proxy for keyed hosts, the
  install CA; the Git relay removed), §12 (the threat rows, and the push row replaced), §18 (this record).
- The admin guide (host keys, shared keys and who may use them, the CA and client settings, scoping tokens) and the
  user guide (the Keys page, git with a key).
- TEST-PLAN: TOOL-U-020, TOOL-S-021, RT-I-016 and GW-I-014 go with `git_push`; SEC-019 is restated (the credential
  appears nowhere outside the gateway; no per-call binding); new rows for R1 to R6, the trust matrix, the header rule,
  scrubbing, restart and spawn, claimed at build time.

## Tests

One test per rule on the real stack (a `host-keys` stack suite with a fake keyed HTTPS server and a fake git host),
plus unit tests; IDs are claimed at build time.

| Rule | Test | Expected |
|------|------|----------|
| R1, R6 | Members A and B; A has a personal key and a shared key bound to A only for `api.test`; B's task runs on A's shared workspace, reads a placeholder A's earlier run left in a file, and calls `api.test`; team runs (v1.1: a team's helpers are its person's own agents): A leads a team whose helper tries the lead's placeholder and its own, and B leads a team on A's shared workspace whose helper tries the placeholder A left there | B's run holds none of A's keys; the host receives nothing with a value; 403 in words; a `foreign_sandbox` row with B as who (another task's placeholder is judged before its run, §3). A's own task succeeds. A's helper holds A's keys, as its lead does: the lead's placeholder is refused `foreign_sandbox`, its own works; B's helper holds none of A's keys and is refused `foreign_sandbox` with B as who |
| R2 | Set a key; list and get it as owner and administrator; a task calls an echo endpoint and dumps its environment and files | Metadata only in every answer; the value nowhere in the sandbox, events, logs or database plaintext; the echo carries the placeholder |
| R3 | Run 1 saves its placeholder and leaves a background loop; run 2 in the same task; another task of the same sender uses the saved placeholder; a straggler after the run | Run 2's own placeholder works, run 1's is `expired` (run 1 has ended: a task has one run at a time, and `expired` is checked before `foreign_run`); `foreign_run` only while both runs live, covered by unit tests (GW-U-110); the other sandbox `foreign_sandbox`; after the run `expired`; nothing reaches the host on a refusal |
| R4 | 25 keyed requests, 3 refused ones, then one more with the audit table unwritable | One `ok` row counting 25 with sender, run and host; refusal rows by outcome; the last request refused and nothing sent |
| R5 | Keys for an allowed and a not-yet-allowed host; the list changes during a run; `git clone` and `push` with a `basic` key and a path prefix, then a push outside it | No placeholder for the unlisted host until the next run after it's allowed; a removed host refused `not_allowed`; clone and push inside the prefix succeed, outside it `wrong_host`; no `git_push` in the catalogue |

Supporting: the trust matrix on both backends (Python `urllib` and `requests`, `pip`, Node, `curl`, git reach keyed
hosts; an unkeyed host shows its real certificate; an invalid upstream certificate is refused); the header rule
(unit and fuzzed, HTTP/1.1 and HTTP/2); scrubbing across chunk boundaries; a gateway restart during a run; a spawned
agent gets at most its caller's keys; every UI refusal has its words, a unit test and a fake-front e2e.

## Links

- PRD FR-19, FR-28; ARCHITECTURE §5.4, §10, §12, §18
- ADR-0017 (licence policy), ADR-0023 (amended), ADR-0026 (runtime protocol revision), ADR-0028 (runtime-mutable
  settings), ADR-0033 (amended), ADR-0034 (team runs), ADR-0035 (amended)
- TEST-PLAN SEC-019, TOOL-U-020, TOOL-S-021, RT-I-016, GW-I-014, GW-I-151
