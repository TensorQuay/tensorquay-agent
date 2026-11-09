# ADR-0036: Managing skills in the product (amends ADR-0029)

## Status

Proposed (draft for the Product Owner; slice 1 only). No code before the Product Owner's OK.

## Date

2026-09-29

## Context

Skills work inside the agent (ADR-0029): a skill is a folder with a `SKILL.md` (name, description, instructions) and
optional helper files; the prompt carries a byte-stable index capped at 8 KiB, and the `skill` tool loads one skill when
the model decides to use it. Today they reach a sandbox only as read-only mounts of server folders: the install tier
(`[skills] root` in `gateway.toml`, `/skills/install`) and a per-person folder in the gateway's storage
(`skills/users/<id>`, `/skills/user`) that nothing can write. Nobody can add, see or share a skill from the product;
agent profiles name skills by hand; microVM sandboxes get no skills at all (SEC-023). The PRD has skills per install,
team or user (FR-11, section 6.1, FR-23) but no requirement for managing them.

The product research (Skills management, 29 Sep) proposes a Skills page where anyone adds private skills and
administrators add shared ones, stored by the gateway as immutable versions and delivered read-only to every sandbox.
It keeps the `SKILL.md` format of Anthropic's Agent Skills, so skills move between products. This record decides slice
1, the gateway: tables, versions, the file store, eight endpoints and the mount sets. Slice 2 (the web), slice 3
(microVM delivery) and slice 4 (the Context panel and guides) follow it.

## Decision

### Tiers and names
- Three tiers: **personal** (one person's, visible to them alone, administrators included, as personal agent
  profiles are), **shared** (administrators write, everyone reads) and **server** (the install folder, read-only in
  the product, listed as "From the server").
- A name is 1 to 64 characters of lowercase letters, digits and hyphens, and matches the `SKILL.md` name. It is unique
  per owner among personal skills, and among shared skills. A personal skill may share a name with a shared or server
  one: the most specific tier wins in the sandbox (personal, then shared, then server), and the loser is reported as
  shadowed, as discovery already does.

### Tables and versions
- `skills`: `id`, `tier` (`personal` | `shared`), `owner_id` (null for shared), `name`, `description` (at most 1,024
  characters, from the current version's `SKILL.md`), `current_version`, `created_at`, `updated_at`.
- `skill_versions`: `id`, `skill_id`, `number` (1, 2, …), `size_bytes`, `file_count`, `digest` (SHA-256 of the
  normalised bundle: sorted paths, modes and contents), `created_by`, `created_at`. A version is immutable; every
  upload or edit is a full new version and becomes current. The last 20 versions of a skill are kept.
- Files: `storage/skills/versions/<version id>/`, the unpacked, checked folder, written once (to a temporary folder,
  then renamed into place) and read-only after. A download is rebuilt from it as a zip.
- Server-folder skills are not in the tables; the list endpoint reads them from the folder (read-only).

### Checks on every write (upload, edit, import)
A `SKILL.md` at the top with a valid name and description (YAML front matter, as today's parser reads it); at most
10 MB unpacked and 500 files (the Product Owner decides 10 or 30 MB); no symbolic or hard links, no absolute path, no
`..`, no special files; a zip is unpacked with the same bounds, so a zip bomb fails on its unpacked size; path names are
UTF-8 without control characters. Each problem is named in `skill_invalid`'s details.

### The eight endpoints (`/api/v1/skills`, session or personal token)
| Method and path | What it does | Who |
|---|---|---|
| `GET /skills?tier=mine\|shared\|server` | List: name, description, current version, updated, and the agent profiles that use it | Everyone; `mine` is the caller's own |
| `POST /skills` | Create from a zip or path-qualified files (multipart), or from JSON (name, description, instructions); `shared: true` for a shared one | Everyone for personal; administrators for shared |
| `GET /skills/{id}` | One skill: its files (paths and sizes) and versions | Its owner; everyone for shared |
| `POST /skills/{id}/versions` | A new full version; the name must stay the same | Its owner; administrators for shared |
| `GET /skills/{id}/versions` | Version history: number, when, by whom, size | Its owner; administrators for shared |
| `GET /skills/{id}/download?version=` | A version as a zip (default: current) | Its owner; everyone for shared |
| `DELETE /skills/{id}` | Delete, refused while an agent profile names it (`skill_in_use`, naming the profiles the caller may see) | Its owner; administrators for shared |
| `POST /skills/check` | Validate an upload without saving; returns its problems | Everyone |

(The research listed the dry run as `GET`; it carries the upload body, so it is a `POST`.) Writes are versioned and
retried like the other settings APIs (an `Idempotency-Key` replays; a stale version is `409 conflict`). New codes:
`skill_invalid` (with each problem), `skill_too_large`, `skill_name_taken`, `skill_in_use`. Shared-skill writes are in
the admin audit (area `skills`, no file contents); personal skills are their owner's data and are not audited, as
personal profiles. Agent profiles keep their `skills` field (all, none, only these names); names now resolve across
the caller's personal and the shared skills, and a profile naming a skill that no longer exists is unusable with the
field named, as profiles already are for routes.

### Mount sets: what a sandbox gets
- At sandbox creation the gateway resolves the **mount set**: the task owner's personal skills and the shared skills,
  each at its current version, filtered by the task's profile (all, none, only these), plus the server folder. The
  filter moves from the runtime to the gateway, so a sandbox only ever holds skills its task may use; the runtime keeps
  its own filter as a second check.
- Docker: each selected version folder is bound read-only at `/skills/personal/<name>` or `/skills/shared/<name>`;
  the server folder stays at `/skills/install`. Nothing is copied, and a running sandbox keeps exactly the versions it
  started with, since version folders never change. The runtime discovers `/skills/shared` and
  `/skills/personal` beside `/skills/install`; `/skills/user` goes.
- MicroVM (slice 3): the same set, packed into a small read-only ext4 image keyed by the set's digest, cached and
  reused, attached as a read-only drive and mounted at `/skills` by the VM's init.
- `sandbox_instances.skill_set` records the set (tier, name, version id) a sandbox was created with: the Context panel
  shows what was offered, and a version is removed from disk only when it is past the last 20 and no live sandbox's set
  names it.

### Upgrade
At the first start, each folder under `storage/skills/users/<person>/` that passes the checks becomes that person's
skill at version 1 (its folder copied into the version store); a folder that fails stays where it is, is logged with
its reasons, and is listed nowhere. The old per-person folders are then no longer mounted. The server folder is
unchanged.

## Consequences
- People and administrators manage skills without server access; the format stays portable.
- Delivery on both backends from one resolved set; microVM installs get skills (closing SEC-023's limit) in slice 3.
- A skill is text and helper files the agent reads in its sandbox: no new guardrail. The checks keep uploads
  well-formed; the network allow-list still bounds any helper that calls out (ADR-0035); a credential in a skill file
  is plain text the agent can read, which the upload screen warns about.
- Skills belong to people, not workspaces: a shared workspace never carries one member's private skills into another
  member's task.

## Tests (slice 1; claimed when built)
Unit: names, the upload checks (links, `..`, special files, sizes, file count, zip bomb, a bad `SKILL.md`), the bundle
digest, tier precedence. DB: the eight endpoints (access per tier, versions, idempotent retries, stale versions, the
audit), `skill_in_use`, the mount set per profile choice and per owner, a running sandbox keeping its versions after
an edit, version retention with a live set, the upgrade import (good and bad folders).

## Decisions for the Product Owner
- Go ahead after the guardrail work (candidate 9)?
- Size limit per skill: 10 MB unpacked (proposed), or 30 MB as Anthropic's?
- May members share a skill with everyone, or administrators only (proposed)?
