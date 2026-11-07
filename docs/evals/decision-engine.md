# The decision engine, with and without: the measurement plan (E2E-DECISION-008)

Status: **planned, not run** (12 Oct 2026). DEV-PLAN Phase 9 item 1 asks for "the with-versus-without measurement",
and PRD FR-27 and ADR-0013 point 9 say why: an engine's effect "is measured, never assumed". This note gives the plan,
what is built for it, and what is left before a GPU window.

## The question
Does a decision engine that sends routine steps to a cheaper route save tokens, steps or wall time, without losing
completion? That cost at equal quality is what the seam exists for (ADR-0013 point 9).

## What is built
- **The engine:** `examples/decision-reference` (Apache-2.0, standard library only). After a step whose calls all only
  read (`read_file`, `glob`, `grep`, `read_notes`) and all succeeded, it sends the next step to the route named by
  `--cheap-route`. Every other step stays on the task's own route. It never narrows tools, acts, compacts or stops.
  Exposure `none`: it sees tool names and statuses only.
- **The harness:** `tqa-e2e --engine-profile <id> --max-engine-calls N [--max-wall-minutes M]`. Each task's rounds
  run once per arm, ABBA per round, and a rerun keeps its arm and place. Each attempt line records `arm`, `round`,
  `pair_position` and its decisions (TK-U-032, TK-U-033, TK-U-034; `tests/e2e/README.md`).
- **The analysis:** `tests/e2e/decision_compare.py RUN_DIR [--out FILE]`. It reads a run and never starts one. It
  reports:
  - completion per arm with Wilson intervals, the exact McNemar test, and the paired completion difference with
    its bootstrap interval;
  - tokens, steps and wall time as paired differences with bootstrap intervals;
  - the ABBA order effect;
  - the engine's calls, latency share, decisions and fallbacks;
  - **the routes** (12 Oct): how often each was chosen before a step, and the tokens each spent, from the run's
    `events/`;
  - **the verdict** (12 Oct): the rule below.

## The design
| | |
|---|---|
| **Tasks** | The E2E task set, `tests/e2e/tasks`: 20 tasks (Python, shell and TypeScript; add tests, fix, refactor, docs, config, two long migrations, log triage) |
| **Arms** | Control: `ModelDecides` on the task's route `local`. Engine: a profile naming the reference engine, with `--cheap-route local-fast` |
| **Routes** | `local`: the shared GPU's Qwen3.8-27B, thinking on, native tools (the eval nights' route). `local-fast`: the same server and model, thinking off (`chat_template_kwargs.enable_thinking=false`), native tools, declared as `local`'s fallback, which is how a profile's engine may choose it |
| **Size** | 3 rounds × 2 arms × 20 tasks = **120 attempts**, 60 pairs; ABBA per round |
| **Fixed** | One model, one route set, one candidate's images, one harness build, one tree; concurrency 1 (below) |
| **Caps** | `--max-engine-calls 1500` (the flag is required; the reference engine costs nothing) and `--max-wall-minutes` set to the window. The night's stop rules (cutoff, Team B pause) apply as on eval nights |

**Two changes from ADR-0013 point 8, for the lead's decision:**
1. **The engine arm is the reference engine, not the outside typed-question service.** It costs no money, needs no
   credential, and needs no product-owner OK for a live smoke, so it can run in the next free window. The
   typed-question service is the same measurement with a second engine profile. It follows after the owner's OK,
   with the caps point 8 sets.
2. **`local-fast` keeps native tools** (point 8 had `local-text`, the text protocol). Only thinking changes between
   the routes, so a difference is thinking's cost on routine steps, not the tool protocol's parsing. A `local-text`
   run is a later variant if wanted.

**Concurrency 1.** Wall time is one of the metrics. With several attempts at once, each attempt's time depends on what
else the GPU is serving, and the pair's two attempts may meet different loads. At concurrency 1 a pair runs
back-to-back on an otherwise idle server. *Est.* about 2 to 3 hours for 120 attempts, at night 1's short-task times
(0.5 to 3 minutes each). Concurrency 4 would take about 1 hour, with the wall-time result marked as under load.

## What is measured
From the E2E-DECISION-008 row ("tokens, steps, wall time and completion reported side by side with confidence
intervals; no claim without this data") and ADR-0013 point 9:
- completion: each arm's pass rate (95 % Wilson), the discordant pairs (exact McNemar), and the paired difference
  (bootstrap 95 %);
- tokens (prompt plus completion), steps and wall time per attempt: paired differences, engine minus control, with
  bootstrap 95 % intervals;
- the engine: its latency and share of wall time, the fallback rate by reason, decisions by kind, and **how often each
  route was chosen and what each route spent**;
- the order effect (ABBA), so a warm cache that favours the second attempt of a pair shows.

## The pass rule
E2E-DECISION-008 is a measurement, P2. It passes when the report exists with every number above and its intervals,
for at least 10 pairs. ADR-0013's rule decides what the report may claim, and `decision_compare.py` applies it:
- **Fewer than 10 pairs:** no claim either way.
- **A regression,** whatever the saving: the paired completion difference's 95 % interval lies wholly below zero.
- **A saving,** per metric: its paired difference's 95 % interval lies wholly below zero.
- **A cost,** per metric: the interval lies wholly above zero (the engine's latency could do this to wall time).
- **Otherwise:** no measurable effect.

## Before the run
1. The routes on the stack the run uses: `local-fast` added, and `local` given `fallbacks = ["local-fast"]`. A
   fallback also takes over on a provider failure (ADR-0025) in both arms. Each attempt's events show it, and the
   report counts it under the routes.
2. The engine as a container on the stack's control network, from the candidate's runtime image (which has Python):
   `python3 examples/decision-reference/engine.py --port 8700 --cheap-route local-fast`. It's declared in
   `gateway.toml` (`url = "http://decision-reference:8700"`, `insecure_http = true`, `exposure = "none"`), since
   only the file may set plain http (as the settings suite's engines are). `tools/ext-check decision` against it first.
3. A profile naming it (`decision_engine = "reference"`), and its id passed as `--engine-profile`.
4. The run: `tqa-e2e --route local --repeat 3 --concurrency 1 --engine-profile <id> --max-engine-calls 1500
   --max-wall-minutes <window> --out <run>`, then `decision_compare.py <run> --out <run>/decision.md`.
5. A dry run first on the scripted model: each arm completes, and the engine arm's events hold `decision` events
   choosing both routes.

It runs on its own stack or the eval stack after night 2, in a GPU window the lead gives. Not before night 2 is set.

## Limits
- One model, one family of tasks (coding), local routes only. The result is the reference policy's on this model,
  not any engine's.
- Thinking off is the saving's source and its risk. A routine step after reads may still need thought, and the
  completion rule is there to catch that.
- Three rounds per task is the ADR's size. A small effect on completion stays inside its interval, and the report
  says so rather than claiming it.
