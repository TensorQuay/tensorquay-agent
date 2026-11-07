# Comparison with the open reference: E2E-CMP-003 (PRD M6)

**State:** measured on 2026-09-28 on the release candidate `a14afc0`: 48/50 against the reference's 50/50, a
difference of −4.0 points with a 95 % interval of −10.0 to 0.0. M6 is **not yet shown**: the interval reaches 10
points below the reference, past the 5-point bar. Both our misses were harness failures, not the agent's: the task's
check timed out on work that passes (see Results).

PRD M6 asks for task completion within 5 points of OpenHands, the open reference product, on a shared subset of the
task set with the same model.

## Method: everything the same except the product
- **Tasks:** the ten tasks tagged `cmp` in `tests/e2e/tasks`, five attempts each, 50 per side. The statement is the
  task's `task.md`, verbatim, with no hints.
- **Starting point:** both sides start from the same seeded workspace and baseline commit (`tqa-e2e --seed-only` for
  the reference, the driver's own seeding for ours).
- **Budgets, the same on both sides, and where each is set:**

  | Budget | Our side | The reference |
  |---|---|---|
  | Steps (one model request and its tool calls) | the task's `max_steps`; the harness cancels the attempt past it | the task's `max_steps` as the agent server's `max_iterations` (`tests/e2e/cmp/reference.py`) |
  | Tokens | the task's `max_tokens`; the harness cancels past it | the task's `max_tokens`; the runner stops the attempt past it, read from the server's own usage |
  | Wall clock | the task's `wall_clock_secs` plus 60 s | the same, enforced by the runner |

- **The model, asked the same way:** the same server, build and flags, with the same request settings.
  - Our side reaches the server through the gateway's relay, whose route sends an output cap of 16,384 tokens and the
    chat-template switch that turns the model's thinking on, and leaves temperature and top_p to the server.
  - The reference reaches the server directly on its internal network, sent the same output cap and switch, with
    sampling and reasoning effort left to the server.
  - Tool calls are native on both sides, without strict mode.
- **Retries, which are part of each product:**
  - our runtime retries a transient model error up to 3 attempts per step, and the route's fallbacks, if any, apply;
  - the reference's model client retries on its own (5 times by default).
  - Both sides' harnesses rerun an attempt once only when the model server or the connection failed, never for the
    agent's own failure:
    - ours after a provider failure (`harness.provider`);
    - the reference's when its own event log classes the error as `transient` (a timeout, a connection error or a
      5xx from the model server) or `rate_limit`.
  - The run records how many retries, fallbacks and reruns each side used.
- **One attempt at a time** on both sides, so neither gets more of the server, and in the same order: each task's
  attempts together, tasks in id order. Our side runs first, then the reference.
- **No warm cache for the second side:** each side runs on a fresh start of the server, with the same image, flags and
  settings. Our side runs on the window's first start, and the server is restarted before the reference's side. So
  both start with an empty prefix cache, and neither gets the other's prompts cached. The restart times are recorded
  below. The server is never started with its development routes (they are unauthenticated), and nothing on the host
  reaches it during the run: no forward, no published port.
- **Toolchain:** the reference's agent server is built on our runtime and Workstation images, so both have the same
  tools installed. The reference uses its default tools without a browser (no task uses one), and neither side gets a
  product-specific prompt.
- **Scoring:** the task's own check, in the same image, with the same protected-path rule. The reference's workspace
  is scored by `tqa-e2e --check-only`.
- **Classes, mapped onto one scheme:**

  | Class | Our side (the harness's classes) | The reference |
  |---|---|---|
  | Completed | `pass` | the check passed |
  | The model's failure | `model.wrong_work`, `model.tampered`, `model.tool_args`, `model.stuck`, `model.unreadable`, `model.budget` (the stuck and unreadable stops read `model.budget` until 7 Oct 2026) | the check failed, a protected path changed, `stuck` (our `model.stuck`), a budget stop, or an error the reference classes as the agent's own (`agent_action`, such as its step limit, or `quota`) |
  | Stopped at the wall clock | `harness.timeout` | `wall_clock` |
  | A provider failure (rerun once) | `harness.provider` | an error classed `transient` or `rate_limit` |
  | Other harness failures | the other `harness.*` | any other error (`internal`, `config`, `auth`, `unknown`, or none recorded), or a broken check |
  | Not started | (none) | `setup`: not counted, and rerun |

  Completion counts each attempt's final run; every harness failure is counted, a rerun's included, so a rerun never
  hides one.
- **Isolation of the reference:** its container is on the model's internal network only, with no route outside it.
  The model key is sent only in the request that starts a conversation, and is removed from everything saved.
- **Tokens** are recorded as a secondary measure, not part of M6.

## Setup
- **Candidate:** the release candidate above, images pinned by ID: gateway `c580e6b4b246`, runtime `47fea18da874`, Workstation `8ccb9b88a291`.
- **Reference:** OpenHands 1.24.0, whose agent server is 1.49.6 (source commit `fcc102a`, MIT), built on the
  candidate's runtime and Workstation images: `8eefa67b9887` and `38f832321861`. Its API was checked
  against its own `/openapi.json`, and one uncounted attempt passed on 28 Sep with no outside network.
- **Model:** a 27B open-weight model (4-bit floating-point weights, FP4) served by an open-source inference server on one
  32 GB GPU; 64k context; four concurrent sequences; native tool calls without strict mode.
- **Server starts (the cache-fairness method):** our side on the start at 20:10:33 (+0800); the server stopped at
  20:51:34, and the reference ran on the restart at 20:51:49, ready at 20:53:56.

## Results

| Side | Completed | 95 % Wilson | Model failures | Harness failures | Stopped at the wall clock | Retries / fallbacks / reruns | Tokens (prompt / completion) |
|---|---|---|---|---|---|---|---|
| TensorQuay Agent | **48/50 = 96.0 %** | 86.5–98.9 % | 0 | 2 (`harness.check`) | 0 | 0 / 0 / 0 | 1,294,879 / 118,849 |
| OpenHands (reference) | **50/50 = 100.0 %** | 92.9–100.0 % | 0 | 0 | 0 | not exposed / – / 0 | 2,856,455 / 125,657 |

Our side ran 20:12:37–20:51:34 (39 min) and the reference 20:53:56–21:35:48 (42 min), one attempt at a time each.

**The difference (ours minus the reference):** −4.0 points, 95 % interval −10.0 to 0.0. The interval comes from a
paired bootstrap over tasks: the ten tasks are resampled with replacement many times, each keeping its two sides
together, and the difference is recomputed each time.

"95 % Wilson" is the range the true completion rate most likely lies in, given the attempts made.

**Per task (completed of 5):**

| Task | TensorQuay Agent | OpenHands |
|---|---|---|
| py-add-tests | 4/5 | 5/5 |
| py-docs-readme-usage | 5/5 | 5/5 |
| py-feature-csv-export | 5/5 | 5/5 |
| py-fix-off-by-one | 4/5 | 5/5 |
| py-rename-refactor | 5/5 | 5/5 |
| sh-build-fix | 5/5 | 5/5 |
| sh-feature-backup-dry-run | 5/5 | 5/5 |
| ts-config-npm-test | 5/5 | 5/5 |
| ts-fix-date-range | 5/5 | 5/5 |
| ts-refactor-extract-validation | 5/5 | 5/5 |

**Our two misses.** Both are `harness.check`: the task's check timed out at its 120 s bound after the agent's run had
ended normally.
- On `py-fix-off-by-one`, the check had already printed that the tests pass, and its container still didn't exit in
  time.
- Run again on the exact workspaces the attempts left, both checks pass in about a second.

So the agent's work was right both times, and the fault is in how our harness runs its check container, not in the
product under test. By the method they still count: only a provider failure is rerun, and a harness failure is never
hidden. Counting the agent's work alone, both sides completed 50/50. The check container's exit is a harness finding,
to be fixed before the next run.

## Limits
- 50 attempts a side. With both near 90 %, the 95 % interval on the difference is about ±12 points. So the result can
  show "within 5 points" only once the interval stays inside it, which needs more attempts or fewer misses; this run's
  interval, −10.0 to 0.0, does not.
- Ten small tasks, one model and one date. A reference release or a model change needs a new run.
- The reference's first start built its image from source with network access. Its counted attempts had no outside
  network.
