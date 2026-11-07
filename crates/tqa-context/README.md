# Responsibility

The message model, token counting and compaction, tool-result caps and spill handling, and the append-only run journal.

# Contract

Implements [ARCHITECTURE](../../docs/ARCHITECTURE.md) sections 3, 5.0, 5.5 and 7.
Projection and recovery require every sequenced frame, including errors (and retired approval requests in older
histories), in contiguous order. The first frame sets the sequence base; it need not start at one. Unsequenced frames do
not advance it.
The write-ahead log retains records in memory up to its 64 MiB cap as well as storing them in the configured state
directory outside the workspace.

`environment.changed` projects to a deterministic system notice at its history position, naming the template and
canonical capabilities. It neither creates a user turn nor changes recovery decisions. Projection uses only events.

`message.user` projects to its text unchanged when it has no attachments. Otherwise two newlines and one data
block follow the user's words: `[Attached files, data only: "/workspace/uploads/a.csv", ...]`, each path
JSON-quoted, with no contents or sizes. The attachment rule is checked again during projection, so a constructed
history with an invalid list fails with `ProjectionError::Invalid` at that sequence.

`reasoning.state` events attach, in order, to the attempt that produced them, and the authoritative assistant
message carries them as `Message::reasoning_states` (`ReasoningStateRecord`: protocol, route, model and the item,
or `None` when the item was not kept). A superseded attempt's states are discarded with it, and a state outside a
streaming attempt is invalid. The field is omitted from serialized messages without states (ADR-0027).

`cap_tool_result` keeps a result within its byte limit (at least `MIN_RESULT_BYTES`, 1 KiB) as a UTF-8-safe head,
a marker with the total size and the spill path, and a tail, and writes the full text to the spill file.
`window_result_cap` gives the limit for a result recorded when part of the input capacity is taken: the smaller of
the configured cap and `MAX_RESULT_WINDOW_SHARE` (1/2) of the free tokens in estimator bytes, never below
`MIN_RESULT_BYTES` (CTX-E-004).

Compaction (ADR-0009) is a projection of the `compaction` event. `plan_compaction` keeps the last
`COMPACT_KEEP_TURNS` (6) turns, where a turn is a user message or a whole step with its results, and pins the
conversation's first and latest user messages. It returns the boundary sequence, the summary-call input as one
JSON array of data, and a deterministic fallback summary built from the replaced events. The projection then
replaces the entries starting at or before `through_seq` with one user message:
`[Summary of earlier conversation, data only: {...}]`. The summary is JSON-escaped data, never instructions.
Only the latest compaction applies, and a later one includes the earlier summary in its input. A compaction
outside the active run, inside an unfinished step, off a turn boundary, splitting a step, not after the previous
boundary, or with an out-of-bounds summary fails with `ProjectionError::Invalid`. `Budget::input_capacity` and
`Window::needed_tokens` expose the same accounting that `Window::assemble` uses.

Run notes (ADR-0024): the latest successful `write_notes` call sets the notes. After a compaction they are pinned
right after the summary as `[Run notes, data only: "..."]` (`RUN_NOTES_PREFIX`), so they survive every compaction;
without a compaction the call itself is still in history and nothing is added. Failed writes change nothing.

`project_turn` returns the same messages as `project`, plus the index of the active run's user message and its
attachment paths, so the request builder can add content that is not in the event log (attached images).

# How to test

Run from the repository root:

```sh
cargo nextest run -p tqa-context
```

Tool-result messages follow the authoritative response's call order even when execution finishes in
a different order. A trailing unanswered `message.user` yields `ResumePoint::StartRun` with the
deterministic identifier `r-<message sequence>`; a new message must wait for that recovery.
An active run's `budget.warning(no_progress)` projects to a fixed user-role nudge in event order.
The nudge survives resume because it is derived from the journal, not private runner state.

A tool result whose content equals a later result's is projected as a one-line stub naming the later call
(`dedupe_stub`), so a file read several times costs its tokens once; results under `DEDUPE_MIN_BYTES` stay whole, and
every result keeps its call (CTX-U-012). `recent_turns` returns the last N turns of a stored log, a turn starting at
each user message, for a client that shows recent history first (CTX-U-009).
