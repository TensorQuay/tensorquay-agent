# Refs from an earlier run

**State:** proposed, 18 Oct 2026. **Decides:** the technical lead, on the evaluation below. **Builds on:** ADR-0039
section 3 (the computer tool), TOOL-U-066 (a `type` refusal says what to do instead), CORE-E-012 (the repeated-failure
guard).

## The problem

The computer tool names elements by refs from its latest snapshot (`e7`), and "the refs last until the next snapshot".
Each run builds its own computer tool, so its snapshots are numbered from 1 again. The conversation, though, still holds
every earlier run's snapshots, with their refs. And between runs the desktop browser was reset to `about:blank`
(ADR-0039 section 6), so none of those elements exist any more.

In a live Desktop check, the person took over and handed back, then sent "open the page again". The model's next run
observed once (snapshot 1, of the reset browser) and then typed into `e7`, the address field from the previous run's
snapshots. It got "stale_ref: e7 isn't in snapshot 1; observe again; or leave out ref to type into the focus (after
ctrl+l for an address)" and sent the same call twice more, and the repeated-failure guard ended the run. The refusal
already said what to do, and it was ignored. The reused number may not have helped: the earlier run also had a
snapshot 1, in which `e7` existed.

## Options

| | What changes | For | Against |
|---|---|---|---|
| **A. A run-start line** | When a Desktop session's history holds snapshots, the run's context says that the browser was reset since and refs from earlier runs are gone: observe first. | Words only, no tool change. | It asks for more reading of the kind the model just ignored. Every Desktop run pays for it. |
| **B. A refusal that carries a fresh snapshot** | A ref that isn't in the latest snapshot is refused as now, and the refusal includes a new snapshot: "e7 isn't in snapshot 1; here is snapshot 2: …". | The model gets valid refs in the same answer, without another round trip. It only costs anything when a ref is stale. | The refusal is longer (bounded by the snapshot's element limit). A call that only looks is still counted as a failure, so the guard stays the backstop. |
| **C. Numbers that continue** | A run's snapshot numbers carry on from the history, as the action caps already do (`Spent::from_journal`). The refusal can then say "e7 is from snapshot 4, an earlier run's; the latest is snapshot 5". | No number means two different screens, and the refusal says why the ref is gone. Small. | On its own it is still only words. |

Refs that carry their snapshot (`4e7`) would make collisions impossible. But they change the grammar every model has
learned from the tool's description, and the marks with it, so they aren't considered here.

## Recommendation: B with C, if the evaluation agrees

B answers the failure directly: a model that repeats a ref after being told to observe gets the observation, and its
next call has refs that work. C is cheap, removes the reused numbers, and gives B's refusal an honest reason. A goes
into the evaluation as the words-only comparison. If it does as well as B, it is the smaller change and wins.

## How we'd evaluate it

Offline, like the grounding evaluation (`docs/evals/computer-grounding.md`): the model is given a prepared transcript
and its next call is scored, with no desktop running. Each scenario is an earlier run on the fixture site, with
snapshots and refs, and its end. Then comes a new message asking to go on, and the new run's first `observe` of a
blank page. 40 seeded scenarios vary the earlier run's length, the ref the model used last, and whether a Take over
came between. Each arm is run 3 times per scenario on every local route that has the Desktop.

| Arm | The transcript as the arm would make it |
|---|---|
| Baseline | As today. |
| A | The run-start line before the new run's first message. |
| B | After one stale ref, the refusal with the fresh snapshot. |
| B + C | As B, with numbers carried on and the refusal naming the earlier run's snapshot. |

**Scored:**
- At the new run's start (baseline, A, C): whether the first acting call uses a ref from an earlier run.
- After one refusal (baseline, B, B + C): whether the next call is valid: a ref in the latest snapshot, `observe`, or
  `type` with no ref after ctrl+l. Repeating a stale ref, or naming another one, is not valid.

**The bar:** the chosen arm makes a valid next call after a refusal in at least 95 % of samples, with no route worse
than baseline. Then a live confirmation on the Desktop: the same take-over-and-continue task 10 times, with no run
ended by the guard over a stale ref.

## Tests (for the chosen arm)

| Level | Test |
|---|---|
| Tool unit | A ref missing from the latest snapshot is refused as `stale_ref`, with a new snapshot whose number follows. A mark from an earlier screenshot and a changed focused window keep today's refusals. The refusal's snapshot replaces the latest, so its refs work next. (B) |
| Tool unit | Snapshot numbers carry on from a history that holds snapshots, and start at 1 without one. A stale ref from an earlier run's snapshot is named as such. (C) |
| Core | The guard still ends a run on the third identical stale call with no change between (CORE-E-012): the extra snapshot doesn't reset it. |
| Prompt (A only) | The run-start line appears only for a Desktop session whose history holds snapshots. |
| Evaluation | The table above, recorded under `docs/evals/` with each route's numbers. |
