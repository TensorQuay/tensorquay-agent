# E2E task set

The task set behind TEST-PLAN §16: small repositories, each with a task for the agent and an automated check that
decides whether the agent did the work. `tqa-e2e` (in `crates/tqa-testkit`) runs the set against a stack through its
public API and classifies every failure as `harness` or `model` from the event log (E2E-WAVE-001, PRD M1).

The harnesses' own unit tests (`test_*.py` here, in `cmp/` and in `live/`) run in every `tools/check.sh` (gates
`DEPLOY-U-e2e`, `DEPLOY-U-e2e-cmp` and `DEPLOY-U-e2e-live`), without Docker or the network; the tasks' seed and hidden
tests under `tasks/` are the agent's, not ours.

## Requirements
- A running stack and an API token for it, stored in a file that only you can read. Tokens are created from a
  signed-in session (FR-26); the driver reads the file and never prints the token.
- **Host access to the stack's storage.** The driver copies each task's seed into a new workspace directory under the
  gateway's storage root (`--storage-root`), through a short-lived container, because the storage belongs to the
  runtime user. Run it on the host where the stack stores its workspaces.
- Docker, and the runtime image the stack uses (`--runtime-image`); the checks run in it without network.

## Running
A dry run against the scripted model costs nothing and proves the harness, the tasks and their checks
(TK-I-001). It starts its own stack on a random port:

```sh
TQA_DOCKER_TESTS=1 python3 tests/e2e/dry_run.py
```

A run against a real route:

```sh
cargo run -p tqa-testkit --bin tqa-e2e -- \
  --base-url http://127.0.0.1:8090 --token-file <token file> --route <route> \
  --tasks tests/e2e/tasks --out tests/e2e/results/<date>/<run> \
  --storage-root <gateway storage root> --runtime-image <image> --gateway-image <image> \
  --allow-paid-route --max-total-tokens <cap> [--native-tools false] [--repeat 3] [--concurrency N] [--yes] \
  [--keep-workspaces] [--cleanup-wait-secs 900]
```

Without `--dry-run`, the driver refuses to start unless the route is allowed to cost money and the whole run has a
token cap, and it prints its spending bound and stops until `--yes` is added. Each task's `max_tokens` and
`max_steps` also cancel a runaway attempt. `--native-tools` records how the route is configured, because the public
API does not report it.

A run cleans up after itself, so its attempts never clutter anyone's task list or Workspaces page (TK-U-026,
TK-U-027):
- once an attempt's evidence is saved (its events and the downloaded copy of its workspace), its task is deleted;
- at the end of the run, each attempt's workspace is deleted. A workspace can't be deleted while its task's sandbox is
  still idling out, so the sweep retries every 15 s for up to `--cleanup-wait-secs` (0 to 3600, default 900, past the
  default 600 s idle window). The summary ends with a Cleanup line, or names each workspace left on the server;
  `attempts.jsonl` records each attempt's `workspace_id`.

Only the run's own workspaces are deleted, by id. `--keep-workspaces` keeps everything, for debugging a run.

`--concurrency N` (1 to 16, default 1) runs up to N attempts at once, each in its own freshly seeded workspace, and
starts them in task order. At most 2 seed and check containers run at once, whatever N is, because concurrent Docker
work under load broke checks. An attempt is numbered when it starts, and a rerun goes first in the queue, so at 1 the
numbering is the one-at-a-time driver's. With N above 1, a harness failure stops new starts (the running attempts
finish, and the summary names the attempt that stopped them); the token cap likewise stops new starts once the
attempts finished so far reach it. The summary lists attempts in number order (TK-U-024, TK-I-006).
`concurrency_run.py` (TK-I-006) runs one task's scripted solution four times at concurrency 1 and 4 on an owned
stack with the scripted model, and compares the runs. Its copy of the task has a check that sleeps first, so the
four-at-a-time run must queue on the limit of two harness containers; run it on its own, like the dry run.

### Comparing a decision engine (E2E-DECISION-008)
`--engine-profile <profile id>` compares the model deciding alone with a decision engine (ADR-0013 point 9). The
profile must name the engine. Each task's rounds then run once per arm, interleaved per task and round: the control is
made without a profile and the engine arm from it. The pair's order alternates per round (ABBA: control then engine,
then engine then control), so neither arm always runs second on a cache the other warmed. A rerun keeps its arm and
place.
- `--max-engine-calls N` is required with it. Once the run's engine calls reach N, no attempt starts, in either arm.
  The engine's own budget in the gateway stays the hard limit.
- `--max-wall-minutes M` (1 to 1440, with or without a profile): no attempt starts after M minutes, so a stuck arm
  can't use up a GPU window.
- With a paid route, the token estimate counts both arms.

Each line of `attempts.jsonl` records its `arm` (`model` or `engine`), its `round`, its `pair_position` (1 or 2) and `decisions`, counted from the event log:
decisions by kind, fallbacks by reason, `engine_calls` (every consultation that reached the engine, so not
`circuit_open` or `budget_spent`), and `engine_latency_ms`. These tests are pure (TK-U-032, TK-U-033, TK-U-034); a
comparison run needs a stack, a GPU window and the lead's go.

`decision_compare.py RUN_DIR [--json] [--out FILE]` reads a comparison run and reports it (it never starts one). It
pairs the arms by task and round, keeps the last attempt of each rerun chain, and drops a pair with a harness failure
or one arm only, counting why. It reports:
- completion per arm with 95 % Wilson intervals, and an exact McNemar test on the discordant pairs;
- tokens, steps and wall time as paired differences (engine minus model) with seeded bootstrap 95 % intervals;
- the order effect: the difference when the engine ran first against when the control did, and each metric at place
  1 and 2;
- the engine's calls, its latency's share of wall time, and its decisions and fallbacks;
- the routes, from the run's `events/`: how often the engine chose each before a step, and the tokens each spent;
- the verdict by ADR-0013's rule: a saving only where its interval lies wholly below zero, a regression (whatever the
  saving) where the paired completion difference's interval does. The plan: `docs/evals/decision-engine.md`.

Below 10 pairs it says it makes no claim. A run written before `round` existed is paired in each task's schedule
order. Tests: `python3 -m unittest tests/e2e/test_decision_compare.py`.

Results go to the `--out` directory, which must not exist yet: `run.json` (the environment), `attempts.jsonl` (one
line per attempt), `events/` (every event log), `work/` (the checked workspace copies and check output) and
`summary.md` (completion with a 95 % Wilson interval, harness failures, reruns and harness expectations).
`tests/e2e/results/` is ignored by git.

`dry_run.py --logs DIR` saves the redacted gateway, fake-model and sandbox logs to `DIR` before teardown, for
diagnosing one run (the compaction demo's summary calls, for example).

### A check on another tool's workspace (E2E-CMP-003)
`--check-only` runs one task's check on a workspace another tool left, the way an attempt's check runs, so a
comparison scores both sides with the same check:

```sh
cargo run -p tqa-testkit --bin tqa-e2e -- --check-only --tasks tests/e2e/tasks --task <id> \
  --workspace <dir> --out <record dir> --runtime-image <image> [--workstation-image <image>]
```

The workspace is another program's output: it is copied into `<record dir>/work` and never changed, and the copy holds
only directories and regular files (symbolic links and anything else are dropped and counted in `check.json`, as an
attempt's download never makes a link). A changed protected path is `tampered`; otherwise the seed's protected paths are restored and the check runs.
`check.txt` holds the check's output (a broken check ends with its harness line) and `check.json` the verdict.

`--seed-only` gives the other tool the same starting point: it seeds an empty workspace with the task's seed, as one
commit with the fixed author and date an attempt's baseline has, by the driver's own seeding, and hands it back to the
host user:

```sh
cargo run -p tqa-testkit --bin tqa-e2e -- --seed-only --tasks tests/e2e/tasks --task <id> \
  --workspace <empty dir> --runtime-image <image>
```

### A wave (E2E-WAVE-002)
A wave runs several attempts at once, so the stack must admit that many sandboxes for one user. `wave_profile.py`
reads the gateway's two runtime-mutable admission limits (`admission.slots` and `admission.per_user`, ADR-0028) with
an administrator's token, raises what is short of N, and records the values it found in the run directory so
`restore` puts back exactly those:

```sh
python3 tests/e2e/wave_profile.py check   --base-url http://127.0.0.1:8090 --token-file <token file> --concurrency 8
python3 tests/e2e/wave_profile.py apply   --base-url http://127.0.0.1:8090 --token-file <token file> --concurrency 8 \
  --record tests/e2e/results/<date>/<run>-profile
python3 tests/e2e/wave_profile.py restore --base-url http://127.0.0.1:8090 --token-file <token file> \
  --record tests/e2e/results/<date>/<run>-profile
```

Both limits are at most the configuration file's `sandbox.max_live`, which only a restart changes; `check` and `apply`
say so and change nothing when it is below N. The model server's own concurrency (its sequence limit) is set where the
server starts, and belongs in the run's notes with the server's flags. Self-test without a stack:
`python3 -m unittest tests/e2e/test_wave_profile.py`.

## A task
```
tests/e2e/tasks/<id>/
  task.toml    id, title, tags, budgets, protected paths, check image and timeout, expects_compaction,
               and optionally the environment template the conversation runs in (for example "workstation")
  task.md      the message the agent receives; nothing else in this directory reaches the agent
  seed/        the repository as the agent finds it; committed as one baseline commit before the run
  check/       run.sh and anything it needs (held-out tests, mutants, expected output); never shown to the agent
  dry-run/     solve.json and wrong.json: scripted model sessions that solve the task and fail it
```
The check runs on a copy of the workspace downloaded through the files API, never in the agent's sandbox. Paths in
`protected` are compared with the seed first (a change is `model.tampered`) and restored before the check.
`run.sh` exits 0 for pass and 1 for fail; any other exit, a timeout or more than 64 KiB of output means the check
itself is broken (`harness.check`). It prints one JSON line, `{"passed": ..., "reason": ...}`. The files API does not
keep execute bits, so a check calls `sh` or `python3` explicitly. A check must be deterministic: no network, no clock,
no order assumptions.

A task with `template = "workstation"` runs in the Workstation environment; one with `check.image = "workstation"` is
checked in the Workstation image, which the run supplies with `--workstation-image` (the driver refuses to start
without it). TypeScript tasks use Node's own runner on Workstation, with no compiler: erasable syntax only (no
`enum` or `namespace`), explicit `.ts` extensions in imports, explicit test globs such as `node --test
test/*.test.ts`, and a check that accepts both the spec reporter (the default) and TAP when it reads the summary.
There is no Go toolchain in either image yet, so there are no Go tasks.

The set keeps its gate requirements, checked by a unit test: at least five `fallback-relevant` tasks
(E2E-FALLBACK-004), exactly ten tagged `cmp` (E2E-CMP-003), at least two long tasks with `expects_compaction`, and in
each language a fix, a feature, tests, a refactor and docs or configuration. The long tasks are sized to need
compaction on a 32k window; on a model with a larger window, run them on a route whose context limit is 32k.

Before a task joins the set: the seed alone must fail its check (`tqa-e2e --seed-check`), `solve.json` must pass and
`wrong.json` must be classified `model.wrong_work` (TK-I-001). A check that the seed passes proves nothing. A task
whose seed is produced by a script keeps that script next to `task.toml` (for example `generate.py`), outside `seed/`.

## The Gate 4 demo (E2E-TEAM-011)
`gate4.py` runs the DEV-PLAN Phase 4 demo as one scripted scenario over the public API:

1. A invites B, and each stores their own credential for a local authenticated Git origin (the RT-I-016 fixture from
   `crates/tqa-sandbox/tests/egress`, imported unchanged).
2. A shares a workspace with B, with write access.
3. A's run holds the workspace lease through its held push step (the scripted model's `wait_ms`; ADR-0035 left no
   approval to wait at), and B's run on the shared workspace queues behind it.
4. B forks the workspace and runs in the fork in parallel, pushing `demo/b`.
5. A's held step goes on and A pushes `demo/a`, and B's queued run then starts.

```sh
TQA_DOCKER_TESTS=1 python3 tests/e2e/gate4.py
python3 -m unittest discover -s tests/e2e -p 'test_*.py' -v
```

It uses the local `tqa-gateway:local`, `tqa-fake-model:local` and `tqa-runtime:workstation` images (the runtime
needs Git), and the Docker backend, because fork is unavailable on microVMs. The fake-model image must support
`"select": "arrival"`; set `TQA_FAKE_MODEL_IMAGE` to use another tag. It finishes within ten minutes.

The scenario sets `"select": "arrival"`, so the fake model answers across all conversations in arrival order. The
orchestrator starts each phase only after the previous one reaches a known point, so no two runs ask the model at the
same time.

Evidence goes to `results/gate4-<utc>/`:
- `events-{holder,waiting,fork}.jsonl`;
- `transcript.md`;
- `result.json`, with the run intervals, push outcomes, origin refs and the orchestrator's observations.

No token, password or credential is recorded; the script checks this before writing.

## An MCP task (E2E-MCP-006)
`mcp_task.py` runs one task that needs an MCP-provided tool, on the real stack. The install's catalogue
(`[[mcp.servers]]` in gateway.toml) defines one stdio server, `words`: the workstation image's own `python3` running a
tiny MCP server passed inline as an argument, so no image changes. The conversation starts with it (enabled by default),
the gateway delivers it in `session.config`, and the runtime starts it inside the sandbox. The fake model calls
`mcp__words__count_words`. The check is that the journal records `mcp.words.count_words` with the result `words: 4`,
the run completes, and an `mcp.server` event reports the server ready with one tool before the run starts.

```sh
TQA_DOCKER_TESTS=1 python3 tests/e2e/mcp_task.py
```

It needs images built from 17.0 or later (the gateway's catalogue delivery and the runtime's MCP client), with
`TQA_RUNTIME_TEST_IMAGE` naming the workstation runtime image. It finishes within five minutes.

## A profile task (E2E-PROFILE-007)
`profile_task.py` runs one task under a restricted profile on the real stack (ADR-0012). The install enables two MCP
sources by default: E2E-MCP-006's catalogue server `words`, and a connector `tracker`. The connector is a small MCP
server on the standard library, run from the workstation runtime image as an extra Compose service on the gateway's
control network. It starts before the gateway, which names it by its address in gateway.toml: the proxy refuses a name
that resolves to a private address (SEC-024), but not an address the file names itself.

The owner makes a personal profile that allows `read_file` and `tracker` only, then a conversation from it. The fake
model calls `shell`, then `mcp__tracker__lookup`, then answers. The checks:
- the conversation has the profile's servers and connectors (none, and `tracker`);
- only `tracker` starts (`mcp.server`);
- `shell` is answered as an unknown tool naming only `mcp.tracker.lookup` and `read_file`;
- the connector's call succeeds, and the run completes.

```sh
TQA_DOCKER_TESTS=1 python3 tests/e2e/profile_task.py
python3 -m unittest discover -s tests/e2e -p 'test_profile_task.py' -v
```

It needs gateway and workstation runtime images at protocol revision 8 or later (the profile snapshot), with
`TQA_RUNTIME_TEST_IMAGE` naming the workstation runtime image. It finishes within five minutes.
