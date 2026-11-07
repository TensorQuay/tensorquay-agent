# ADR-0027: Opaque reasoning state

## Status

Proposed

## Date

2026-09-25

## Context

Some model protocols return reasoning as an opaque item the model continues from when it is sent back:
- On the Responses protocol, reasoning items carry `encrypted_content` when a stateless request asks for it.
  Sending them back is optional, but without them a reasoning model re-reasons at every step of a tool loop.
- On the Messages protocol, the assistant turn that holds tool calls must be sent back with its thinking blocks (text
  and signature) unchanged while thinking is enabled.

Requests are built from our own history, statelessly (the journal is the authority and a resumed run must send what
the history says), so this state has to live in the journal. The items are large, opaque and meaningless to people.

## Decision

- **One event for both protocols:** the sequenced event `reasoning.state { protocol, route, model, bytes, item }`.
  - `item` is one provider output item as JSON object text, stored as received and never interpreted: a Responses
    reasoning item without its `status`, or a Messages thinking block.
  - It is recorded when the response completes, in stream order, before that response's `tool.call` events.
- **Bounds:**
  - at most 128 KiB per item, and at most 16 items and 512 KiB per step;
  - an item whose escaped form would not fit in one event frame is also not kept.
  - A state over a bound is still recorded, with `bytes` but without `item`, so a later request knows the state
    was lost. There is no separate notice event.
- **Replay key: the same route, protocol and model.** Opaque state is tied to the endpoint and model that produced
  it, so replaying it elsewhere would be refused permanently.
  - The request builder puts the matching items first in the assistant message they belong to.
  - Other routes never receive them, which covers every fallback.
  - A run that later returns to the route reuses its states.
- **Compaction** drops the states of the turns it summarises, and retained turns keep theirs. The compaction request
  itself sends none. Amended 30 Sep 2026 (CORE-U-074, CORE-U-044): the window estimate counts the states a request
  replays, since they are sent. When a request would pass the compaction threshold, the oldest replayed states are left
  out of it first, message by message, before any turn is folded (a provider accepts a history without them; that
  turn's thinking is lost to the model, never to the journal). The latest assistant message's states always go, since
  its tool calls need them: they narrow the output like any input, but never fail a request or call for compaction on
  their own.
- **Protocol adapters:**
  - Responses requests `include: ["reasoning.encrypted_content"]` only when the route has a reasoning policy,
    records only items that carry encrypted content, and replays each as an input item before its response's
    message and calls.
  - Chat has no reasoning items; its only state is a tool call's opaque extra (see the amendment below).
  - Messages (a later change) places them first in the assistant content, and omits `thinking` for the one request
    whose latest tool turn lost its state.
- **Clients never download items:**
  - the events API and SSE deliver `reasoning.state` without `item` (`ServerFrame::for_clients`);
  - only the runtime's history (`/runtime/history`, the resume path) returns items, byte for byte;
  - exports show only the size;
  - the item's `Debug` form shows only its size, and no log or diagnostic prints it.
- **Protocol revision 5:** the gateway sends history containing the event to the runtime, so the revision rises
  (ADR-0026) with the table row, the floor and the runtime image label.

## Amendment (2026-09-27): Chat tool-call extras

Some Chat endpoints attach opaque data to each tool call (`tool_calls[i].extra_content`) and refuse the next request
of the turn when it is not sent back on that call (observed live: a 400 "Function call is missing a thought_signature
in functionCall parts"). It is the same kind of state, so it uses the same event, bounds, replay key, compaction rule
and client rule as above:
- **Item:** `{"type":"chat.tool_call_extra","call_id":<the call's id>,"extra_content":<the object as received>}`, one per
  call that carries an object `extra_content`, whatever its inner keys; never interpreted. Observed live on a compatible
  endpoint (2026-09-27): one leaf, `extra_content.google.thought_signature`, 438 bytes, on the stream's tool call.
- **Decode:** the Chat decoder matches an extra to its call's id by the call's index, whether it arrives with the id,
  before it or after it. A call that never gets an id keeps no extra.
- **Encode:** each item goes back as `extra_content` on the assistant `tool_calls` entry with the same id. An item whose
  call is not in that message is left out, never put on another call.
- **Route probe:** its round trip replays the first answer's states the same way; its observation shows only the extra's
  leaf paths and sizes (`tool_call_extras`).
- **No protocol revision:** the event and its schema are unchanged, and the gateway already stores and returns a Chat
  state (GW-I-051); an older runtime simply leaves the states out, as before.
- **Limitation, fallbacks:** a run that reaches such an endpoint in the middle of a turn, by a fallback or a resume on
  another route, has tool calls from another route without their extras. The endpoint refuses that request
  permanently, and the refusal carries its words. No placeholder value is sent; this is revisited only if the provider
  documents one for history it did not produce.

## Consequences

- Reasoning models keep their reasoning across a tool loop on the Responses protocol, and the Messages protocol can
  use thinking with tools.
- The journal and the gateway store grow by up to 512 KiB per step on reasoning routes. Clients are not affected.
- A route switch or a compaction loses reasoning continuity, but never correctness.

## Links

ARCHITECTURE sections 5.2, 6, 7 and 15; TEST-PLAN PROTO-U-009, LLM-C-025, CTX-U-016, CORE-U-044, GW-I-051, SEC-021,
LLM-U-043, GW-I-037;
[ADR-0009](ADR-0009-compaction.md), [ADR-0019](ADR-0019-transparent-relay.md),
[ADR-0025](ADR-0025-fallback-routes.md), [ADR-0026](ADR-0026-runtime-protocol-revision.md).
