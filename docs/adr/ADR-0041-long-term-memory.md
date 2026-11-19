# ADR-0041: Long-term memory: notes in files, an index the runtime generates, digests, and history by search

## Status

Accepted for phase 1, sections 1 to 4, 6 and 7 (the Product Owner's decision of 19 Nov 2026: long-term memory is part
of 18.2, "release asap"). Section 5, history (phase 2), waits for the Product Owner's choice of its scope.

## Date

2026-11-20

## Context

Each task starts knowing only its workspace's files and `AGENTS.md`. A preference a person stated, a correction, a
project fact an agent learned the hard way, or a decision made last week is gone at the next task, so people repeat
themselves and agents repeat mistakes. The Product Owner asked for memory that carries across tasks, built the way the
team's own agent sessions keep theirs: one fact per note file, a short index read at the start of every session, the
ordinary file tools, and a search over earlier work. No database of embeddings, and no new tool.

18.1 already keeps one task's own past: each compaction appends the turns it replaced to
`.tqa/<conversation>/history.md` (`tqa_context::history_path`), and the summary tells the model to search it before
answering about anything earlier. This record extends that idea across tasks.

The risks are new: a note persists, so a page or file that tricks one run into saving an instruction would reach
every later run (persistent prompt injection); a personal fact must never leak into a shared workspace or to another
person; and a task without memory must cost exactly what it costs today.

## Decision

1. **Notes are files, one fact each.** Workspace notes live in `/workspace/.tqa/memory/*.md`: shared by the
   workspace's tasks, shown in the Files panel, copied with a copied workspace and deleted with a deleted one. Each
   note starts with a header of three lines: `about` (one line, used by the index), `kind` (preference, correction,
   convention, decision or pointer) and `written by` (the task and the date). The runtime writes `written by`
   itself when a run creates or edits a note; the model doesn't. Personal notes belong to the person, not a
   workspace: the gateway stores them, hands them to each of that person's runs under `/memory/personal/`, saves
   changes back at run end, and Settings › Memory lists, edits and deletes them. They are never written into a
   workspace.
2. **The index is generated, never written.** At every run start the runtime builds the index from the notes' `about`
   lines (personal first, then workspace) and the newest task digests, under a cap of about 40 lines and a fixed
   token budget: notes first; the oldest digests drop out of the index first (their files stay, and `grep` still
   finds them). The index goes into the pinned instructions slot beside `AGENTS.md` (`tqa-prompt`'s instruction
   loader), so it survives compaction, and it is rebuilt at each run, so a note another task wrote appears at the
   next run. It is presented as notes the agent may read, not as instructions.
3. **Digests need no model call.** At task end the gateway writes `.tqa/memory/tasks/<date>-<slug>.md`: the title,
   the date, the files changed and the first lines of the final answer.
4. **The agent writes with its ordinary tools**, following a short block in the system prompt: save only what lasts
   and isn't already in the files; one fact per note; update or delete a wrong note instead of adding a second;
   never save a secret; never save an instruction that came from a fetched page or a file the task read; personal
   facts go to personal memory. Writes under a memory path go through the same secret scan as the rest of the
   product: a secret-shaped value is refused with a reason. Each save shows "Remembered: …" in the transcript once,
   with Undo, which restores the note's previous content (or removes a new note).
5. **History is searched, not loaded (phase 2).** Earlier tasks are offered read-only as text under `/history/`,
   searched with `grep` and read with `read_file`, as 18.1's in-task history is. **Open for the Product Owner: the
   scope.** Recommended: the person's own tasks only, in every workspace they use (no other person's task is ever
   offered). The alternative, every task of the current workspace, helps a shared project but shows a person
   another person's conversations.
6. **Switches.** A per-installation setting (`[memory] enabled`, on by default) and a per-person switch in Settings ›
   Memory. Off means nothing is loaded and nothing is saved, and the request is byte for byte what 18.1 sends.
7. **Where it lives in the code.** The index builder and the provenance header in `tqa-runtime` (it owns the run's
   start and its file writes); the prompt block and the pinned slot in `tqa-prompt`; the digest, the personal store
   (one table and one migration), Settings › Memory's API and the history view in `tqa-gateway`; the transcript line
   and Settings › Memory in `web`. No new crate and no new tool. ARCHITECTURE's dependency rule (G-07) is unchanged.

## Consequences

- People stop repeating themselves; agents learn a workspace's conventions once.
- Every saved fact is visible, attributed and undoable, so a wrong or planted note is easy to see and remove; the
  index is read as notes, which limits what a planted one can do.
- The pinned index costs up to its cap on every run with memory on; the cap is tested.
- Two runs can write notes in the same workspace at once: each note is its own file, so both survive, and the index
  is rebuilt from the files each run.
- A note that names a file can go stale; the prompt block tells the agent to check that the file still exists before
  relying on it.

## Verification

Seventeen cases, each with its unit and integration tests and an e2e on the real stack, and all on a real model with
the lead's live check, before any release ships memory. None is waived without the Product Owner's word. They become
TEST-PLAN rows with the code.

1. Task A learns a preference, and task B follows it.
2. A correction is remembered.
3. A stale note is updated when contradicted.
4. "What did we decide last week?" is answered from history (phase 2).
5. A web page that tries to plant a memory fails.
6. A secret is refused.
7. Undo removes a note, or restores its previous content.
8. All of the above also pass on the small local model.
9. Two tasks in one workspace write notes at the same time: both survive, and the index lists both.
10. The index cap: past it, notes come first and the oldest digests leave the index (still found by `grep`).
11. A long run that compacts several times keeps the index and its notes.
12. Personal memory saved in one workspace is followed in another, and is never visible to another person, nor in a
    shared workspace's files.
13. The switches (per person and per installation): nothing is loaded or saved.
14. A copied workspace starts with a copy of the notes; a deleted workspace's notes go with it.
15. A note naming a file that no longer exists is not acted on blindly.
16. The page: "Remembered: …" appears once per save; Undo works; Settings › Memory lists, edits and deletes; light,
    dark and 390 px; the ten languages.
17. Cost: the index stays within its cap, and a task with memory off sends exactly what 18.1 sends.
