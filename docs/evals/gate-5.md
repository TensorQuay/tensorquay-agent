# Gate 5: Local-model quality

**State:** **reached on release candidate 15b**, by the technical lead's verdict (11 Oct 2026): every local-model run passed, M6 is met, and the phase
demo was accepted by the technical lead on 10 Oct (151 steps, 2.1 M tokens, 690 s, its check passed; the report's 49
part lines and its reference line "Audit reference: KESTREL-4471"). **10 Oct 2026, release candidate 15b** (the release continues as 15c, which changes
only the web, so these runs on 15b's server and runtime stand): the local-model rows ran on 15b's images on 10 Oct.
**E2E-WAVE-001 passed (60/60)**, **E2E-FALLBACK-004 passed its row (24/30 reported, no harness failure)**, and **the
phase demo completed** with six compactions and the task statement intact (below, "The local-model runs on 15b").
E2E-WAVE-002 (60/60) and E2E-CMP-003 (ours 50/50, reference 50/50) ran in the same window. **11 Oct 2026:** 15c is on
main as `1586fd9f` (its tree `release/c15` at `bf85085f`).
Before that, **8 Oct 2026, release candidate c15**: every unit, contract and edge row,
the coverage clause, PERF-030 and PERF-040 pass in c15's integration, on c15's own server, runtime and web. **Not
reached** until the three local-model runs rerun on c15's images (not scheduled yet; the GPU is after the soak).
Earlier: refreshed for release candidate **c14b-rc** (5 to 7 Oct 2026). **Not reached.** Every unit, contract
and edge row, the coverage clause and PERF-030 pass on candidate 14b's integration (c14b-rc's gateway
and runtime images); PERF-040 passed there too, on 14b's web code, and reruns on c14b-rc's. The
local-model rows ran on c14b-rc's images on 7 Oct: **E2E-WAVE-001 passed (60/60)**; **E2E-FALLBACK-004 passed its row
(26/30 reported, no harness failure)**; **the phase demo did not (0/1)**, stopped by our `read_file`'s strictness before it was summarised, which
c15 fixes. c15 reruns all three. The two fixes 13h left open (the retry of an unreadable answer, CORE-E-040; a null-word
argument counted as absent, TOOL-U-034) are in c14b-rc and tested. The 13h and 13g refreshes and the history from 26
to 29 Sep follow.

## Scope
The TEST-PLAN §17 Gate 5 row: the context, core, model-layer, prompt and tool IDs listed there; coverage ≥ 90 % on the
pure crates; PERF-030 and PERF-040; E2E-WAVE-001 and E2E-FALLBACK-004; and the phase demo (a long task on a local
model with a visible compaction and the task statement intact).

## Release candidate c15 (8 Oct 2026)
**Source:** the integration of C15 (8 Oct 01:23 to 04:24 +08:00, logs `int-b5caac76-*`), on the
images built from that commit (`candidate15-tags.txt`), web included. A row passes only if each of its test IDs ran and
none failed.
- **Method:** each ID's Rust tests were found by the ID in a `TEST:` line or comment above the test, or in the file's
  header, and looked up in the nextest logs.
- **Steps:** unit 2,355 of 2,355 passed; the database step 2,581 of 2,581; runtime 190 of 190. None failed.
- The TEST-PLAN §17 Gate 5 row is unchanged since 13h.

| Rows | c15 | Evidence |
|---|---|---|
| CTX-U-001…003, CTX-E-004, CTX-U-005/006, CTX-U-009, CTX-U-012…015 | pass (11) | every tagged test passed |
| CORE-U-022…025, CORE-E-028 | pass (5) | the same |
| LLM-C-003…005, LLM-E-009, LLM-U-011/012, LLM-U-017, LLM-C-013, LLM-C-020/022, LLM-I-021 | pass (11) | the same |
| PROMPT-U-001…004 | pass (4) | the same |
| TOOL-U-004/005, TOOL-U-010, TOOL-E-009 | pass (4) | the same |
| CTX-U-010, CORE-U-027, LLM-U-015, TOOL-E-016 (P2) | pass | the same |
| 13h's fixes: CORE-E-040, LLM-U-050, TOOL-U-001, TOOL-U-034 | pass | the same |
| Coverage ≥ 90 % on the pure crates | **pass** | `cov-pure`: 95.61 % lines, 92.73 % regions |
| PERF-030 compaction cost | **pass** | release mode (`perf-release`): p50 1.824 ms, p99 2.217 ms, 14,268 summary and 470 step tokens; c14b-rc 1.828 and 2.066 ms |
| PERF-040 web rendering | **pass**, on c15's web source | `perf:transcript`: interactive at p50 722 ms, p99 739 ms (≤ 2 s at p50), frames p95 16.8 ms, `noisy: false`. Unlike c14b-rc, the web and the server are the same commit |
| E2E-WAVE-001 | **passed on 15b** (10 Oct): 60/60 = 100.0 %, no harness failure | c14b-rc: 60/60 |
| E2E-FALLBACK-004 | **passed its row on 15b** (10 Oct): 24/30 = 80.0 % reported, no harness failure | c14b-rc: 26/30 reported, no harness failure |
| Phase demo (`local-32k`, `compaction-50`) | **completed on 15b** (10 Oct): six compactions, the task statement intact; **accepted by the technical lead** (10 Oct, 22:50) | c14b-rc: 0/1, stopped by a `read_file` refusal, fixed in c15 (TOOL-U-051, `bc8114f1`) |

What else c15 changes for this gate:
- **The compaction trigger** counts the last call's real token count (`5fffc930`, CORE-U-084).
- **The text route** drops a closing-tag run another call format leaves, and retries an echoed correction
  (`ff5c239e`, CORE-U-085).
- **A guessed `cursor`** beside line numbers is ignored, not refused (`bc8114f1`, TOOL-U-051). This is the phase
  demo's stop on c14b-rc.

All three are tested in the steps above. Their effect on a real model is what the local-model reruns measure.

### The local-model runs on 15b (run 10 Oct 2026, 20:00 to 22:44)
**Pre-drafted 8 Oct, retargeted 9 Oct, filled 10 Oct from the night's logs and records** (each cell's source is named
beside it). c15 was respun as **15b** (c15 plus two heap fixes, the metrics upkeep
GW-U-177 and the event hub's sweep GW-U-178), so these runs are on 15b's images (gateway, runtime and Workstation from
`23112f6b`; the web page is never used). The unit, contract, coverage and PERF rows above are c15's integration
(`b5caac76`). One model server for all runs, one attempt at a time. Planned window 20:00 to 01:00 (latest start
22:00), with a fallback of 13 Oct 20:00 to 01:00.

**The order** (one held GPU, as on c14b-rc):
1. **The model server.** The routes as the stack has them: `local` (tools declared natively, concurrency 4),
   `local-text` (text calls, thinking off), `local-32k`.
2. **E2E-CMP-003** (PRD M6): the shared subset on our product, then on the reference product, with the same model.
   It's recorded in [comparison.md](comparison.md), not a Gate 5 row; it shares the window because it needs the same
   server.
3. **E2E-WAVE-001** on `local`.
4. **E2E-FALLBACK-004** on `local-text`.
5. **The phase demo** on `local-32k` (`compaction-50`).
6. **The admission step** (route `local` at concurrency 8), then **E2E-WAVE-002** (Gate 9, 8 at once), then the step
   restored. Both changes are logged.
7. The server stopped.

A step that can't finish before the window's end is skipped and reported, never run past it.

**Where each result comes from.**
- The block's run log names each run's start, its results directory (`tests/e2e/results/<date>/slot2-<run>-c15b`), its
  `summary.md`, the summary's verdict line and its exit, in the order above.
- The block's record copies each verdict, with the attempts by outcome, `provider.retry` by reason and the compactions.
- **Read the numbers, not the verdict's label.** The harness's summary labels every run "E2E-WAVE-001" and prints its
  90 % target, also for the fallback and the demo (as on 13h and c14b-rc), unless c15's harness has changed that.
  Only E2E-WAVE-001 has a rate target.
- Each miss is classified by hand, as on c14b-rc: the model's own work or reply, or a harness failure, from the
  attempt's outcome and events.

| Row | What "reached" needs (TEST-PLAN) | 15b result (10 Oct) | From |
|---|---|---|---|
| E2E-WAVE-001 | ≥ 90 % complete, and no `harness` failure (PRD M1) | **Passed:** 60/60 = 100.0 % (95 % Wilson 94.0 to 100.0 %); harness failures 0; `provider.retry`: none. 21:16 to 22:08 | the run log's `wave` run and its `summary.md`; the record's attempts line |
| E2E-FALLBACK-004 | the success rate reported per model, and no `harness` failure; no rate is set | **Passed its row:** 24/30 = 80.0 % reported (95 % Wilson 62.7 to 90.5 %); harness failures 0. The 6 misses are the model's: 4 replies that couldn't be read as a text call (`model.unreadable`), and 2 finished runs whose task check failed (`model.wrong_work`, both on one task). `provider.retry`: 21 for an unparseable text call, 12 for native tools. 22:08 to 22:20 | the `fallback` run and its `summary.md` (ignore the label's 90 %) |
| Phase demo (`local-32k`, `compaction-50`) | a long task on a local model with a **visible compaction** and the **task statement intact** (§17: "the phase demo accepted by the technical lead") | **Completed** (1/1; 151 steps in 11.5 min; harness failures 0); **compactions 6**, the first at event 5297 (it summarised through event 5127), each made by the model; **task statement intact: yes**: every compaction's summary still states the ledger task and keeps the reference line the first message asked the final report to end with, and the report file the last message asked for has its 49 part lines and ends with that line (the task's check passed). 22:20 to 22:31. **Accepted by the technical lead** (10 Oct, 22:50) | the `demo` run, its `summary.md` and the record's compactions line; the statement checked in the summary the compaction produced, against the task's first message |
| E2E-WAVE-002 (Gate 9, recorded here) | step time, tokens and completion recorded | **60/60 = 100.0 %** (95 % Wilson 94.0 to 100.0 %); harness failures 0; the server restarted with 8 sequences, and the route `local` at concurrency 8 for it. 22:33 to 22:44 | the `wave8` run; the admission step's two log lines |
| E2E-CMP-003 (M6, in [comparison.md](comparison.md)) | completion within 5 points of the reference; method and versions recorded | **Ours 100.0 %** (50/50), **reference 100.0 %** (50/50), **difference 0 points**: within M6's 5. A fresh server for each side. 20:04 to 21:12 | the comparison run's log (our run, then the reference's) and its record |

**What the night recorded** (the run log's header and the records):
- **Images:** 15b's tag file (built 9 Oct 12:41, runtime protocol revision 13): gateway `040068250224`, runtime
  `62d8588edbfa`, Workstation `1648c896cdc4`; the web page wasn't used.
- **The harness:** every run recorded commit `23112f6b`, clean.
- **Routes:** `local` version 8, strict off, concurrency 4; `local-text` version 2, text calls, thinking off; `local-32k`
  version 1.
- **The window:** the shared GPU schedule's 20:00 to 01:00 slot; the GPU was held from 20:00 and released at 22:44.
  - The first start at 20:00 stopped at 20:02, before any run, on a quoting error in our block's command.
  - The second (20:02 to 21:12) ran the model check and E2E-CMP-003. It skipped the waves: its window end, past
    midnight, was read as the same day's. That tooling is fixed and tested.
  - The third (21:15 to 22:44) ran the four wave items above.
- **The admission step:** before it, the route `local` at concurrency 4, and the stack's admission at 8 slots, 8 per
  person. For E2E-WAVE-002, `local` went to 8; the admission was already 8 and wasn't changed. After it, `local` was
  back at 4 and the admission at 8 and 8.
- **Accounts:** the four wave runs used an administrator's API token; E2E-CMP-003 used a member's. **c16's Gate 5 runs
  the waves as a member**, the path real users take.

**The technical lead's acceptance (10 Oct, 22:50):** "compaction-50 passed (151 steps, 2.1 M tokens, 690 s, check
passed), REPORT.md 49 part lines + 'Audit reference: KESTREL-4471'. Accepted." **The verdict (11 Oct): Gate 5 is
reached on 15b**: every run passed, M6 is met, and the phase demo is accepted.

## Release candidate c14b-rc (5 to 7 Oct 2026)
**Source:** candidate 14b's integration (4 to 5 Oct 2026, logs `int-16589e55-*`), each row checked
against the logs: a row passes only if each of its test IDs ran and none failed. c14b-rc is those images with the web
image rebuilt from a later commit. The TEST-PLAN §17 Gate 5 row is unchanged since 13h.

| Rows | c14b-rc | Evidence |
|---|---|---|
| CTX-U-001…003, CTX-E-004, CTX-U-005/006, CTX-U-009, CTX-U-012…015 | pass (11) | unit, ignored and runtime nextest steps, none failed |
| CORE-U-022…025, CORE-E-028 | pass (5) | the same steps |
| LLM-C-003…005, LLM-E-009, LLM-U-011/012, LLM-U-017, LLM-C-013, LLM-C-020/022, LLM-I-021 | pass (11) | the same steps |
| PROMPT-U-001…004 | pass (4) | the same steps |
| TOOL-U-004/005, TOOL-U-010, TOOL-E-009 | pass (4) | the same steps |
| CTX-U-010, CORE-U-027, LLM-U-015, TOOL-E-016 (P2) | pass | the same steps |
| 13h's fixes: CORE-E-040 and LLM-U-050 (an unreadable answer retried once), TOOL-U-001 and TOOL-U-034 (an empty or null-word argument counted as absent where the field can't hold it) | pass | the same steps, so E2E-FALLBACK-004 and the phase demo now run on the builds 13h asked for |
| Coverage ≥ 90 % on the pure crates | **pass** | `cov-pure`: 95.57 % lines, 93.23 % regions, 92.16 % functions |
| PERF-030 compaction cost | **pass** | release mode (`perf-release`): p50 1.828 ms, p99 2.066 ms, 14,268 summary and 470 step tokens, as 13h's; against the 26 Sep baseline (1.847 ms, 2.039 ms) −1.0 % and +1.3 %: within 10 % |
| PERF-040 web rendering | pass on 14b's web; **to run on c14b-rc's** | `perf:transcript`: interactive at p50 726 ms, p99 911 ms (≤ 2 s at p50), frames p95 16.8 ms, `noisy: false`. It renders the web from the integration's source, `16589e55`; c14b-rc's web is `101ffeca`, which changes the transcript's banners (four commits under `web/src/transcript`) |
| E2E-WAVE-001 | **pass**, 60/60 = 100 % (94.0–100 %), no harness failure | 7 Oct, below (13h: 58/60) |
| E2E-FALLBACK-004 | **pass**, 26/30 = 86.7 % (70.3–94.7 %) reported, no harness failure | 7 Oct, below: two wrong work, two unreadable replies (13h: 25/30 with one harness failure). The row asks for the rate per model and no harness failure; it sets no rate |
| Phase demo (`local-32k`, `compaction-50`) | **fail**, 0/1: stopped at step 44 by a `read_file` refusal, with no compaction yet | 7 Oct, below; the refusal is fixed in c15 (TOOL-U-051) |

The integration's nextest steps ran 2,323 unit tests, 2,540 in the database step (which runs the ignored tests with the rest)
and 188 runtime tests, every one passing.

### The local-model runs on c14b-rc (7 Oct 2026, results 06:48–07:11)
On the Gate 5 stack with c14b-rc's images (gateway `fde0b91b`, runtime `347b895b`, Workstation `84ff9c47`, from dev
`16589e55`), `qwen3.8-27b` on the shared GPU; one attempt at a time, in the plan's order; E2E-WAVE-002 (Gate 9) last.
Results in `tests/e2e/results/2026-10-07/slot2-*-c14b-rc` on the evaluation host; the harness ran from a clean
checkout of `16589e55`.

- **E2E-WAVE-001: pass, 60/60** (route `local`, tools declared natively), no harness failure and no `provider.retry`.
- **E2E-WAVE-002** (8 at once): **59/60 = 98.3 %** (91.1–99.7 %), no harness failure. The one miss
  (`ts-add-tests-slug`) the harness labelled `model.budget`, after 12 steps and with no retry: `repeated_failure`,
  now `model.stuck`; classified by hand as the model's own wrong tests ([waves.md](waves.md)).
  The admission step (route `local` at concurrency 8; admission already 8 and 8) was applied at 06:59:52 and restored
  at 07:11:13, both logged.
- **E2E-FALLBACK-004: 26/30 = 86.7 %** (70.3–94.7 %) on `local-text`, with no harness failure: the row's
  acceptance ("Success rate reported per model; no `harness` failures") is met. It sets no rate; the 90 % is
  E2E-WAVE-001's, which an earlier summary line printed for every run.
  The misses are the model's: two did the wrong work (`py-add-tests`, `sh-add-tests-strings`), and two stopped after
  three replies in a row that couldn't be read (`sh-build-fix`, `ts-feature-status-filter`, three retries each).
  The harness labelled those two `model.budget`, a budget stop they weren't (`fallback_parse`); they are
  `model.unreadable` since 7 Oct, still the model's.
  The recovery of
  a swallowed reply fired as on 13h: `provider.retry` 23 times for `fallback_parse` and 16 for `native_tools` (one
  more counted as `malformed`).
- **The phase demo: fail, on our tool again, before any compaction.** At step 44 the model called
  `read_file {"cursor":"1","start_line":1,"end_line":200,…}` three times; the tool refused each ("not a cursor from an
  earlier read_file; to read by lines, leave cursor out") and the run ended (`model.tool_args`). The model guessed the
  paging cursor beside a line range. The refusal was our tool's strictness, and the tool is the product's, so this
  stays a product failure of c14b-rc, not a harness failure (unlike 13h's demo, which was reclassified): c15 reads the
  lines and says the cursor was ignored (TOOL-U-051). **Compaction never
  began**, and that is a finding too: the last request was 26,205 tokens by the server's count on a 32,768-token
  route, past where compaction begins (85 % of the 24,576-token input room, about 20,890 tokens, with the route's
  8,192-token Max output) and past the input room itself; the model server accepted it only because it serves a
  65,536-token context. The task's own estimate undercounts dense text such as the demo's ledger (1.64 times on a
  Qwen model, as measured for CORE-U-084), so it never asked for a compaction.
  c15 also triggers on the server's count (CORE-U-084).

**What c15 must rerun before the gate can be reached, and why:**
1. **The phase demo** (`local-32k`, `compaction-50`) on c15's images: the run must show a compaction and the task
   statement intact. It has never yet reached its compaction on a real model: on 13h and c14b-rc our tools stopped it
   first, and on c14b-rc the trigger would also have come late. c15 has both fixes (TOOL-U-051, CORE-U-084).
2. **E2E-FALLBACK-004** on c15's images: it passed its row on c14b-rc (26/30 reported, no harness failure), but c15
   changes what every task runs on, as for E2E-WAVE-001 below, so the pass is c14b-rc's. The rerun reports the rate
   (13h: 25/30; c14b-rc: 26/30) and must have no harness failure.
3. **E2E-WAVE-001** on c15's images: it passed on c14b-rc, but c15 changes what every task runs on (the instructions,
   v12; the compaction trigger; `read_file`), so the pass is c14b-rc's, not c15's.
4. **PERF-040 on c14b-rc's web** (`pnpm run perf:transcript` from a `101ffeca` tree), on a quiet host: after the soak;
   no GPU.
5. **The technical lead's evaluation**, recorded at the end of this page.

## Candidate 13h (1 Oct 2026)
**Source:** the technical lead's integration run on `5b79a932` (1 Oct 18:30–23:56 +08:00, logs `int-5b79a932-*`), each
row checked against the logs: a row passes only if each of its test IDs ran and none failed.

| Rows | 13h | Evidence |
|---|---|---|
| CTX-U-001…003, CTX-E-004, CTX-U-005/006, CTX-U-009, CTX-U-012…015 | pass (11) | unit and ignored nextest steps (13g's draft counted these as 12; they are 11) |
| CORE-U-022…025, CORE-E-028 | pass (5) | unit and ignored nextest steps |
| LLM-C-003…005, LLM-E-009, LLM-U-011/012, LLM-U-017, LLM-C-013, LLM-C-020/022, LLM-I-021 | pass (11) | unit and ignored nextest steps |
| PROMPT-U-001…004 | pass (4) | unit and ignored nextest steps |
| TOOL-U-004/005, TOOL-U-010, TOOL-E-009 | pass (4) | unit, ignored and runtime nextest steps |
| CTX-U-010, CORE-U-027, LLM-U-015, TOOL-E-016 (P2) | pass | the same steps; outside the release's P0/P1 audit |
| Coverage ≥ 90 % on the pure crates | **pass** | G-04: 95.90 % lines, 93.50 % regions across the pure crates (`cov-pure`) |
| PERF-030 compaction cost | **pass** | release mode (`perf-release`): p50 1.815 ms, p99 2.056 ms, 14,268 summary and 470 step tokens; against 13h-pre −1.2 % and −4.0 % with the same tokens, against the 26 Sep baseline (1.847 ms, 2.039 ms) −1.7 % and +0.8 %: within 10 % |
| PERF-040 web rendering | **pass** | `perf:transcript`: interactive at p50 910 ms, p99 1,295 ms (≤ 2 s at p50), frames p95 16.8 ms. The run marked itself `noisy` (the host's load above three quarters of its CPUs), so its figures are an upper bound |
| E2E-WAVE-001 | **pass** | 2 Oct, `local` (native, strict off), 20 tasks × 3: **58/60 = 96.7 %** (88.6–99.1 %), no harness failure; 1 `model.wrong_work`, 1 `model.budget` (now `model.stuck`) ([waves.md](waves.md)) |
| E2E-FALLBACK-004 | **fail** (1 harness failure) | 2 Oct, `local-text`: **25/30 = 83.3 %** (66.4–92.7 %), against 19/30 on 29 Sep; 3 `model.wrong_work`, 2 `model.budget` (now one `model.stuck`, sh-config-env-spec, and one `model.unreadable`, sh-feature-backup-dry-run), 1 `harness.provider` (below) |
| Phase demo (`local-32k`, `compaction-50`) | **fail**, on our tool | ended at turn 6 on three refusals of `read_file`'s `cursor: ""`, at 11.5k of 32k tokens: compaction was never reached (below) |

The integration's nextest steps ran 2,054 unit, 2,135 ignored and 180 runtime tests, every one passing.

### The local-model runs on 13h (2 Oct 2026, 10:04–11:59)
On the Gate 5 stack moved to 13h's images, the server at 4 sequences, one attempt at a time, in the plan's order;
E2E-WAVE-002 (Gate 9) last, the server restarted at 8 sequences.

- **E2E-WAVE-001: pass**, 58/60 (above).
- **E2E-WAVE-002** (8 at once): **59/60 = 98.3 %** (91.1–99.7 %), no harness failure. The admission step (route
  `local` at concurrency 8; admission was already 8 and 8) was applied at 11:26 and restored at 11:51, both logged.
- **E2E-FALLBACK-004: 25/30**, the first run with the recovery of a reply the server swallowed (CORE-U-065). The
  retry fired often and mostly recovered: `provider.retry` 16 times for `fallback_parse` and 14 for `native_tools`,
  in 20 of 31 attempts. That recovery declares the tools natively on this text-protocol route, on the server's own
  evidence that it parsed a call natively; it is by design and now recorded in PRD FR-2 and an ADR-0007 amendment
  (15 of 55 requests in the capture below).
  - *The harness failure.* `ts-fix-date-range` attempt 29 ended at step 4 with `provider_error`, reason `protocol`:
    27 completion tokens and no text. No byte of the answer survived (the relay keeps no bodies; the task was gone),
    and `Protocol` errors were never retried. Its rerun passed.
  - *The capture.* The same task six times through a proxy that kept every request and answer (55 exchanges): 6 of 6
    passed, every stream valid, every streamed tool call complete. Not reproduced; recorded in TEST-PLAN §19a.
  - *Mitigation.* An answer that can't be read is now its own error, retried once as a cut stream is (CORE-E-040,
    LLM-U-050); a refused request is never retried.
- **The phase demo: fail, on our tool, not on compaction.** At turn 6 the model called `read_file` with `cursor: ""`
  and a line range, three times; the tool refused each ("not a cursor from an earlier read_file") and the run ended
  `repeated_failure` at 11.5k of 32k tokens, before any compaction. The harness counted it `model.budget`. Under
  TEST-PLAN §16's rule it is `model.tool_args`, reviewed by hand, and this refusal was our strictness: **reclassified
  as a harness failure.** Fixed for the next release: an empty optional string argument counts as absent for every
  builtin tool (TOOL-U-001, TOOL-U-034), and a refusal names the field and the way out.
- **The demo rerun (2 Oct, 15:40–15:46): fail, on our tool again, later.** With the empty-argument
  fix it went 69 steps (18 before) and then ended `repeated_failure` on three refusals of
  `grep {"cursor": "None", "file_type": "None", "glob": "None", …}`: this model fills optional fields with the word
  "None". The harness classed it `model.tool_args`, the refused call in the evidence; the hand review finds our
  strictness, so it is **reclassified as a harness failure**. Fixed for the next release by a rule decided on 2 Oct: a
  null-word (`None`, `null`, `undefined`, any case) counts as absent wherever the field can't really hold it, every
  cursor and every closed list (an `enum`, and `grep`'s file type), but never in a free string such as a path or a
  pattern (TOOL-U-001, TOOL-U-034). The demo reruns on that build after 13h's 24-hour run.

**What is missing before the gate can be reached:**
1. ~~E2E-WAVE-001 on the candidate's images~~: passed on 13h, 2 Oct.
2. E2E-FALLBACK-004 without a harness failure: a run on the build with the answer retry (CORE-E-040).
3. The phase demo (`local-32k`) on the build with the null-word rule (after the 24-hour run).
4. All three need the shared GPU and the local models; when they run is the technical lead's to schedule, or the
   integration gains a local-model step.

## Candidate 13g (1 Oct 2026)
**Source:** the technical lead's integration run on `86bdd6ee` (1 Oct 03:59–07:32 +08:00, logs `int-86bdd6ee-*`) and
the TEST-PLAN audit of it (1 Oct 2026), each row checked against the logs.

| Rows | 13g | Evidence |
|---|---|---|
| CTX-U-001…003, CTX-E-004, CTX-U-005/006, CTX-U-009, CTX-U-012…015 | pass (12) | unit and ignored nextest steps |
| CORE-U-022…025, CORE-E-028 | pass (5) | unit and ignored nextest steps |
| LLM-C-003…005, LLM-E-009, LLM-U-011/012, LLM-U-017, LLM-C-013, LLM-C-020/022, LLM-I-021 | pass (11) | unit and ignored nextest steps |
| PROMPT-U-001…004 | pass (4) | unit and ignored nextest steps |
| TOOL-U-004/005, TOOL-U-010, TOOL-E-009 | pass (4) | unit, ignored and runtime nextest steps |
| CTX-U-010, CORE-U-027, LLM-U-015, TOOL-E-016 | P2: not audited | outside the release's P0/P1 audit |
| Coverage ≥ 90 % on the pure crates | **pass** | G-04: 95.90 % lines, 93.50 % regions across the pure crates (`cov-pure`) |
| PERF-030 compaction cost | **not measured** | its timing test is ignored by default and ran only inside the coverage run, where instrumentation fails it; its logic test passed |
| PERF-040 web rendering | **not measured** | `pnpm run perf:transcript` is not an integration step |
| E2E-WAVE-001 | **not run** on 13g | needs the local-model box; last measured 27 Sep (60/60 with `strict = "off"`, below) |
| E2E-FALLBACK-004 | **not run** on 13g | needs the local-model box; last measured 29 Sep (19/30, one harness failure, so the row was not met) |

**What is missing before the gate can be reached:**
1. PERF-030 in a plain `--run-ignored` run (no coverage) on the candidate, against the 26 Sep baseline.
2. PERF-040, `pnpm run perf:transcript`, on the candidate (≤ 2 s to interactive, p50 over 50 runs).
3. E2E-WAVE-001 on the candidate's images with the local model (≥ 90 %, no harness failure).
4. E2E-FALLBACK-004 on the candidate's images, judged by its row as written (decided on 1 Oct 2026): a success
   rate reported per model and no harness failures, with no completion threshold. So the 28 Sep run (10/30, no harness
   failure) met it and the 29 Sep run (one harness failure, its rerun passed) did not; the history below judged both by
   their rates, which the row doesn't ask for.
5. The phase demo (`local-32k`) on the candidate: on 28 Sep it passed 4 of 12 attempts, two with the model's own
   compaction summary, and the 8 failures were one `read_file` cursor loop fixed since (TOOL-U-001; history below).

## History (26 to 29 Sep 2026)

### What was independently verified
- Every unit, contract and edge ID in the row has named tests. A filtered run across the context, core, model, prompt,
  tool and runtime crates, including the ignored tests and without containers: **197 run, 197 passed**.
- **Coverage:** 96.22 % lines (93.38 % regions) across the pure crates, measured with `cargo llvm-cov`.
- The local-model runs used a fixed release build of the E2E harness (ef79747) and its task snapshot, never rebuilt
  during the runs, against stack images pinned by digest and recorded in the run directories.

### Measurements
**Environment:** one Linux workstation; a 27B open-weight model (4-bit floating-point weights) served by an open-source
inference server on one 32 GB GPU; 64k context, 4 concurrent sequences; native tool calls on route `local`; the
text protocol on `local-text`.

| ID | Result | Target | Verdict |
|---|---|---|---|
| PERF-030 | p50 1.847 ms, p99 2.039 ms; +0.3 % time and ±0 tokens against the previous baseline | within 10 % of baseline | pass |
| PERF-040 | report on record | row target | pass |
| E2E-WAVE-001 (`local`, 60 attempts), 26 Sep | **48/60 = 80.0 %** (95 % Wilson 68.2–88.2 %), 1 harness failure (a check timeout under load; its rerun passed) | ≥ 90 %, no harness failure | fail |
| E2E-WAVE-001 (`local`, 60 attempts), 27 Sep rerun with `strict = "off"` | **60/60 = 100.0 %** (95 % Wilson 94.0–100 %), no harness failure | ≥ 90 %, no harness failure | **pass** |
| E2E-FALLBACK-004 (`local-text`, 30 tasks), 26 Sep | **18/30 = 60.0 %** (42.3–75.4 %), 12 harness failures (11 empty responses: the server drops the answer after the model's reasoning on its thinking-on path, 1 sandbox-create stall) | row target, no harness failure | fail |
| E2E-FALLBACK-004 (`local-text`, 30 tasks), 28 Sep, the shipped configuration (thinking off on `local-text`) | **10/30 = 33.3 %** (19.2–51.2 %), no harness failure | row target, no harness failure | **fail** as recorded; meets its row (no harness failure); see 7 Oct |
| E2E-FALLBACK-004 (`local-text`, 30 tasks), 29 Sep, candidate 6 (a14afc0), `tqa_call`, thinking off | **19/30 = 63.3 %** (45.5–78.1 %), 1 harness failure (`harness.provider`; its rerun passed). The 11 failures: 7 `model.wrong_work`, 4 `model.budget` (now `model.unreadable`) | row target, no harness failure | **fail** |
| Demo (`local-32k`) | pass: 154 steps, one compaction, the codeword intact. The compaction used the deterministic fallback summary: the 1024-token summary call spent every token on reasoning (see below) | row target | pass, with a finding |

### What the failures turned out to be
- **Most of the wave's misses are one defect:** 30 of 60 native-path attempts contain a refused `edit_file` whose
  `old_text` lost its line breaks. None of the 40 text-protocol attempts do.
- **Raw capture (2026-09-27):** unstreamed responses with token ids show the model's own tokens and the parsed
  arguments are identical. The model drops the backslash when it JSON-escapes `old_text` inside the `edits` array,
  writing a bare `n` for each line break, while `new_text` in the same call is escaped correctly. The server's parser
  passes this through unchanged.
- **Wording A/B (2026-09-27):** the same three tasks × 3 on the current build, with and without a schema wording that
  asks for exact copies with line breaks.

  | | Baseline | Wording |
  |---|---|---|
  | Completion | 4/9 | 2/9 (within noise at n = 9) |
  | `old_text` with a line break | 0/43 | 0/64 |
  | Exact matches | 14 % | 14 % |

  The wording addresses the content, not the escaping inside an array parameter, and it did not help.
- **Cause found: strict tool calls (2026-09-27).** Four runtime builds, 9 attempts each, on the same server:

  | Build | `edit_file` sent strict | Completion | `old_text` with a line break, matched |
  |---|---|---|---|
  | B-r2 | yes (every tool) | 6/9 | 0/34 |
  | A3 (every builtin strict-valid) | yes | 5/9 | 0/27 |
  | A2 | yes | 5/9 | 3/15 |
  | A1 (`edit_file` schema not strict-valid) | no | **9/9** | **19/21** |

  A direct A/B on the server, with the same `edit_file` schema and prompt and an `old_text` that must span two lines,
  checked on the raw response bytes: with `strict: true`, 0/5 calls keep a line break (3 join the two lines, 1 splits
  the edit into two single-line edits, 1 keeps only the first line); without it, 5/5 keep it in one edit (a first 6 + 6 run: the same). The server's constrained
  decoding for strict tools (vLLM 0.30, `qwen3_coder` parser) never emits a line break inside a tool's string argument.
  This explains the raw capture above: the tokens were already constrained. Local routes on this server therefore set
  `strict = "off"` (docs/ARCHITECTURE.md, route settings); no adapter branch is needed.

### The rerun (2026-09-27, 21:59–23:02)
- **Build:** gateway, runtime and Workstation images from one dev commit (dd97549), checked for their revision label
  and a single runtime binary; the E2E harness built from the same commit with the task snapshot of ef79747 (the 20
  tasks byte-identical). The only intended change from 26 Sep: `strict = "off"` on route `local`, set through the admin
  routes API.
- **Proof on the wire, before the counted run:** one uncounted attempt went through a byte recorder in front of the
  server. Its upstream requests carried all 9 tools with `strict: false`, none `true`. The recorder was then taken out
  of the path, and all 60 counted attempts went to the server directly, as on 26 Sep.
- **Result:** 60/60, no harness failure and no budget stop (26 Sep: 48/60, 10 budget stops, 1 harness failure). 61
  minutes of model wall time, as on 26 Sep.
- **Edits:** 107 `edit_file` edits; 61 carry a line break in `old_text`; 97 match the file exactly once; no attempt has
  a refused edit that lost its line breaks (26 Sep: 30 of 60).
- **Compaction:** the one compaction (the long HTTP migration task) still used the fallback summary, as expected on
  this build (see below).

### The two server checks after the rerun (direct requests, 23:03–23:06)
- **Compaction summary:** the demo's summary request (1024 output tokens, thinking on), streamed and not: both ended
  with `finish_reason` `length`, all 1024 completion tokens were reasoning, and the content was empty. The summary now
  gets a step's output budget on its route, at least 1024 (CORE-U-057, ADR-0009).
- **Empty answers on `local-text`:** the captured request of a failing attempt comes back empty with either fence label
  (`tool_call`, or a renamed one). With `logprobs` on, or with thinking off, both labels come back intact: the model
  writes its reasoning, closes it, then prose and a correct call. The server drops everything after the reasoning on
  its thinking-on path without `logprobs`. This check rested on single samples per variant; the 28 Sep A/B below
  suggests the label does matter, so it was not conclusive (ADR-0007, amended). `local-text` turns thinking
  off in its route extras, and an empty answer on a text-protocol route now fails with a reason that says what was
  seen (CORE-U-055).

### E2E-FALLBACK-004 on the shipped configuration (2026-09-28, 01:44–01:55)
- **Configuration:** the candidate's images (deef3f2): system prompt version 4 (with the untrusted-content paragraph),
  the text protocol writing `tool_call`, and `local-text` with thinking off in its route extras.
- **Result:** 10/30 = 33.3 % (Wilson 19.2–51.2 %), no harness failure. The empty answers are gone (26 Sep: 11), but the
  model fails the work itself more often.
- **The 20 failures:** 7 stopped without writing a call (the reply announces one, then ends); 5 wrote a call the parser
  refused (`fallback_parse`: malformed, schema or too many); 4 fell into a loop of closing-tag lines (`</invoke>`,
  `</untrusted_content>`); 4 made calls and did the wrong work.
- **Direct A/B right after (the captured text-route request, thinking off, non-streamed, 5 per variant):**

  | Label | Untrusted-content paragraph | Replies with a call block | Empty replies |
  |---|---|---|---|
  | `tool_call` | no | 1/5 | 2/5 |
  | `tool_call` | yes | 3/5 | 0/5 |
  | `tqa_call` | no | 4/5 | 0/5 |

  Replies with many completion tokens but little or no content show generated text dropped on the server with
  thinking off too. The `tool_call` label seems to trigger it more often, and the paragraph shows no harm in single
  requests (the closing-tag loops appeared only in the multi-turn run). Five per variant is too few to decide: the
  earlier conclusion that the label is irrelevant rested on single samples, and a proper A/B (20 per arm, label ×
  thinking) is planned before the label is changed again.

### The label × thinking A/B (2026-09-28, 06:43–06:48)
- **Design:** the same captured text-route request as above (a first step), non-streamed, 20 per arm, label
  (`tool_call`, `tqa_call`) × thinking (on, off), interleaved by round with each round's four arms sent together; raw
  replies kept. A reply counts when it holds a fenced call block.

  | Label | Thinking | Replies with a call block (Wilson 95 %) |
  |---|---|---|
  | `tool_call` | on | 12/20 (39–78 %) |
  | `tool_call` | off | 2/20 (3–30 %) |
  | `tqa_call` | on | 17/20 (64–95 %) |
  | `tqa_call` | off | 14/20 (48–85 %) |

- **Every other reply is dropped text,** not a wrong call: all 80 end with `stop`, and each reply without a block is
  either empty after 44 to 102 completion tokens or a first line of prose with the rest missing.
- **The label matters with thinking off** (2 against 14, two-sided Fisher exact p = 0.0002) and isn't shown to with
  thinking on (12 against 17, p = 0.16). Thinking matters with `tool_call` (12 against 2, p = 0.002) and not with
  `tqa_call` (17 against 14, p = 0.45). This corrects the single-sample check after the rerun.
- **The shipped configuration is the weakest arm:** `tool_call` with thinking off, 2/20.
- **Limits:** one request, one step and one server version. This is a per-step rate, not task completion, which
  E2E-FALLBACK-004 measures.

### The compaction summary on the real model (2026-09-28, 06:49–06:51)
- Two direct summary requests to the same model with thinking on and `max_tokens` 8192, the budget the CORE-U-057
  rule gives `local-32k`: both ended with `stop` and parsed with all six fields (2.1 KB and 1.7 KB of content).
- Reasoning took 3665 and 4967 tokens. 8192 fits, but the second reply wouldn't fit the 4096 default that applies
  when a route declares no `max_output`.

### The compaction demo on `local-32k` (2026-09-28, 06:52–07:40)
- **Configuration:** the candidate's images (deef3f2) with the summary's output budget (43fde0f), the `compaction-50`
  task (a fifty-message ledger audit), one attempt per results directory, repeated until 07:40.
- **Result:** 12 attempts, 4 passed, no harness failure.
  - Two passes compacted with the model's own summary: once with 3133 completion tokens (2477 reasoning), and once
    twice, the second time with 5008 (3665 reasoning). No compaction fell back.
  - The other two passes stayed under the compaction threshold, so they don't show compaction.
- **All 8 failures are one tool-call loop:** `read_file` called with `cursor` set to `"None"`, `""` or `"none"` next
  to a line range, refused with "cursor excludes start_line and end_line", and repeated until `repeated_failure`,
  between turn 2 and turn 16.
- **Changes for the next candidate:**
  - the refusal names what to leave out, and a cursor the tool didn't issue is refused as such (TOOL-U-001);
  - a route without a declared output gives the summary 8192 tokens (CORE-U-057), since 5008 would have been cut off
    at the old 4096.

### E2E-FALLBACK-004 with `tqa_call` (candidate 545c3d0, 2026-09-28, 10:12–10:24)
- **Configuration:** the candidate's images (545c3d0), with the text protocol writing `tqa_call` (instruction version 4)
  and `local-text` with thinking off. Harness and task snapshot as in the earlier runs.
- **Acceptance (TEST-PLAN):** "Success rate reported per model; no `harness` failures". The row sets no rate. The 90 %
  in the harness's summary line is PRD metric M1, which is about the E2E-WAVE set, not this row.
- **Success rate:** 21/30 = 70.0 % (Wilson 52.1–83.3 %). With `tool_call` and thinking off (01:44 above) it was
  10/30 = 33.3 % (19.2–51.2 %).
- **The blocker, harness failures:** 8 attempts over 36 (6 of them the harness's own reruns). After the reruns, 2 of the
  30 rounds still end as harness failures. Every one of them is a text-route reply that never arrived as text:
  - 6 end as `empty_response` and 2 as `protocol`;
  - the evidence is the server's own usage: each bad reply reports 14 to 150 completion tokens and 0 reasoning tokens,
    and 0 characters of text arrived;
  - what the server actually sent is not captured, so no text at all, or the reply turned into native tool calls
    (our Chat decoder reads those even on a text route, and a malformed one is a protocol error), is still open;
  - one of the `protocol` failures happened with no retry at all.
- **Retries:** the loop gives an empty reply one retry. Of 21 retried bad replies, 13 came back good and 8 bad again.
  The harness then reruns an attempt once: 4 of 6 recovered.
- **How the row can pass (proposed, in order):**
  1. Capture raw responses of bad replies behind a recorder (the captured request repeated, or one run), to tell "no
     text sent" from "text sent as tool calls".
  2. Try a request change on the server, as a 20-per-arm A/B: `logprobs` on (on 27 Sep it kept thinking-on replies
     intact, single samples) and `tool_choice: "none"` (it stops the server from reading calls out of the reply).
     Both are route extras, with no product change.
  3. Recover in the product if a bad reply stays independent enough: up to 3 tries for an empty reply on a text
     route, each visible as `provider.retry`. At the observed 8 in 21 bad-after-bad this leaves about 1 of today's 8,
     so it lowers the count but can't guarantee zero on its own.
  4. If the capture shows calls, decide the open product question from 26 Sep: whether a text route accepts the
     server's native calls.
- **Not counted:** a first start at 09:44 went 0/30, because every model call was refused with HTTP 502. The stack's
  gateway had been recreated without its model network. A false start at 10:11 was stopped after one attempt.
- The compaction demo's repeat on this candidate moved to the next GPU window.

### E2E-FALLBACK-004 on candidate 6 (2026-09-29, 06:49–06:59), and what swallows the calls
- **Configuration:** candidate 6 (a14afc0), `local-text` with `tqa_call` and thinking off, the review stack, one GPU
  window (from 06:34, when the shared GPU was free).
- **Result:** **fail, 19/30 = 63.3 %** (Wilson 45.5–78.1 %), with 1 harness failure. That follows the precedent of
  E2E-WAVE-001 on 26 Sep, which failed on one harness failure whose rerun passed. The 30 final attempts are 19
  `pass`, 7 `model.wrong_work` and 4 `model.budget` (all four `fallback_parse`: `model.unreadable` since 7 Oct):
  - **all 7 `model.wrong_work` ended after one step** at about 1.9k or 3.8k tokens: py-add-tests #2,
    py-feature-csv-export #5 and #6, py-fix-off-by-one #7, sh-add-tests-strings #15, sh-build-fix #17 and #18;
  - the 4 `model.budget` stops were sh-config-env-spec #19, sh-feature-backup-dry-run #23,
    ts-feature-status-filter #27 and ts-fix-date-range #30;
  - **the harness failure,** attempt #25 (ts-feature-status-filter): the first step's reply came back empty three
    times (`provider.retry` `empty_response` twice), and the run failed with `provider_error` ("the server returned
    no text on a text-protocol route"). The rerun, #26, passed.
- **The raw capture (slot8, 07:00–07:01),** the captured text-route request sent straight to the server:
  - 40 replies gave 27 call blocks, 11 prose only and 2 empty; the 20 with `logprobs` on gave 13, 6 and 1; the 20
    with `tool_choice: "none"` gave 14, 3 and 3;
  - **in the empty reply with `logprobs`, the model does write a call:** the sampled tokens are
    `\n>python3 -m unittest discover -s tests -t . 2>&1 | head -50>`. That is a call in the model's **native** format
    (`<parameter=command>…`, whose markup tokens don't show as text), not our fenced `tqa_call` block;
  - every stream delta of that reply is empty while those tokens are generated: the server holds them back;
  - **the prose-only replies carry about 40 completion tokens more than their text accounts for** (60 to 110 tokens
    for 79 to 96 characters). That fits "a sentence, then a call that was swallowed", which is the one-step
    `model.wrong_work` pattern.
- **The cause:** the inference server runs with automatic tool choice and a tool-call parser for this model family.
  When the model writes its native call format, the server's parser takes it as a tool call. A text-route request
  declares no tools, so the call is dropped: no text and no `tool_calls` reach us. `tool_choice: "none"` doesn't stop
  it. The loop then sees either an empty reply (retried, and after the retries a `provider_error`, which the harness
  classes `harness.provider`) or a sentence with no call (a final answer, so the check fails: `model.wrong_work`).
- **The confirming capture (07:03):** the same request with the eight tools also declared natively (OpenAI `tools`)
  gave **20 of 20 replies as `tool_calls`**. The model calls reliably in its own format, and the server parses it when
  tools are declared.
- **Is a provider error a harness failure?** Not in the sense the row means. The empty reply, and the one-step
  endings, come from the model server's configuration interacting with our text route. The model and the harness are
  not at fault. The harness has no class for "the server swallowed the reply", so it is scored `harness.provider`
  (and, after a sentence, `model.wrong_work`). A class of its own, `provider.swallowed`, recognised by "completion
  tokens reported, no text and no call delivered", would keep such attempts out of both counts. That is proposed
  here and not yet built.
- **Fix options: which works on installations we don't run?**
  1. *Serve text routes without automatic tool choice.* This works on our stack, but it's the operator's server
     setting, and one server often serves a native route and a text route at once (as here). Guidance for the admin
     guide, not a fix we can rely on.
  2. *Recover native-format calls in our text parser.* This helps only where the server has no parser and the
     model's native markup arrives as text. On a server with a parser the call never reaches us, so this alone doesn't
     fix what we measured.
  3. *Detect a swallowed reply and adapt (recommended).* A text-route reply whose usage reports completion tokens
     but which delivers no call (no text, or text that accounts for far fewer tokens) is a swallowed reply. The
     relay repeats that request once with the tools declared natively. Our decoder already accepts native
     `tool_calls` on a text route. If the server refuses declared tools (a server without a parser answers 400 to
     `tools` with automatic choice), the route is marked "no server parser" and stays text-only. This depends on no
     server setting and doesn't break servers without a parser.
  4. *Tell administrators at the route check.* The route probe sends one text-protocol request; if the reply is
     swallowed, it says so and recommends native tool calls for the route.
  - **Recommendation:** 3 in the product, with 2 as a cheap companion and 4 in the route check; 1 in the
    deployment docs. Each is built test-first, and the row then runs again in the next GPU window.

### Limits of the acceptance and next steps
- **A known mismatch in candidates up to 545c3d0 and a14afc0:** after an unreadable text-protocol reply, the retry
  prompt says "tool_call block" while the instructions write `tqa_call` (since instruction version 4). The model is
  told two labels after a parse failure. That is a possible factor in the text-route results, the 29 Sep 07:00 run on
  a14afc0 included. It is fixed for the next candidate (CORE-U-022).
- E2E-WAVE-001 passes on the rerun. **Gate 5 stays open on E2E-FALLBACK-004:** 19/30 on candidate 6 (29 Sep) with 1
  harness failure, where the row asks for none (21/30 with 8 on 545c3d0; 10/30 on deef3f2 before the label change).
  The 29 Sep capture found the cause of the one-step endings and the empty replies: the server's tool-call parser
  swallows the model's native calls on a text route (above).
- **Next:**
  - for E2E-FALLBACK-004: the swallowed-reply detection with its native retry (option 3 above), test first, then
    the row again in the next GPU window;
  - the compaction demo on candidate 545c3d0 (moved to the next window);
  - the compaction demo again on the next candidate, with the `read_file` refusals that name what to leave out;
  - a report of the dropped-text behaviour to the inference server's project: drafted, with the A/B as its evidence;
    publishing needs the Product Owner's approval.
- **Known error in the record:** the `run.json` files of the runs on the deef3f2 images (E2E-FALLBACK-004 on 28 Sep
  01:44–01:55 and the compaction demo 06:52–07:40) record the base runtime image as `fe21cf21…`, but the stack's base
  template ran `c0544511…`. These tasks run on the Workstation template, whose recorded id (`fcae4b34…`) matched the
  stack, so the results stand. Later runs check the stack's templates before they start.
- The runs are single-environment measurements. Completion rates carry the Wilson intervals shown, and nine-attempt
  comparisons only indicate direction.
