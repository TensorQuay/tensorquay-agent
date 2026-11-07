# ADR-0025: Fallback routes

## Status

Proposed

## Date

2026-09-25

## Context

A conversation runs on one model route. When that route is unavailable for longer than the retry budget, for
example a local server restarting or a cloud route rate-limiting, the run fails even when another configured route
could finish it. Routes differ in capabilities (native tool calls, vision, window size), and budgets must not be
bypassed by switching.

## Decision

- **Configuration:** an admin declares `fallbacks` per route in `models.toml`: at most three other routes, in order,
  validated at load (existing names, not itself, unique, no cycles). A fallback's own fallbacks are not followed.
  The gateway passes to the runtime only the fallbacks the user may use, as secret-free snapshots, and its relay
  accepts only the conversation's route and those fallbacks.
- **When to switch:** after the active route spends its retry budget on availability errors (408, 429, 5xx,
  timeouts, connection failures, cut streams), or at once when a 429 or 503 asks to wait more than 60 seconds.
  Permanent errors, budget stops, cancellation, a text-protocol parse failure and content filters never switch.
- **The relay's own throttle** (a 429 marked `TQA-Relay-Throttle`, added after PERF-020): it is waited out and spends
  no attempt, so only cancellation and the wall clock end the wait. A `user` throttle never switches, because the
  user's rate is shared by every route. A `route` throttle switches when it asks to wait more than 60 seconds, or
  after it has refused a retry budget's worth of attempts in a row. The relay never forwards the header from
  upstream, so a provider cannot turn off switching.
- **Record:** the sequenced event `route.fallback { from, to, reason }`. The step continues with a new attempt on the
  next route; the rest of the run stays there; the next run starts on the conversation's own route.
- **Capabilities follow the active route:** its window and output reserve (the window is re-checked and compaction
  runs if needed before the first request on the new route; a compaction between attempts of an open step is
  therefore allowed), its tool-call mode (the text protocol of ADR-0007 without native tools), its image input (a
  fixed notice without vision) and its reasoning setting.
- **Budgets are per run:** steps, output tokens and wall-clock time continue across routes; every attempt is metered.
- **Resume:** the active route is restored from the journal. If a recorded fallback is no longer configured, the run
  returns to its own route with a `route.fallback` notice (reason `route_unavailable`) and never fails for it.

## Consequences

- Runs survive a route outage when an admin configured an alternative; the transcript says when and why a switch
  happened.
- A switch loses the prefix cache and may compact on a smaller window.
- The relay must authorize a fixed list of routes per conversation instead of one.
- Opaque state does not follow a switch (ADR-0027). On an endpoint that requires each tool call's opaque extra back,
  a switch to it in the middle of a turn is refused permanently for that turn, with the endpoint's words, because the
  earlier calls come from another route and carry none.

## Links

ARCHITECTURE sections 5.3, 6 and 10; TEST-PLAN LLM-U-018, CORE-U-041, CORE-U-042, CORE-U-043, RT-I-018;
[ADR-0007](ADR-0007-tool-call-fallback.md), [ADR-0009](ADR-0009-compaction.md),
[ADR-0019](ADR-0019-transparent-relay.md).
