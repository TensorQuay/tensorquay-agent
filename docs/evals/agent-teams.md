# Agent teams: E2E-AGENTS-020 and the v1.1 evaluation (plan)

**State:** the plan (2 Oct 2026) was run on three nights (11 to 13 Oct, builds 16b, 16c and 16e) and judged blind
(14 Oct). Section 3 is the report (21 Oct 2026), and its result is **indicative**. The plan covers DEV-PLAN Phase
10's slice 7: the end-to-end test E2E-AGENTS-020 on the fake model, then the evaluation that ADR-0034 took as proposal
J, about 20 representative team tasks with judged results, a single agent against an agent team at the same budget.
The technical lead accepted the plan and its bars before anything was run.

## 1. E2E-AGENTS-020: an agent team end to end on the fake model

**What it proves:** every slice of ADR-0034 works together through the product's own surfaces, with the model
scripted so the run is the same every time. It proves nothing about how well a real model leads; that is section 2.

**Stack:** the compose stack with `[agent_teams] enabled = true`, the fake model and the Workstation image, driven by
the web suite as the other stack suites are (one person, one browser). Defaults otherwise: `max_members` 3, the
budget and deadline at their defaults, and the liveness thresholds at theirs.

**The script, step by step** (the fake model's answers, keyed by the run's task and turn):
1. The person asks a fresh task: "Compare Q3 revenue by region in `data/q3.csv` with the targets in `targets.md`,
   and fix the rounding bug in `report.py`." The seeded workspace holds both files and a failing `test_report.py`.
2. The lead calls `start_member` twice: `analyst` with `own_copy` and the brief for the comparison, and `fixer` with
   `my_files` and the brief for the bug.
   - Check: the task list groups both helpers under the lead.
   - Check: each helper's task opens with the lead's brief, shown as the lead's words, not the person's.
3. The lead calls `ask_all` ("Anything blocking?"). `analyst` answers with `send_to_lead` (`answer`); `fixer` asks
   back (`ask`, "Round half up or half even?"). The lead calls `ask_person`.
   - Check: the lead task shows Waiting for you.
   - The person answers on the team panel ("half up"), which wakes the lead.
4. The lead calls `decide` ("Round half up.") referring to `fixer`'s question.
   - Check: both helpers read the decision; the person's answer reached the lead as a person's message, not an
     agent's.
5. `fixer` edits `report.py` on the lead's files and reports with `files: ["report.py"]`. While `fixer` works, the
   lead's `read_reports` waits answer `yield_files`, and the lead ends its turn.
   - Check: the two never work on the files at once; the lead is woken by `fixer`'s report.
6. `analyst` writes `comparison.md` in its own copy and reports it with `files: ["comparison.md"]`. The lead calls
   `take_files` on that report.
   - Check: `uploads/analyst-comparison.md` is in the lead's workspace with the same bytes.
7. The lead calls `read_helper` on `analyst`, then `done` with a summary.
   - Check: the team ends; the end note lists both helpers' reports and the file handed back, and the summary.
   - Check: both helpers' tasks are ordinary tasks again, with no team tools, and the lead's next run has none
     either once it has read the note.

**Checks across the run:**
- The tests in the lead's workspace pass after step 5.
- No message was delivered twice or lost: each reader's seqs are increasing with no gap among what was addressed to
  it.
- Every helper message reached the lead marked as untrusted.
- No run waited on a held slot past its bound.
- Restart case (a second run of the same script): the gateway is restarted at a named step, once the fixer's report is
  in the team's log, through a marker handshake between the spec and the driver (no timer). Delivery resumes with
  nothing lost or doubled, and the stop at step 7 still writes one end note.

**As built (3 Oct 2026), two deviations from the steps above:**
- The script takes the analyst's files (step 6) before the fixer works (step 5), so the restart point is after the
  fixer's report rather than between steps 5 and 6. A scripted lead can't branch, so its runs are ordered by what wakes
  them: one wake for both helpers' first messages, the person's answer, then the fixer's report.
- "The tests in the lead's workspace pass" is checked as the fixed line in `report.py`: the default environment the
  stack runs has no Python to run `test_report.py` with.
- The scripted lead names the analyst's report and the fixer's question through `seq_of` (TK-U-042), filled from its
  own reads, so the script doesn't depend on which helper answered first.
- The person answers on the team's route, not on the panel, and the views are checked once the team has ended (4 Oct
  2026): the scripted team moves on in a second or two, so a person's answer typed on the page would race the fixer's
  ask, which wakes the lead first. "Waiting for you" on the lead's task is WEB-E2E-024's; here the answered question's
  card, both helpers under the lead, each helper's brief and the end note (each helper's report, and the files as their reports named them) are checked, and the lead's run must have
  completed after `done` (ADR-0034 point 8).

**Not in it:** the budget and deadline stops, the idle stop, `stop_member` and the person's Stop are each covered by
their DB tests (GW-I-107, GW-I-108, GW-I-111) and by the web's own tests; E2E-AGENTS-020 keeps one happy path plus
the restart.

**On c15 (8 Oct 2026):** the agents stack suite (`deploy/tests/demo.py --suite agents`, on C15's
images, step 2b, 05:29 to 05:31) passed 5 of 6.
- **E2E-AGENTS-020** passed, both its plain run and its run with a gateway restart.
- **The owner's edge cases:** E2E-AGENTS-022 (Stop team), 023 (a restart while helpers work), 024 (deleting a lead
  with live helpers) and 025 (the helper limit) passed.
- **E2E-AGENTS-021** (a helper whose run fails) is skipped: it is marked to wait for the gateway's fix (GW-I-260),
  which comes in c16.

**On c16 (12 to 20 Oct 2026):** the suite has run in every c16 integration from 16d on. It passed 12 of 12 on 16d to
16j: E2E-AGENTS-020, E2E-AGENTS-021…030 and WEB-E2E-062. On 16y (20 Oct) it passed **17 of 17** in 6.9
minutes, with the five cases added since:
- E2E-AGENTS-031, a message to a leading lead;
- E2E-AGENTS-032, a lead that ends its turns without `done`;
- E2E-AGENTS-033, two helpers on the lead's files;
- E2E-AGENTS-034, a team's ends in words;
- E2E-AGENTS-035, a stopped helper handed back by the gateway.

The same path ran on the real model too: in the phase demo on 16j (four helpers, ended by `done`; `gate-10.md`) and
in the real-world team cases on 16k.

## 2. The evaluation (proposal J): a single agent against an agent team

**The question:** on work that splits, does an agent team do better than one agent given the same budget, and what
does it cost in time and failures? The answer informs whether agent teams stay behind the install switch, off by
default, for v1.1, and what the guides tell administrators.

**Design: paired, same budget, judged blind.**
- **Both sides get the same task statement, workspace, model route, budget and wall clock:**
  - the single agent is a plain task;
  - the team is the same task with agent teams on. Its lead is told nothing extra: whether to start helpers is the
    model's call, as it is for a person.
- **The same budget:**
  - the single agent's run budget is the task's `max_tokens`;
  - the team's `agent_teams.token_budget` is the same number for the lead and its helpers together;
  - each side gets the task's `wall_clock_secs` plus 60 s, and the team's deadline is set to the same.
- **The model:** the local candidate route used for Gate 5, the same server, build and flags on both sides, one
  attempt at a time so neither side gets more of the server. A second pass on one hosted route is
  optional and named in the report if run.
- **Attempts:** 3 per task per side, 60 per side, in the same order (each task's attempts together, single then team).
- **Reruns:** only when the model server or the connection failed (as E2E-CMP-003 does), never for the agent's own
  failure; every rerun is listed.

**The task set: 20 tasks in four kinds, chosen to split.** Each has a seeded workspace, a statement, an automatic
check where one is possible, and a list of key facts the judge looks for. Sources are local files in the workspace
(no internet), so both sides read the same material.

| # | Kind | Task | Why it splits | Automatic check |
|---|---|---|---|---|
| R1 | research | From 40 incident write-ups in `incidents/`, the five most common root causes, each with three cited incidents | sources divide by folder | key facts present; each citation names a real file |
| R2 | research | From three vendors' API docs in `vendors/`, a comparison table on auth, rate limits, pagination and pricing | one vendor per helper | the table's 12 cells match the key facts |
| R3 | research | From a year of release notes in `changelog/`, every breaking change with its version and migration step | by quarter | breaking changes found / 17 |
| R4 | research | From 60 customer comments in `feedback.csv`, the main themes with counts and two quotes each | by segment | counts within ±2 of the key |
| R5 | research | From four policy documents in `policy/`, every place they contradict each other | pairs of documents | contradictions found / 6 |
| R6 | research | From a mid-sized repository, an architecture overview: components, data flow, and where to add a feature | by component | components named / 8 |
| P1 | report | Q3 sales by region from `sales/` (four regional CSVs) against `targets.md`, as a one-page report with a chart | one region per helper | totals per region exact |
| P2 | report | From 12 sets of meeting notes in `meetings/`, the decisions, owners and open questions | by month | decisions found / 15 |
| P3 | report | A risk register from `project/` (plan, budget sheet, status mails): risk, likelihood, impact, owner | by source type | risks found / 10 |
| P4 | report | A literature-style summary of eight papers in `papers/` (text files) on one question, with agreements and gaps | two papers per helper | each paper cited; key claims / 16 |
| P5 | report | An onboarding guide for the repository in `repo/` from its code, tests and existing docs | code, tests and docs | sections present; commands in it run |
| C1 | code | Add tests to three untested modules of `lib/` to 80 % line coverage each | one module per helper | coverage ≥ 80 % per module; tests pass |
| C2 | code | Migrate the API client in `client/` and its four callers to the v2 interface (`py-long-api-migration`, extended) | callers divide | the task's check |
| C3 | code | Fix the four failing tests in `suite/`, each in a different module | one bug per helper | all tests pass, none changed |
| C4 | code | Add CSV and JSON export to the CLI in `tool/`, with tests and docs (`py-feature-csv-export`, extended) | format and docs | the task's check plus the JSON cases |
| C5 | code | Triage the logs in `logs/` and fix the two bugs they point to (`py-long-log-triage`, extended) | triage, then two fixes | the task's check |
| C6 | code | Rename the `Record` type across the package and its docs without breaking the public API (`py-rename-refactor`, extended) | by package | tests pass; public names unchanged |
| M1 | mixed | Find which of the three CSV parsers in `parsers/` is fastest on `data/` and switch the tool to it, with a benchmark | measure, then change | the benchmark script runs; the tool uses the fastest |
| M2 | mixed | Read the bug reports in `issues/`, reproduce each with a failing test, then fix the two that are real | reproduce, then fix | the two real bugs fixed; the third left with its test marked expected to fail |
| M3 | mixed | Document the service in `svc/` (API reference from code) and add the missing input validation it reveals | docs and code | reference covers every route; validation tests pass |

The code tasks reuse or extend the repository's e2e tasks in `tests/e2e/tasks`, so their checks already exist. The
research and report tasks are new, and their workspaces and key-fact lists are written before any attempt is run, by
someone other than the judges.

**Judging: a fixed rubric, blind to the side.** Each attempt's final answer and changed files are exported without
anything that names its side (no team tools in the transcript excerpt, helpers' outputs only as they appear in the
lead's answer). Two judges score each attempt independently. A packet goes to a third judge when any criterion is
scored 0 by one judge and 2 by the other, or when the two judges' totals (0 to 10) differ by more than two points, so
small differences that add up are settled too (decided 5 Oct 2026); the third judge's whole score then settles it.
Otherwise the attempt's judged score is the mean of the two totals.
- **Redaction is symmetric** (decided 5 Oct 2026): in both sides' answers, every helper name and role any of the
  task's team attempts had, and the side words ("helper", "agent team", "team member", "sub-agent"), become "a
  colleague" (or "colleagues"), with no visible mark. Those words still mostly come from team answers, so the
  residual tell is named in the report's limits.
- **Blindness is measured, not assumed:** each sheet has a `side_guess` column ("which side do you think this
  was?": single, team, or blank when unsure). The report gives each judge's share of right guesses with its interval;
  chance is 50 %.

| Criterion | 0 | 1 | 2 |
|---|---|---|---|
| Correct | wrong or unchecked claims, failing check | mostly right, one material error | right, the check passes |
| Complete | under half of the key facts or parts | most of them | all of them |
| Grounded | claims without sources | some cited | every claim traceable to a workspace file |
| Coherent | contradictory or duplicated parts | uneven, but usable | one consistent answer |
| Usable as is | needs rework | needs edits | ready for the person |

An attempt's judged score is the sum (0 to 10). The automatic check is reported beside it, not folded in.

**What is measured, per side:**
- the pass rate on the automatic checks, and the mean judged score, with paired differences per task and 95 %
  intervals (paired bootstrap over tasks, as in E2E-CMP-003);
- tokens spent, wall time, and for the team: helpers started per task, messages, waits that yielded, wakes, and how
  each team ended;
- failures by class:
  - both sides: model error, tool arguments (`model.tool_args`), stuck (`model.stuck`), unreadable replies
    (`model.unreadable`), budget, deadline;
  - teams only: helper idle or stuck notices, a helper that never reported, a report not taken, the lead finishing
    with a helper's work unread, and the end note missing;
- whether the lead started helpers at all. A lead that worked alone is still a team attempt, and the share of them is
  reported.

**Long tasks and real collaboration (proposal of 7 Oct 2026, for the owner's acceptance bar):** the 20 tasks above
are short and split by partition.
- **Short:** 15–30 min of wall clock, at most 40 steps and 300,000 tokens.
- **Split by partition:** by folder, vendor, region or module. Each helper's part is independent of the others, and
  the lead's merge is a concatenation.
- **So they measure fan-out, not collaboration or long horizons.** The owner's bar asks for both:
  - long work, multi-hour and over many files;
  - real collaboration: work that must be split, handed back and merged, with helpers depending on each other's output.
- **Seven tasks are added in two kinds,** with the same design: paired, same budget, judged blind.
- **Each one's automatic check is an integration check:** it passes only if the parts fit together, never a count of
  parts found.

| # | Kind | Task | Why it needs collaboration or time | Automatic check |
|---|---|---|---|---|
| D1 | dependent | A producer (`writer/`, writes a binary event log) and a consumer (`report/`, reads it and summarises) exist only as stubs and a one-page format spec with two deliberate gaps; build both | the format's gaps must be settled once and both sides must follow the settlement | a hidden round-trip test: the consumer reads 500 events the producer wrote, exactly |
| D2 | dependent | A four-stage pipeline: parse three log formats in `logs/`, normalise to one schema, aggregate per service, write the weekly report | each stage's output is the next one's input; a wrong schema upstream breaks every later number | the report's 24 figures exact; the stages' own tests pass |
| D3 | dependent | Change `Money` from float to integer cents across a 13-module package whose modules import each other and share a helper, keeping the public API | the edits overlap in shared modules: parallel helpers collide unless the lead orders or merges them | the workspace's tests and held-out exact cases pass; the public API is unchanged; no float arithmetic on money in the package (a scan) |
| D4 | dependent | A regression in a 60-commit history: find the commit that introduced it, write a failing test, fix it | bisecting, reproducing and fixing are separable, but the fix depends on the bisect and the test on the reproduction | the test fails on the culprit's parent's tree and passes after the fix; the named commit is the seeded one |
| D5 | dependent | Implement a rate limiter from a spec, then review it against the spec and fix what the review finds (three seeded traps in the spec's edge cases) | an implementer and a reviewer, whose findings go back to the implementer | hidden edge-case tests (the three traps) pass |
| L1 | long | Port a 45-module service from direct SQLite calls to the repository interface in `store/`, with its 25 call sites and tests (2.5 h, 300 steps, 3 M tokens) | many files over hours; the interface must be fixed before the call sites move | the full suite (180 tests) passes; no `sqlite3` import outside `store/` |
| L3 | long | Audit a 300-file repository: find the 12 seeded defects (security and correctness), report each with its location, and fix six with tests (2 h, 250 steps, 2.5 M tokens) | wide reading over hours, then fixes that must not collide | defects found and located / 12 (passes at 10); the six fixes' tests pass |

- **New measurements on these tasks, team side:**
  - hand-backs that conflicted or overwrote a helper's file;
  - waits on another helper's output;
  - work redone because a dependency changed;
  - whether the lead fixed an interface before fanning out.
- **On every task, both sides:** the time to the first passing check.
- **The rubric is unchanged.** For D tasks, Coherent means one design throughout: one format, one schema, one error
  convention.

**Run plan for Fri 9 and Sat 10 Oct (the lead's decisions, 7 Oct):**
- **Two nights** on the shared GPU's night slot.
- **Not enough room for the whole design:** that's 27 tasks × 2 sides × 3 attempts, about 60 h of serial model time.
  So the two nights favour the new kinds, which answer the owner's question, and the short set's three attempts move
  to later nights.
- **Always one attempt at a time,** as the design says. Single and team sharing one server at once would confound the
  time and the load that are being measured (the lead, 7 Oct).
- **Fri night (about 10 h):**
  - D1–D5 × 2 sides × 1 attempt, in the design's order: 10 attempts at about 35 min, 6 h;
  - then eight short tasks × 2 sides × 1 attempt: R1, R2, P1, P4, C2, C3, M2 and M3 (one or two per kind, the most
    split), 16 attempts at about 15 min, 4 h.
- **Sat night (a 12 h window):** L1 and L3 × 2 sides × 1 attempt, sequentially, about 9 h. Then D1–D5's second
  attempts while time remains.
- **Batches:** each night runs in batches, each with its own `--out` so each resumes on its own: `fri-d` and `fri-s`
  on Friday, `sat-l` and `sat-d` on Saturday (the lead, 8 Oct). The packets are made once, after the last batch, and
  the report reads all four, combining them by task.
- **L2 is dropped** (the lead, 7 Oct).
- **What it can show:** with one or two attempts per task, the write-up labels the result **indicative**, a direction
  with its intervals stated as wide. The three-attempt runs come on later nights, and a claim about the short set waits
  for them.
- **Stops and resumes:** the runner resumes a stopped night (`--resume`, tested on the scripted model on 7 Oct). Each
  night's last attempt must finish before the window ends, or it's set aside and runs first the next night.

**Readiness before Fri 22:00:**
- the seven new tasks' fixtures, seeds, hidden checks, key facts and scripted dry-run sessions, through `generate.py`
  (built Thu–Fri);
- `stack_dry_run.py` passing on the scripted model: both arms on every task, the new ones included, plus the resume
  test (stopped mid-attempt, then `--resume`);
- the long tasks' budgets set in their `task.toml`;
- the judges named (two plus a third), with the packets' redaction checked on a team answer that names its helpers.

**The bars** (proposed here and accepted by the technical lead as proposed, before any run):
- Agent teams ship in v1.1 behind the install switch, off by default, whatever the result. The result decides only the
  guides' advice and the next step.
- **Recommend for split work:** the team's mean judged score is higher, and the interval excludes no gain on research
  and report tasks. It also costs no more than 1.5× the wall time and has no team-only failure class in more than 10 %
  of its attempts.
- **Keep experimental:** otherwise, and the report names the failure classes to work on.

**Reporting:** this page becomes the report (section 3): the method as run, the per-task table (both sides, three attempts each),
the intervals, the failure classes with examples, and the limits (one model route, 20 tasks, local sources only).

**The harness** (`tests/e2e/teams_eval/`, see its README):
- **Tasks:** the 20 tasks live in `tests/e2e/teams_eval/tasks/`, not in `tests/e2e/tasks/`, because every directory
  there joins every tqa-e2e run, the gates' included. They are tqa-e2e task directories, so `tqa-e2e --check-only`
  checks an attempt as it checks its own. `generate.py` writes them from their ground truth.
- **Code tasks:** C4 and C5 extend `py-feature-csv-export` (JSON and docs added) and `py-long-log-triage` (a second
  crash). C2 and C6 are new tasks shaped like `py-long-api-migration` and `py-rename-refactor`, since the table's
  versions differ from those (four callers; a type, not a function).
- **Where a check counts found items, it passes at:** R3 15 of 17 breaking changes and no more than 2 deprecations
  listed; R5 5 of 6; R6 7 of 8 components; P2 13 of 15 decisions with their owners and 4 of 5 open questions; P3 9 of
  10 risks with 8 owners right; P4 every paper cited and 14 of 16 claims. The counts are in each check's output, and
  the judges see the rest.
- **Arms:** sides switch by restarting the gateway with `[agent_teams]` off or on, twice per task, keeping the
  per-task interleaving against drift in the model server. Each restart's time and length is logged and reported.

## 3. Results (nights of 11, 12 and 13 Oct 2026; judged 14 Oct)

**State:** the report (21 Oct 2026). It comes from the six batches' records (`attempts.jsonl` and each attempt's team
record) and the settled blind judging (`judging.settle`), using the harness's own statistics (`stats.py`). With one
attempt per task and side, everything in this section is **indicative**: a direction, with wide intervals, not a
measured result.

### 3.1 In plain words

- **Does a team beat a single agent?** **Not shown.** On the same night (night 2, 7 tasks), the team's judged score
  was the same as the single agent's: **+0.00** points out of 10 (95 % interval −0.86 to +0.86). The team passed the
  automatic check 4 times out of 7, against the single agent's 5. Night 3's team, set against night 1's single agent
  task by task (13 tasks, on two builds), scored **−0.73** (−2.58 to +0.62). Most of that is one empty answer after
  a cut-off reply. Its checks passed 11 times against 10. Both intervals include zero.
- **On which kinds of task?** Too few teams formed to say. With the brief line, a lead started helpers in **4 of 20**
  team attempts, all on dependent work: twice on D1 and twice on D2. Three of those four passed their checks with a
  judged 10. No lead started a helper on the short tasks or the long ones.
- **At what cost?** Task for task, a team attempt used **0.96×** the single agent's tokens and **0.95×** its wall time
  on night 2 (0.92× and 0.94× on night 3 against night 1). That's because most team attempts worked alone. The one
  two-helper team (night 2's D2) passed as the single agent did, with **3.9×** its tokens and **1.8×** its time.
- **How do teams fail?** One team-only failure in 20 attempts: a helper that never reported. On night 2's D1 the lead
  stopped its only helper 3.7 minutes into the helper's first step, when `read_helper` couldn't say where the helper
  was (fixed in c16: TOOL-U-058, GW-I-277). The lead worked on alone and was stopped at the wall clock. The commonest
  failure on both sides was a cut-off reply (`reply_cut`), which ended 4 attempts.
- **What this means for v1.1:** against the bars in section 2, **keep experimental**. "Recommend for split work"
  needs a gain shown on research and report tasks, and no lead formed a team there. That reading is also the
  technical lead's recommendation; the Product Owner decides. Agent teams stay behind the
  install switch, off by default, either way. **The next step:**
  - the lead's guidance: independent parts get `own_copy` (the phase demo's helpers took turns on `my_files`), and the
    Product Owner's call on whether a lead should start helpers unasked for a large, separable goal;
  - then measure again, with three attempts per task and both sides on the same night.

### 3.2 What was run

| | Night 1 (Sun 11 Oct) | Night 2 (Mon 12 Oct) | Night 3 (Tue 13 Oct) |
|---|---|---|---|
| Build | 16b | 16c | 16e |
| Tasks | D1–D5; R1, R2, P1, P4, C2, C3, M2, M3 | L1, L3; D1–D5's second attempts | Night 1's 13 tasks, team side only |
| Batches (each its own `--out`) | `fri-d`, `fri-s` | `sat-l`, `sat-d` | `fri-d`, `fri-s` (team) |
| Attempts per task and side | 1 | 1 | 1 |
| Attempts run | 26 (13 a side) | 14 (7 a side) | 13 |
| Brief line on the team side | no | yes | yes |
| Team attempts that started a helper | 0 of 13 | 2 of 7 | 2 of 13 |
| Reruns (model server or connection only) | 0 | 0 | 0 |

- **Model:** the local route used for Gate 5, the same server and flags on both sides, one attempt at a time.
- **Judges:** two independent judges scored all 40 judged attempts on the rubric. One was a maintainer who hadn't run
  the nights; the other was a reviewer in a fresh context, who saw only the packets, their README and the
  rubric. The technical lead was the third judge for the one split (10 against 6) and gave a whole score of 6, having
  run the answer's code. Three packets differed by exactly 2 and settled at the mean. None is waiting.
- **Deviations from section 2:**
  - **The brief line.** On night 1 the team side never started a helper, so the night was a single agent against a
    single agent. From night 2 the team side's task carried one line, approved by the technical lead, in a person's
    words and naming no tool: "You can lead a team on this: start helpers, each taking one part of the work from your
    brief and reporting back to you. Use them where splitting the work helps." So the team's lead is no longer "told
    nothing extra".
  - **Night 1's team side isn't judged.** It started no helper, so it isn't a team attempt.
  - **Night 3 ran the team side only.** It is paired with night 1's single agent, task by task, across two builds.
  - **One attempt per task and side,** not three, and 15 of the 27 tasks designed.
  - **The long tasks were short.** L1 and L3 were budgeted for 2 to 2.5 hours. Each attempt ended within 8 minutes:
    L3 passed in about 7 minutes on both sides, and L1 ended in a cut-off reply on both sides.

### 3.3 Pass rates and judged quality

A side's pass rate has a 95 % Wilson interval over its attempts. The judged score is 0 to 10, the mean of two blind
judges, or the third judge's where they split. Short is the eight short tasks, dependent is D1–D5 and long is L1 and
L3.

| Night and side | Tasks | Automatic check passed (95 %) | Mean judged score |
|---|---|---|---|
| 1, single agent | all 13 | 10/13 (50–92 %) | 9.54 |
| | dependent 5 | 3/5 | 9.60 |
| | short 8 | 7/8 | 9.50 |
| 1, team side (no helper; not judged) | all 13 | 10/13 | — |
| 2, single agent | all 7 | 5/7 (36–92 %) | 8.14 |
| | dependent 5 | 4/5 | 9.40 |
| | long 2 | 1/2 | 5.00 |
| 2, agent team | all 7 | 4/7 (25–84 %) | 8.14 |
| | dependent 5 | 3/5 | 9.40 |
| | long 2 | 1/2 | 5.00 |
| 3, agent team | all 13 | 11/13 (58–96 %) | 8.81 |
| | dependent 5 | 3/5 | 7.20 |
| | short 8 | 8/8 | 9.81 |

**Paired, task by task** (team minus single; 95 % paired bootstrap over tasks):

| Pairing | Tasks | Judged score | Check passes |
|---|---|---|---|
| Night 2, the same night | 7 | **+0.00** (−0.86 to +0.86) | team −1 (−0.43 to 0.00 per task) |
| Night 3's team against night 1's single agent | 13 | **−0.73** (−2.58 to +0.62) | team +1 (−0.23 to +0.38 per task) |

- **Night 2's differences:** D1 (single 10, team 8) and D3 (single 7, team 9).
- **Night 3 against night 1:**
  - D5's team attempt scored 0, an empty answer after a cut-off reply;
  - D3 went from 9.5 to 6: the team's answer charged a shipping fee 100 times too high, checked by running it;
  - D1 (8.5 to 10), M3 (10 to 9), P1 (9 to 9.5) and R1 (7 to 10) moved less.
- **The three zeros** are all empty answers with no files, after the run ended `run.failed` with code `reply_cut`:
  night 2's L1 on both sides and night 3's D5 on the team side. They are run failures, not judged quality.

**Blindness:**

| Judge | Guessed | Right | Share (95 % Wilson) | Unsure |
|---|---|---|---|---|
| 1 | 14 | 4 | 29 % (12 % to 55 %) | 26 |
| 2 | 30 | 27 | **90 % (74 % to 97 %)** | 10 |

Judge 2's interval lies wholly above 50 %, so the packets told that judge the side, and its scores carry that
caveat. Judge 1's guesses were at chance or below.

The likely tells:
- the answers' own words about splitting the work or not;
- "a colleague", which shows in 16 of 20 team packets against 9 of 20 single ones;
- the redaction also turning code uses of "helper" into "a colleague".

### 3.4 Cost: tokens and wall time

Per attempt, the median on each side. The ratio is the team's over the single agent's, as the median of the per-task
ratios. Both sides had the same budget and wall clock (section 2).

| Night and side | Tokens (median) | Minutes (median) | Minutes of the attempts that passed (median) |
|---|---|---|---|
| 1, single agent | 63,825 | 1.6 | 1.4 |
| 2, single agent | 243,925 | 5.8 | 5.3 |
| 2, agent team | 207,020 | 6.8 | 6.9 |
| 3, agent team | 41,628 | 1.6 | 1.2 |

| Pairing | Tasks | Tokens, team over single | Wall time, team over single |
|---|---|---|---|
| Night 2 | 7 | 0.96× | 0.95× |
| Night 3 against night 1 | 13 | 0.92× | 0.94× |

- **Stopped by the budget:** none on either side. **Past the wall clock:** one, night 2's D1 team attempt (below).
- **The short tasks** took about a minute each (median 1.0 minute on every side). The dependent tasks took 1 to 13
  minutes (one team attempt ran on to its wall clock, 36 minutes), against the plan's estimate of about 35.

### 3.5 How the teams worked

Four team attempts started helpers, all on their own copies (`own_copy`):

| Attempt | Helpers | What happened | Check, score | Against the single agent |
|---|---|---|---|---|
| Night 2, D1 | 1 (tests) | The lead stopped the helper during its first step, with no report. The deadline notice came, and the harness stopped the team at the wall clock, with no report and no file handed back | no check result, 8.0 | single passed, 10.0; 568k tokens against 385k, 36.2 min against 5.3 |
| Night 2, D2 | 2 (stages 1–2, stages 3–4) | Both helpers reported with their files and the lead took them; the 80 % notice came; the lead called `done`, and four files were handed back | passed, 10.0 | both passed, 10.0; 924k tokens against 236k, 7.6 min against 4.3 |
| Night 3, D1 | 1 (round-trip tests) | The helper wrote the test file and reported; the lead called `done` | passed, 10.0 | night 1's single failed, 8.5; 551k against 699k, 7.0 min against 9.8 |
| Night 3, D2 | 1 (an independent check of the figures) | The helper computed the expected figures and reported; the lead called `done` | passed, 10.0 | both passed, 10.0; 406k against 417k, 6.5 min against 6.6 |

| | Short | Dependent | Long |
|---|---|---|---|
| Helpers started per team attempt (mean; nights 2 and 3) | 0.0 | 0.5 | 0.0 |
| Leads that worked alone | 8 of 8 | 6 of 10 | 2 of 2 |
| Team-only failures (attempts) | none | a helper that never reported: 1 | none |
| Hand-backs that conflicted or overwrote a file | | 0 | 0 |

- **Read from the team records:** every helper worked on its own copy, and a file taken back gets a free name
  (E2E-AGENTS-028), so no hand-back could overwrite another member's file.
- **Not measured,** because too few teams formed: waits on another helper's output, work redone because a dependency
  changed, and whether the lead fixed an interface before fanning out.

**Failures on both sides, by class** (attempts):
- single agent: a cut-off reply 1;
- team side: a cut-off reply 3 (one on night 1, with no helper), past the wall clock 1;
- no model errors, tool-argument failures, stuck runs or budget stops on either side;
- the harness's own problems: none.

### 3.6 Per task

Each cell is the automatic check (✓ passed, ✗ failed, – no result), then the judged score and the minutes. A team
cell adds the helpers started. Night 1's team side isn't shown: it started no helper, and its checks passed 10 of 13.

| Task | Kind | Night 1, single | Night 2, single | Night 2, team | Night 3, team |
|---|---|---|---|---|---|
| D1 | dependent | ✗ 8.5, 9.8 | ✓ 10, 5.3 | – 8.0, 36.2, 1 helper | ✓ 10, 7.0, 1 helper |
| D2 | dependent | ✓ 10, 6.6 | ✓ 10, 4.3 | ✓ 10, 7.6, 2 helpers | ✓ 10, 6.5, 1 helper |
| D3 | dependent | ✓ 9.5, 9.0 | ✗ 7.0, 7.2 | ✗ 9.0, 6.6 | ✗ 6.0, 5.3 |
| D4 | dependent | ✗ 10, 1.3 | ✓ 10, 1.9 | ✓ 10, 2.2 | ✓ 10, 1.2 |
| D5 | dependent | ✓ 10, 9.5 | ✓ 10, 12.6 | ✓ 10, 6.8 | – 0, 6.4 (cut off) |
| L1 | long | | – 0, 5.8 (cut off) | – 0, 4.5 (cut off) | |
| L3 | long | | ✓ 10, 7.4 | ✓ 10, 7.0 | |
| R1 | research | ✗ 7.0, 1.6 | | | ✓ 10, 2.1 |
| R2 | research | ✓ 10, 0.6 | | | ✓ 10, 0.6 |
| P1 | report | ✓ 9.0, 1.7 | | | ✓ 9.5, 1.6 |
| P4 | report | ✓ 10, 0.7 | | | ✓ 10, 1.0 |
| C2 | code | ✓ 10, 0.9 | | | ✓ 10, 0.8 |
| C3 | code | ✓ 10, 0.9 | | | ✓ 10, 0.7 |
| M2 | mixed | ✓ 10, 1.0 | | | ✓ 10, 1.0 |
| M3 | mixed | ✓ 10, 2.9 | | | ✓ 9.0, 3.5 |

- **D3's checks** fail on the held-out rule that money has no float arithmetic, even where the results are exact.
- **D4 on night 1** failed its check but was judged 10. The check is reported beside the score, not folded in
  (section 2).
- **Night 3's D5** lost about 9,000 tokens to `write_file` refusing calls that left out `create_parents`, before its
  reply was cut off. The same refusal shows in 30 attempt logs across the nights, usually as a cheap retry. It isn't a
  regression (the schema dates from September), and a default is planned for c17.

### 3.7 Limits of this result
- **One attempt per task and side,** so a single lucky or unlucky attempt moves a mean. Later three-attempt runs can
  confirm or overturn it.
- **Teams rarely formed:** helpers in 4 of 20 team attempts, all on dependent tasks. Even with the brief line, the
  result is mostly a single agent against a single agent.
- **Night 3 against night 1** pairs two builds (16b and 16e) on different nights.
- **15 tasks of the 27 designed,** on one model route, with local sources only.
- **Long work wasn't measured.** The long tasks ended in minutes, not hours, so long horizons aren't in this result.
- **The redaction's residual tell:** one judge's guesses were right 90 % of the time (3.3).
- **A cut-off reply** decided 4 attempts, the 3 judged zeros among them. That is the model's output limit, not team
  behaviour.

### 3.8 Since the nights: real-model teams on later builds
These are not evaluation attempts: one attempt each, not paired and not judged blind. They show what a team does on
the real model once it forms (`gate-10.md` has the details).
- **The phase demo (16j, 15 Oct):** four helpers and `done`, with a passing finish check in about 10 minutes. The
  lead gave every helper `my_files`, so they took turns on its workspace instead of working at once.
- **The real-world cases (16k, 17 Oct):**
  - three researchers at once on their own copies (B1);
  - two helpers totalling sales and costs to the cent (B3);
  - a helper using the Desktop (B4);
  - a two-helper trip plan built from both reports (B9c, after the same goal ran out of the default budget in
    B9b);
  - two helpers building a web page in turns on the lead's files (B10b).
- **The live checks (16o to 16u, 18 Oct):** teams steered mid-run, stopped, and handed back by the gateway after a
  budget stop, all on the real model.

The defects these runs found are fixed (`gate-10.md`). They bear on cost and reliability, not on the question above.
