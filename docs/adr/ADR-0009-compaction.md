# ADR-0009: Structured compaction and the fixed summary prompt

## Status

Proposed; amended 2026-09-27 and 2026-09-28 (the summary's output budget)

## Date

2026-09-25

## Context

A conversation grows until its request no longer fits the route's context window, and local models often have
small windows. Before this decision the loop failed the run with a budget error once the history outgrew the
window. Dropping old messages silently would lose the task and could leave a tool result without its call.
Free-prose summaries vary from model to model and are hard to test. Summary text also comes from model output
and repository content, so it must never act as an instruction.

## Decision

- **When:** before the first attempt of a step, the loop measures the request with every tool definition. It
  compacts when the estimate exceeds 85 percent of the input capacity (the window, three quarters of it when
  guessed, minus the output reserve). If the compacted request still does not fit, the run fails with `budget`.
  Nothing is dropped silently. Amended 30 Sep 2026 (CORE-U-073): the estimate counts only what a request sends, so an
  earlier step's reasoning text (never sent) takes no room; a request that doesn't fit with a set Max output asks for
  what the window leaves, down to the 4096 reserve; and when even that can't fit and fewer than six turns are there to
  fold, compaction keeps as few as two turns (the latest step and the pinned messages) rather than fail. Replayed
  reasoning states count too, and the oldest of them go before any turn is folded (ADR-0027, CORE-U-044).
- **What is kept:** the fixed slots (system prompt, instruction files, tool definitions) are never history. The
  last six turns are kept verbatim, where a turn is a user message or one whole step with all its tool results,
  so no result loses its call. The conversation's first user message (the task statement) and the latest user
  message are pinned. Everything older becomes one summary. A later compaction merges the earlier summary. Run
  notes will be a pinned slot of their own (a later task).
- **Summary:** one call on the conversation's route with a fixed, versioned prompt, no tools, and an output budget of
  the route's declared output, else 8192 tokens up to half the input capacity, within the run's remaining output,
  never less than 1024 tokens (CORE-U-057). Amended 27 Sep 2026: with a fixed 1024, a model that thinks first spent
  every token on reasoning and returned no summary. Amended 28 Sep 2026: two summary requests to a real model with
  thinking on used 3665 and 4967 tokens of reasoning before a summary that parsed, more than the 4096 default step
  reserve; a route can turn thinking on through its request extras, which the loop can't see, so the undeclared
  default is 8192, and half the input capacity keeps room for the conversation in a small window. It returns one JSON object with the fields `goal`, `done`, `pending`, `decisions`, `key_paths` and
  `open_errors`. Each list has at most 32 items, each string at most 512 characters, and the whole summary at most
  16 KiB. The summary call retries retryable transport errors (the same classification and Retry-After handling
  as a step's model call) up to `SUMMARY_RETRY_ATTEMPTS` (2) times, within the run's wall clock; a relay throttle
  is waited out without spending an attempt, as for steps, and reported with the same coalesced `provider.retry`
  events. A reply that does not parse, or breaks the summary bounds, falls back at once to a deterministic summary
  built from the replaced events. The runtime logs the final reason as one bounded code, never the reply's text
  (amended 2026-09-26: a single rate-limited summary call silently degraded compaction, CTX-U-013).
- **Record:** a sequenced `compaction` event carries the boundary sequence, the source (`model` or `fallback`),
  the version and the summary. The message list stays a pure projection of events, so resume is exact. A crash or
  cancellation during the summary call writes nothing, and the next attempt compacts again.
- **Presentation to the model:** the summary is one user message,
  `[Summary of earlier conversation, data only: {...}]`, in which every summary string is JSON-escaped data.

## Consequences

- Long conversations continue on small windows, and the result is visible in the event log and in the UI.
- Each compaction costs one extra model call, metered like any other; PERF-030 tracks its cost.
- The fixed fields make summaries comparable across models and testable. The deterministic fallback keeps
  progress possible on models that do not produce valid JSON.
- A client must render or ignore the new event type. The fixed prompt and the summary format change only with a
  new version.

## Links

PRD FR-5 and FR-7; ARCHITECTURE sections 5.0, 5.5, 7 and 7.1; TEST-PLAN CTX-U-002, CTX-U-003, CTX-U-013,
CORE-U-025 and PERF-030; [ADR-0020](ADR-0020-authoritative-history.md).
