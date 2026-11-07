# ADR-0029: A builtin `skill` tool for progressive disclosure

## Status

Proposed

## Date

2026-09-26

## Context

PRD section 6.1 and FR-11 make skills a core feature: a skill is a folder with a `SKILL.md` and optional helper files,
installed per install or per user, needing no core code, and shown in the context panel. Skills are discovered at run
start from read-only mounts at `/skills/install` and `/skills/user` in the sandbox (ARCHITECTURE sections 12 and 15).

Putting every skill's instructions into the prompt does not scale: 64 skills of up to 16 KiB each would fill a 32k
local window before the task starts. The prompt therefore carries only a byte-stable index, one line per skill with its
name and description, capped at 8 KiB (PROMPT-U-002). The model needs a way to load one skill's instructions, and to
learn where its helpers are, when it decides to use it.

DEV-GUIDELINES rule 21 requires an ADR for a new builtin tool, explaining why an MCP server is not enough. Two
alternatives exist: widening `read_file` to read the skill mounts, or serving skills from an MCP server.

## Decision

- A builtin tool, `skill {name}`, Read-class and parallel-safe, returns the named skill's `SKILL.md` body and the
  absolute paths of its helper files, at most 100 listed with a count of the rest. Helpers run through the existing
  shell tool by absolute path; the mounts are read-only, so a helper cannot be changed.
- The tool serves only the skills discovered at run start, the same set as the index. An unknown name is an argument
  error that lists the available names, bounded. A skill is reloaded with the discovery checks (manifest bound,
  folder bound, no symbolic link anywhere, the name still matching the folder), so a folder changed after run start is
  refused rather than trusted.
- The tool lives in `tqa-prompt` beside the parser and implements the `tqa-tools` `Tool` trait. `tqa-tools` cannot
  depend on `tqa-prompt` (G-07), and one parser keeps discovery and loading consistent. The runtime registers it only
  when the run has at least one skill, so runs without skills keep their tool catalog unchanged.

## Why not widen `read_file`

`read_file` is confined to the workspace, and path confinement is one of the strictest areas in the guardrails
(100 % coverage, symlink-escape tests). A second root would add a class of paths that is outside the workspace yet
allowed, which every path check, permission fact (`targets_outside_workspace`) and test would have to learn. It would
also give the model a general reader of the mounts, not a skill-shaped result: no helper list and no check that the
folder is still a valid skill. A narrow tool with one argument, a name from a fixed set, cannot be steered to any other
path.

## Why not MCP

- Skills are a core feature (FR-11), not an optional integration. They must work in every sandbox, including one with
  no MCP servers configured and a profile that allows none.
- The index must be byte-stable in the prompt so that prompt caching holds across steps (PROMPT-U-003). The harness
  builds the index from the same discovery that the tool serves. An MCP server's tool list and descriptions are
  supplied at run time by a separate process and could drift from the index, change order or disappear.
- The context panel and the `context.loaded` event report the skills and the skipped folders from the same
  discovery; an MCP server would need a second channel for that.
- The feature is small, has no external dependency and no credentials, and stays inside the sandbox, so a separate
  process in every sandbox would add cost without adding isolation.

## Consequences

- The prompt stays small: an index of at most 8 KiB, with each skill's instructions loaded only when used.
- One more builtin in the catalog when skills exist; none when they do not.
- The tool is read-only and cannot widen permissions. Read-only policies allow it like any other Read-class tool.
- A change to a skill takes effect at the next run; during a run a changed folder is refused.

## Links

PRD section 6.1 and FR-11; ARCHITECTURE sections 3, 8, 12 and 15; DEV-GUIDELINES rule 21; TEST-PLAN PROMPT-U-002,
PROMPT-U-003, PROMPT-U-005, PROMPT-U-006 and TOOL-U-024; [ADR-0024](ADR-0024-run-notes.md) (a builtin rather than
MCP for a similar reason).
