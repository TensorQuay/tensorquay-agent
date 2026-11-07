# ADR-0012: Agent profiles: one record that narrows the install, copied into each conversation

## Status

Accepted

## Date

2026-09-27

## Context

FR-23: "A profile defines instructions, model route, allowed tools and connectors, skills, permission policy, sandbox template and
budgets. Profiles are validated files or admin-UI records, shared or personal; a conversation is created from a profile and keeps it."
ARCHITECTURE §15 makes the profile the unit of composition: a gateway `agent_profiles` record mapped to `SandboxSpec` and the runtime's
run configuration. A future swarm starts sub-agents from profiles (§16.2).

Today every conversation shares one install-wide configuration. The permission policy (`[permissions]`) and the run limits
(`run_limits`) come from gateway.toml. MCP servers and connectors start from their enabled-by-default sets. The template is chosen per
conversation (ADR-0032). The system prompt is the base text plus the workspace's `AGENTS.md` files. No `session.config` field carries
instructions, a tool allow-list or a skills filter.

## Decision

1. **One record, `agent_profiles`.** It holds:
   - `id`, and a `name` (template-name rules) unique per owner and among shared profiles;
   - `display_name`, and an optional `description`;
   - `owner_id`, which is null for a shared profile;
   - optional `instructions` (≤ 16 KiB UTF-8 without NUL);
   - optional `model_route` and `template`;
   - an optional `tools` allow-list of built-in tool names (≤ 64);
   - `skills`: `all`, `none` or ≤ 64 names;
   - `mcp_servers` and `connectors`, at most 8 together (the runtime's bound). An empty or absent list means none: a conversation
     made from the profile starts with no servers or connectors, and a PATCH can't add any;
   - an optional baseline `policy` and ≤ 64 `rules`;
   - any subset of `RunLimits`;
   - an optional `busy_policy`;
   - a `version` and ADR-0028's seed columns.

   One validator serves the file and the API, with field-level reasons. Names are checked at every write (`422 unknown_reference`) and
   again at use.
2. **API and files.**
   - The routes are `GET`/`POST /api/v1/profiles`, `GET`/`PUT`/`DELETE /api/v1/profiles/{id}`.
   - Writes are versioned, take an `Idempotency-Key`, and are audited without values.
   - `[[profiles]]` in gateway.toml seeds **shared** profiles only, with ADR-0028's keyed seed digests and tombstones. Personal
     profiles are user data, never configuration.
3. **Personal and shared (D1).**
   - Any user creates personal profiles, which only they see and use.
   - Administrators create shared profiles, which every member sees and uses.
   - There is no sharing with named users in v1. A member copies a shared profile into a personal one to change it.
4. **Narrow, never widen.** A profile only takes away from what the install allows. It narrows what the install leaves to its
   baseline, and denies anything it lists:
   - the effective policy baseline is the stricter of the two. The engine orders the baselines for every class and fact
     (`read_only` asks for a read outside the workspace, as `workspace_write` does; TOOL-U-014);
   - a profile's rules can only ask or deny; an `allow` is refused when the profile is written (`rules_invalid`);
   - the effective rules are the profile's denies, then the install's rules in their order, then the profile's asks. So an
     install's carve-out (an allow before its deny) stays intact, and an install's explicit allow beats a profile's ask, being
     the administrator's decision; a profile that wants to block uses deny. Revision 8 raises the wire's bound on a
     session's rules to 192, so an install's 128 and a profile's 64 always fit (`MAX_POLICY_RULES`);
   - the merge happens at each session, from the conversation's copy and the install's settings then, so the install's later
     changes still apply. The MCP proxy classifies with the same merge;
   - each limit is the smaller of the two;
   - servers and connectors must be within what the install enables;
   - tools and skills only narrow the runtime's own registry and index.

   A conversation request cannot override a field the profile sets, and a later PATCH can only narrow the profile's servers and
   connectors (`422 set_by_profile`).
5. **The conversation keeps a copy (D3).**
   - At create, the conversation stores a copy of the profile's validated fields with `profile_id` and `profile_version` (≤ 64 KiB,
     the profile's own bound). Every reference is checked: the route exists, the template is offered, the servers exist and the
     connectors exist and are enabled. The first failure is `409 profile_unusable` with the field; the profile list shows the same
     state as `usable: false` with `unusable_field`.
   - Later edits or deletion of the profile don't change existing conversations.
   - The copy is re-checked at each session, never re-read from the profile. A route that has gone fails as a missing route does
     today; a connector or server that has gone is dropped with a notice.
   - `409 template_in_use` names the shared profiles that name a template, and counts the personal ones (their owners alone see
     them).
6. **Mapping.**
   - `SandboxSpec` changes only through the template (ADR-0032). A profile adds no mount, secret or privilege, and egress stays
     install-wide (D4, deferred to a later amendment).
   - The run configuration uses the existing `session.config` fields for the effective `policy`, `limits`, `route` and `mcp`.
   - A new `profile` field carries `ProfileSnapshot {name, instructions?, tools?, skills}`:
     - the core registers only the allowed tools, and answers any other as unknown;
     - the prompt adds the instructions as their own section after the base text and before `AGENTS.md`;
     - the skills filter trims the skill index and the `skill` tool.
   - That field is **protocol revision 8** (D2), one bump commit, with ADR-0032's environment `version`. It is sent
     only to runtimes at revision 8 or above.
7. **With ADR-0031 and ADR-0032.**
   - The MCP proxy's allow-list stays the conversation's `connectors`, filled from the profile. Per-user credentials, approval classes
     and rate limits don't change.
   - A profile's template must be offered at create. An environment switch on a conversation whose profile sets a template is refused
     (`422 set_by_profile`).

## Consequences

- Users compose agents without an administrator, yet can never grant themselves more than the install allows.
- Conversations keep the profile they were created from, as FR-23 and GW-I-025 require, while install-level changes still apply at
  each session.
- Revision 8 means rebuilding runtime images, and older runtimes are refused with `426` (ADR-0026).
- Per-profile egress (ARCHITECTURE §10's Phase 6 note) is deferred. Until it lands, every profile shares the install's `[egress]`.
- **Tests:** GW-U-049, GW-I-025, GW-I-085, GW-I-086, SEC-028, PROTO-U-018, RT-I-028, CORE-U-031, E2E-PROFILE-007, WEB-E2E-010.

## Alternatives

- **Profiles as live references.** Rejected: a profile edit would change running agents' behaviour, and FR-23 says the conversation
  keeps its profile.
- **Instructions as workspace files** (`AGENTS.md`-like). Rejected: the model can edit the workspace, so the profile's instructions
  would not be the profile's.
- **Profiles that can widen the install** (for example a higher step budget). Rejected: a personal profile would then grant its owner
  more than the administrator allows.
- **Sharing with named users** (a share table like workspace shares). Deferred: install-wide shared profiles cover the need at far
  less cost.

## Links

PRD FR-9, FR-23, FR-24, FR-25, FR-32; ARCHITECTURE §4, §9, §10, §15, §15.1, §16.2, §18; ADR-0026, ADR-0028, ADR-0029, ADR-0031,
ADR-0032.
