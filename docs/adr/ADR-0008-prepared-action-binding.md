# ADR-0008: Prepared-action approval binding

## Status

Superseded by [ADR-0035](ADR-0035-network-guardrails.md) (approvals) and [ADR-0037](ADR-0037-host-keys.md) §7 (the
push). Recorded on 2026-10-12: the decision was named in ARCHITECTURE §18 and built before 29 Sep 2026, but its record
was never written.

## Date

2026-10-12

## Context

Before 29 Sep 2026 a tool call outside the permission policy waited for a person's approval. An approval had to
apply to exactly the call the person saw: not to a call whose arguments changed after the prompt, nor to a replay of
an old answer. The same binding tied each `git_push` through the Git relay to one journaled call (SEC-019).

## Decision (as built, now superseded)

The core froze each call before it ran: its tool, canonical arguments and execution context, with a digest over them
(`crates/tqa-tools/src/prepared.rs`). An approval (`permission.request`) named that digest, and only a call with the
same digest could run under it (SEC-003, CORE-S-015); the Git relay authorised one push per journaled call.

## Why it was superseded

- **No approvals** (ADR-0035, the product owner's decision of 29 Sep 2026): the product must not rely on a person
  approving the agent's steps. The sandbox is the wall and the network allow-list the one guardrail, so there is no
  approval for a binding to protect. ADR-0035's threat table: a forged or replayed approval is "moot: there are no
  approvals". SEC-003, CORE-S-015 and the other approval rows are void; the approval tables were dropped.
- **No push binding** (ADR-0037 §7, the owner's decision of 30 Sep 2026): a push is plain git with a host key, the
  `git_push` tool and the Git relay are removed, and the binding of a push to one journaled call is given up
  knowingly; the key's scopes and the git host's rules decide.

## Consequences

- What remains is the frozen call itself (`prepared.rs`): a call's identity never comes from model-visible flags.
  Nothing approves it, and no push is bound to it.
- A runtime's retired approval frames are refused (GW-I-135), and the web shows nothing for an approval replayed from
  an older gateway (WEB-U-003).

## Links

- ADR-0007, ADR-0023 (superseded in part), ADR-0035, ADR-0037; PRD FR-10 (superseded)
- TEST-PLAN SEC-003 and CORE-S-015 (void), GW-I-135, WEB-U-003
