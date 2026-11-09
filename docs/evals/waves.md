# Agent waves: E2E-WAVE-001 and E2E-WAVE-002 (Phase 9)

**State:** measured on 2026-09-28 on the release candidate `a14afc0`, and again on 2026-10-02 on candidate 13h
(`5b79a932`), and on 2026-10-07 on release candidate c14b-rc (built from `16589e55`): both waves **pass** each time
(13h: 58/60 and 59/60; c14b-rc: 60/60 and 59/60, no harness failure; c14b-rc's one miss is the model's, classified
by hand below). Raw data for 13h: the wave runner's records `tests/e2e/results/2026-10-02/slot2-wave-13h` (one at a time) and
`slot2-wave8-13h` (eight at a time); for c14b-rc, `tests/e2e/results/2026-10-07/slot2-wave-c14b-rc` and
`slot2-wave8-c14b-rc`; all kept on the run host outside the repository. PRD M1 asks for at least 90 %
of the task set completed with a passing check on a local model, with no failure caused by the harness. E2E-WAVE-002
runs the same attempts eight at a time. Gate 5's measurement of E2E-WAVE-001 (60/60 on 27 Sep, gate-5.md) is the
baseline.

## Method
- **Tasks:** the 20 tasks in `tests/e2e/tasks` (Python, shell and TypeScript; bug fixes, features, refactors, tests,
  docs and three long tasks), each with its own budgets (25–60 steps, 600–1,500 s, 150k–800k tokens) and an automated
  check. A task's statement is its `task.md`.
- **Attempts:** each task three times, 60 attempts per wave. Each attempt gets its own copy of the task's seeded
  workspace. The check runs on a copy of the workspace the attempt left, with the task's protected paths restored.
- **Classes (the harness's own):**
  - `pass`: the check passed;
  - the model's failures: `model.wrong_work` (the check failed), `model.tampered` (a protected path changed),
    `model.tool_args` (our tool refused its arguments until the run stopped, reviewed by hand), `model.stuck` (the
    same call failing until the run stopped, or no progress), `model.unreadable` (replies that couldn't be read as a
    tool call) and `model.budget` (the attempt ran past its steps or tokens). Until 7 Oct 2026 the stuck and
    unreadable stops were labelled `model.budget` too; the rows below keep the label each run recorded
    and give the new one beside it;
  - harness failures: `harness.timeout` (no end within the task's wall clock plus 60 s), `harness.lost_event`,
    `harness.provider` (the model's calls failed after the runtime's retries), `harness.tool`, `harness.check` (a
    broken check) and the others the harness names.
- **Reruns:** an attempt that fails with `harness.provider` is rerun once. Completion counts its final run, but the
  failure stays counted: every harness failure counts against the wave, and a rerun never hides one. Any other harness
  failure is investigated, not rerun. With eight at a time, a harness failure stops new starts: the running attempts
  finish, and the rest are recorded as skipped.
- **E2E-WAVE-001** runs one attempt at a time (`tqa-e2e --concurrency 1`).
- **E2E-WAVE-002** runs the same 60 attempts eight at a time (`--concurrency 8`), in the same task order. The only
  changes from E2E-WAVE-001:
  - the installation admits eight sandboxes for the user (`tests/e2e/wave_profile.py`, raised before the wave and
    restored after);
  - the inference server runs eight sequences instead of four.

  An attempt that fails only in E2E-WAVE-002 is read as a concurrency defect until shown otherwise.
- **Guards:** each task's own budgets, and a run cap of 10 million tokens per wave.

## Setup
- **Candidate:** the release candidate of 28 Sep. The gateway, runtime and Workstation images are pinned by image ID:
  gateway `c580e6b4b246`, runtime `47fea18da874`, Workstation `8ccb9b88a291`.
- **Harness:** `tqa-e2e` built in release mode from `a14afc0`, with that commit's task set.
- **c14b-rc (7 Oct):** gateway `fde0b91bf88b`, runtime `347b895b203d`, Workstation `84ff9c47aa24`; the runner from a
  clean checkout of `16589e55` (its run record), the same 20 tasks × 3, the same model on route `local`; the run cap
  30 million tokens a wave; E2E-WAVE-002 inside the logged admission step (route `local` at concurrency 8).
- **Model:** a 27B open-weight model (4-bit floating-point weights, FP4) served by an open-source inference server on one
  32 GB GPU; 64k context; native tool calls on route `local` with `strict = "off"`. Four concurrent sequences for
  E2E-WAVE-001 and eight for E2E-WAVE-002.
- **Host:** one Linux workstation, with rootless Docker sandboxes.

## Results

"95 % Wilson" is the range the true completion rate most likely lies in, given the attempts made.

| Wave | Completed | 95 % Wilson | Harness failures | Model failures | Tokens (prompt / completion) | Wall time |
|---|---|---|---|---|---|---|
| E2E-WAVE-001 (one at a time) | **59/60 = 98.3 %** | 91.1–99.7 % | 0 | 1 (`model.wrong_work`, sh-build-fix) | 2,660,281 / 192,687 | 53.6 min (18:07–19:01) |
| E2E-WAVE-002 (eight at a time) | **59/60 = 98.3 %** | 91.1–99.7 % | 0 | 1 (`model.wrong_work`, sh-add-tests-strings) | 2,378,242 / 195,831 | 12.3 min (19:03–19:16) |
| 13h, 2 Oct: E2E-WAVE-001 | **58/60 = 96.7 %** | 88.6–99.1 % | 0 | 2 (`model.wrong_work` and `model.budget`, now `model.stuck`: `repeated_failure`; both sh-add-tests-strings) | 2,459,250 / 175,949 | 62.0 min (10:04–11:06) |
| 13h, 2 Oct: E2E-WAVE-002 | **59/60 = 98.3 %** | 91.1–99.7 % | 0 | 1 (`model.budget`, ts-long-http-migration: a real budget stop, our over-budget cancel, so it stays `model.budget`) | 3,170,244 / 233,973 | 19.8 min (11:31–11:51) |
| c14b-rc, 7 Oct: E2E-WAVE-001 | **60/60 = 100 %** | 94.0–100 % | 0 | 0 | 2,440,775 / 186,448 | about 48 min (attempts 48.0 min; ended 06:48) |
| c14b-rc, 7 Oct: E2E-WAVE-002 | **59/60 = 98.3 %** | 91.1–99.7 % | 0 | 1 (`model.budget`, now `model.stuck`, ts-add-tests-slug: the model's own wrong tests, classified by hand below) | 2,108,498 / 180,696 | 11.4 min (06:59:52–07:11:13) |

**c14b-rc's one miss, classified by hand (7 Oct): the model's.** `ts-add-tests-slug` attempt 45 (E2E-WAVE-002, round 3)
asks for `node:test` tests that pin down `src/slug.ts`'s documented behaviour, without changing it. The model read the
source, tried `slugify` on a few inputs, wrote the test file and ran it; its assertions were wrong three times in a
row, each time differently: a length of 40 where the output is 39, then a trailing `b` that the 40-character cut
drops, then `…ab` where a space becomes a hyphen (`…a-b`). It never changed `src/slug.ts`. The third failure of the
same command, `node --test test/*.test.ts`, ended the run `repeated_failure` (CORE-E-012), and the harness classed it
`model.budget` by its rule then for three failures of the model's own making (TK-U-004); since 7 Oct such a run is
`model.stuck`, which says what happened: no budget ran out. Not a harness failure, and no
product fault: the tests the model wrote asserted behaviour `slugify` doesn't have. The same task passed in the
wave's other two rounds (10 and 14 steps). One more refusal earlier in the run was the model's too: a `write_file`
without its required `create_parents`, which it corrected at once.

A note for the product, not this miss's class: the repeated-failure guard counts the same command failing three times
in a run even when the model changed the workspace between the runs and each failure was different, as here. That is
CORE-E-012 as written; whether a rerun after an edit should count is a design question, recorded for after c15.

Both meet M1's target (at least 90 % and no harness failure). The model's one failure is a different task in each wave.
Neither wave had a provider retry. Seven attempts of the three long tasks recorded no compaction; the harness lists
that as a finding about the harness (`harness.expectation`), not a failure, and it is the same in both waves.

**E2E-WAVE-002, the concurrency measures:**

| Measure | E2E-WAVE-001 | E2E-WAVE-002 |
|---|---|---|
| Step time (one model request and its tool calls), p50 / p99 | 3.2 s / 39.4 s (498 steps) | 4.2 s / 46.6 s (493 steps) |
| Attempt wall time, median / max | 40 s / 186 s | 52 s / 321 s |
| Attempt time in all, against the wave's elapsed time | 50.0 min in 53.6 min | 64.2 min in 12.3 min (about 5 at once) |
| Most model requests running at once (the server's log) | 1 | 8 |
| Lost or duplicated events (stream against log) | 0 | 0 |
| Waits for a sandbox slot (expected 0 in both: a control) | not recorded by this harness build | not recorded by this harness build |
| Server: preemptions (requests paused because its attention cache was full), peak cache use | 0, 21.6 % | 0, 62.6 % |

Eight at a time made the wave about 4.4 times faster, with each step about 30 % slower at the median, and no
failure that the one-at-a-time wave didn't have.

On 13h the step times were 3.2 s / 43.9 s (p50 / p99, 494 steps) one at a time and 4.2 s / 67.8 s (523 steps) eight at
a time; attempt wall time median 37 s and 52 s, longest 182 s and 744 s. The longer eight-at-a-time tail is one long
task (ts-long-http-migration) running out of its budget; the wave as a whole took 19.8 min against 12.3 min on 28 Sep.

**Per task (completed of 3):**

| Task | E2E-WAVE-001 | E2E-WAVE-002 |
|---|---|---|
| py-add-tests | 3/3 | 3/3 |
| py-docs-api-reference | 3/3 | 3/3 |
| py-docs-readme-usage | 3/3 | 3/3 |
| py-feature-csv-export | 3/3 | 3/3 |
| py-feature-retry-backoff | 3/3 | 3/3 |
| py-fix-off-by-one | 3/3 | 3/3 |
| py-long-api-migration | 3/3 | 3/3 |
| py-long-log-triage | 3/3 | 3/3 |
| py-rename-refactor | 3/3 | 3/3 |
| sh-add-tests-strings | 3/3 | 2/3 |
| sh-build-fix | 2/3 | 3/3 |
| sh-config-env-spec | 3/3 | 3/3 |
| sh-feature-backup-dry-run | 3/3 | 3/3 |
| sh-refactor-deploy | 3/3 | 3/3 |
| ts-add-tests-slug | 3/3 | 3/3 |
| ts-config-npm-test | 3/3 | 3/3 |
| ts-feature-status-filter | 3/3 | 3/3 |
| ts-fix-date-range | 3/3 | 3/3 |
| ts-long-http-migration | 3/3 | 3/3 |
| ts-refactor-extract-validation | 3/3 | 3/3 |

## Limits
- Three attempts per task. The 95 % interval on a rate near 95 % over 60 attempts is about ±6 points.
- One model and one server build. M1 names the model class. Which local model stands for that class is the Product
  Owner's decision, and the result holds for this model.
- Eight at a time measures the harness and the host under load, not a model: the model's own throughput limits how
  much faster the wave runs.
