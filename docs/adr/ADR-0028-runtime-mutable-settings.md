# ADR-0028: Runtime-mutable settings

## Status

Accepted

## Date

2026-09-26

## Context

PRD FR-22 asks for admin settings (the model registry, the secret vault, the egress allow-list, sandbox limits per user and usage) and
WEB-E2E-006 expects an admin change to be "effective immediately for new conversations". Today the gateway parses `gateway.toml`
and `models.toml` once at start into an immutable `GatewayConfig` and `Registry`. Only the secret store changes at runtime, and it
has no admin API. The seed file for secrets is re-applied at every start, so a rotation made anywhere else is undone by a restart.

Making configuration mutable touches every place that reads it: admission, the relay, session configuration and the egress proxy.
It needs one rule for what a change affects, one validation path, and an audit trail, so this record fixes them before the admin
APIs are built.

## Decision

- **The database holds the live settings; files seed them.** Model routes, egress rules and limit overrides live in PostgreSQL
  (`model_routes`, `egress_rules`, `settings`), alongside the existing `secrets` table. `models.toml`, the `[egress]` and limit sections of
  `gateway.toml`, and the secrets seed stay supported as seed and as import and export formats (FR-4). Listener addresses, storage,
  the backend, the database and other start-up values stay file-only and immutable.
- **A seed applies only when it changed.** For every seeded item the gateway stores a keyed digest (HMAC with a key derived from the
  install key, never a plain hash) of the seed value it last imported. At start it applies a seed item only when that digest differs, so a
  change made through the API survives a restart and an operator who edits the file still wins. A seed apply bumps the item's `version`
  (so a stale API write then gets `409`) and writes an audit row with actor `seed`, so an admin can see why their change was replaced. An
  item removed from the seed file is kept and marked "no longer seeded"; it is never deleted silently. Every settings item carries
  `seed: "current" | "removed" | null`, so a client can say "from the seed file" or "no longer in the seed file".
- **A deleted seeded item stays deleted.** When an administrator deletes an item that came from a seed file, the gateway keeps a
  tombstone of it (area, item and the keyed digest of the seed value it last imported). At the next start, a seed item whose digest matches
  its tombstone is skipped, so the deletion survives a restart like any other API change. A seed item whose value changed since then (a new
  digest) is imported again and its tombstone removed: an operator who edits the file still wins. A tombstone is dropped once the seed file no
  longer lists the item, so listing it again later counts as a new entry. Re-creating the item through the API carries the tombstone's digest
  over to the new record, so an unchanged seed does not overwrite it either. Tombstones hold no value and no plain hash of one (added
  2026-09-26: before, a deleted seeded item was re-created by the next start's seed import).
- **One validation path.** An API write builds the same typed values the file parser builds (`Registry::load` rules, the config
  schema, the egress host grammar), so a value the file would refuse is refused by the API with the same error, and the reverse.
  Refusals are machine-readable: `400 invalid_request` carries `details: [{field, reason}]`, where `reason` comes from one closed,
  documented list of codes shared by the file and API validators, so clients show translated text next to the field. A route whose
  `key_ref` has no secret yet is accepted and reported (`key_missing: true`); the relay's existing refusal stays the runtime guard.
- **One snapshot, swapped atomically and never backwards.** The gateway keeps an immutable `SettingsSnapshot` (routes, egress rules,
  limits) behind a read-mostly `RwLock<Arc<SettingsSnapshot>>`. Rebuild-and-swap is serialised behind one async mutex (writes are rare),
  and each snapshot carries a generation read in the same transaction; a swap only ever installs a newer generation, so two commits that
  finish rebuilding out of order cannot leave the older one live. A reader clones the `Arc` and drops the guard at once, so no guard is held
  across an `.await`; it uses that `Arc` for its whole operation, and no request sees half a change. No new dependency is needed. The
  snapshot is coherent for one gateway process, which is the supported deployment; several processes would need a change notification
  (for example PostgreSQL LISTEN/NOTIFY), decided if that deployment is ever supported.
- **What a change affects.** New conversations, new sessions (a sandbox created or woken) and new admission decisions use the
  current snapshot. A running sandbox keeps the route its `session.config` carried, and the CPU, memory and pids limits it was created with
  (limit overrides apply at the next admission or creation only); a route change reaches it at the
  next session, as `session.update` does today for an idle route change (GW-I-044). The relay resolves a route by name against the
  current snapshot on every request, so a rotated key or a changed base URL applies at once; a deleted route fails that request with
  a plain error and the run follows the existing fallback rules (ADR-0025). A route's `name` is an immutable key, because conversations store it; routes gain an editable
  `display_name` for the pickers, and there is no rename.
- **Egress.** The policy proxy listener always runs; an empty allow-list denies everything, instead of there being no proxy. The proxy
  decides every NEW connection against the current snapshot, so removing a host denies the next connection from every sandbox at once. Every
  sandbox is attached through the proxy from creation, so the allow-list alone decides, and adding the first rule reaches long-lived
  sandboxes too. The only exception is an explicit, file-only operator setting that disables egress entirely for locked-down installs:
  sandboxes are then internal-only, no allow-list applies, and the admin page says so. A tunnel
  already established to a host that was just removed runs to its existing idle and lifetime bounds and is not cut.
- **Concurrency.** Each record carries a `version`. Writes send the version they read, and a stale write gets `409 conflict`. There is no
  last-writer-wins across admins. A retried write with the same `Idempotency-Key` replays the stored response before the version is
  checked, so a retry after a success never turns into a `409`.
- **Audit.** Every change, by an admin or by a seed apply, writes one row in `admin_audit` (actor, time, area, item, action, the before and
  after versions). No value, and no hash of a secret value, is stored there; the versions are enough to tell changes apart. Secret values never
  appear in the audit, logs, events or responses (FR-28).
- **Access.** `/api/v1/admin/*` is administrators only (members get 403, GW-I-003). Secret writes need a signed-in browser session;
  route, egress and limit writes also accept an administrator's API token (FR-26 parity). Every write accepts `Idempotency-Key` and follows
  the same origin and body-size rules as the rest of the API.

## Consequences

- WEB-E2E-006 becomes achievable, and the admin pages (models, secrets, egress, limits, usage) sit on one pattern.
- Restarting the gateway no longer undoes admin changes, and a file edit still takes effect: the rule is explicit and tested.
- Every reader of configuration moves from the start-up struct to the snapshot. This is the main cost, done area by area: secrets and
  usage first (no snapshot needed), then routes, then egress, then limits.
- A route deleted while in use fails requests plainly rather than being kept alive; fallback routes are the mitigation.
- Tests: seed-change semantics (unchanged, changed, API-changed then restarted, a deleted seeded item that stays deleted across a restart
  and comes back when its seed entry is edited, per area, GW-I-065; a removed seed item kept and marked, the seed overwrite's
  audit row, keyed seed digests that differ from a plain SHA-256); the same validation errors from the file and the API; snapshot atomicity
  under concurrent readers and writers, and a swap that never goes backwards when commits finish rebuilding out of order; `409` on stale versions
  and an idempotent replay that is not a `409`; an allow-list removal that denies the next new connection from an existing sandbox while its
  attachment is unchanged; the audit row for each action without values; plus the WEB-E2E-006 flow.

## Alternatives

- **Reload the files on a signal.** Rejected: the API would then have to write the files, which are operator-owned and often read-only
  mounts, and a partly written file would be a new failure mode.
- **Database only, no files.** Rejected: installs, tests and backups rely on declarative files (FR-4), and the first start needs a route.
- **Apply changes to running sandboxes.** Rejected for routes and egress: a run would change model or network mid-step. The existing
  idle-update path is enough.

## Amendment: routes are the administrators' (1 Oct 2026)

The product owner decided there are no personal models: "only administrators add models; members choose from them"
(PRD §12 item 14, 1 Oct 2026; FR-4). So routes stay among the settings only administrators change: `/api/v1/admin/routes`
refuses a member's create, edit, delete and probe with `403`, by session or API token, and records nothing. A member's
task binds or switches only to a route an administrator made. Personal routes, which FR-4 and ARCHITECTURE once
allowed "when allowed", are not built. TEST-PLAN GW-I-038.

## Amendment: a file route's display name (5 Oct 2026)
A route in models.toml may name its `display_name` (the page pass's N2). The seed stores it in the route's
display-name column, never in its record, under the routes API's rule; an unclean one stops the start. It is part
of the seed record's digest only when set, so a route without one keeps the digest it had and an upgrade seeds
nothing again. A changed name is a seed change like any other; a file without one leaves an administrator's name
as it is (GW-I-216, LLM-U-051).
