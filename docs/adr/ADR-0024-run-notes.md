# ADR-0024: Run notes as builtins, not MCP

## Status

Proposed

## Date

2026-09-25

## Context

Compaction (ADR-0009) replaces older turns with a structured summary. An agent working on a long task needs a record
it controls, such as the goal, decisions and open questions, that survives every compaction verbatim (ARCHITECTURE
section 7.1, run memory). ARCHITECTURE section 8 asks every new builtin tool to explain why an MCP server is not
enough.

## Decision

- Two builtins, `write_notes {text}` and `read_notes {}`, keep one file per conversation at
  `.tqa/<conversation_id>/notes.md` in the workspace, at most 16 KiB, replaced atomically. `write_notes` is a
  Write-class tool and goes through the permission broker like any other write; `read_notes` is Read-class.
- The model context takes the notes from the journal, not from the file: the latest successful `write_notes` call
  sets them, and after a compaction they are pinned right after the summary as quoted data. Resume and replay are
  therefore exact, and a change to the file by other means does not enter the context.
- The file lets the agent re-read its notes in any later sandbox; the workspace outlives sandboxes.

## Why not MCP

The notes are part of the context-management contract: the projection must recognise them in the event log to pin
them, which ties them to the harness's own tool name and argument shape. An MCP server would need a separate process
in every sandbox, could be absent or renamed by a profile, and its results would be ordinary tool output that the
projection cannot rely on. The feature is small, has no external dependency and no credentials, so it stays a
builtin.

## Consequences

- Long runs keep a stable, agent-written memory across compactions and sandboxes.
- Notes are data to the model; they cannot approve anything or widen permissions.
- Read-only policies deny `write_notes` like any write; an admin rule can allow it if wanted.

## Links

PRD FR-7; ARCHITECTURE sections 7.1 and 8; TEST-PLAN CTX-U-013, CTX-U-014; [ADR-0009](ADR-0009-compaction.md).
