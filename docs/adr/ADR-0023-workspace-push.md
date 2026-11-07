# ADR-0023: Pushing from the workspace: approval floor and credential boundary

## Status

Proposed; superseded in part by [ADR-0035](ADR-0035-network-guardrails.md) §5 (2026-09-29). A push needs no approval,
and the relay has no branch rules of its own: it no longer refuses protected branches or updates of an existing branch
(a new branch's zero old object ID is no longer required), and the Git host's rules decide. The credential boundary,
the single-operation authorization bound to the exact journaled call, and the refusal of deletes, tags, multiple refs
and push options stand.

## Date

2026-09-24

## Context

Pushing publishes a workspace commit outside its sandbox. Approval of shell command
text does not bind the resolved repository, branch and commit. A Git credential
must remain in the gateway: the entire sandbox, including its tools, is untrusted.

A new builtin requires an explanation of why the MCP boundary is insufficient.
This operation needs the core's exact prepared-action identity, journal recovery
and a gateway-owned credential grant. An MCP wrapper would need the same trusted
integration. The builtin is a small adapter over those boundaries, not a general
Git API; a future MCP frontend can reuse the relay contract.

## Decision

The opt-in `git_push` tool accepts an explicit HTTPS remote, short branch and full
commit object ID. These values and the execution context determine the existing
prepared-action hash. Every permitted push requires human allow-once, including
under Unrestricted. Read-only policy and applicable deny rules still deny. Session
grants and admin allow rules cannot bypass this approval floor.

The gateway stores the user's credential and binds a short-lived, single-operation
authorization to the approved tuple and runtime. A narrow Git relay enforces that
binding and the egress policy before injecting the credential into upstream HTTPS.
The sandbox receives no provider credential, even temporarily. Git and workspace
hooks never execute on the gateway host.

This version creates new remote branches only. An existing identical commit is a
no-upload success; an existing different commit is refused. The relay permits one
create command with a zero old object ID and the approved new object ID and ref.
Protected branches, deletes, tags, multiple refs and push options are refused.
Only the person who started the run can allow its push: the gateway refuses setup with
`push_decider_mismatch` otherwise, and never uses another user's credential.

The [Git HTTP protocol](https://git-scm.com/docs/gitprotocol-http) carries old/new
object IDs and ref names, not a force flag. Client-side fast-forward checks cannot
constrain an untrusted sandbox. Existing-branch updates require a later design
that proves the fast-forward invariant at the trusted boundary.

## Consequences

The core journals approvals and tool lifecycle events. Uploads are not retried
automatically. A lost response or interrupted upload has an unconfirmed outcome;
recovery must not execute it again. Success needs a relay receipt, not merely a
successful local Git exit. Fixed status messages suppress upstream diagnostics.

The direct UI action is separate work and must use the same executor and approval
contract. This decision does not claim the UI push acceptance tests are complete.

## Links

PRD FR-19; TEST-PLAN TOOL-U-020, TOOL-S-021, RT-I-016, SEC-019; ARCHITECTURE §5.4 and §12;
[ADR-0008](ADR-0008-prepared-action-binding.md) (prepared-action approval binding).
