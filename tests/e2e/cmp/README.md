# E2E-CMP-003: the comparison with the open reference product

PRD M6 asks for task completion within 5 points of OpenHands, the open reference product, on a shared subset of the
task set with the same model. This directory runs the reference side. Our side is an ordinary `tqa-e2e` run of the
same tasks (`--only` the tasks tagged `cmp`) on the same route.

## Method: everything is the same except the product
- **Tasks:** the ten tasks tagged `cmp` in `tests/e2e/tasks`. The statement is `task.md` verbatim, with no hints and
  no product-specific prompt.
- **Starting point:** each attempt's workspace is seeded by `tqa-e2e --seed-only`, the driver's own seeding, so both
  sides start from the same baseline commit.
- **Limits:** each task's own step limit (`max_steps`, sent as the reference's iteration limit) and wall clock
  (`wall_clock_secs`, plus the same 60 s margin the driver allows).
- **Model:** the same server, build, flags, context length and sampling, reached directly on the model's internal
  network (`tqa-a-model`). Our side reaches it through the gateway's relay.
- **Toolchain:** the reference's agent server runs on our runtime image for the default template and on our
  Workstation image for Workstation tasks. It is built once in the setup step, so both products have the same tools
  installed.
- **Scoring:** the task's own check, run by `tqa-e2e --check-only` on the workspace the reference leaves. It is the
  same check, image and protected-path rule as our attempts.
- **One attempt at a time** on both sides, so neither gets more of the server, and in the same order: tqa-e2e's,
  each task's attempts together, tasks in id order.

## Isolation
- The reference container is on the internal model network only. It can reach the model server and nothing else: no
  internet, so no telemetry, and no other container.
- Our own forwarder, built from a local image with Python, publishes its API on `127.0.0.1` for the runner.
- Each attempt gets a random session key. The model key comes from a mode-0600 file, is sent only in the request that
  starts the conversation, and is removed from everything saved.
- The workspace the reference leaves is another program's output. `--check-only` copies only its directories and
  regular files, and never follows a link.

## Setup (once per reference version, with network access, logged)
1. Fetch the agent server's source at the version the reference release pins (OpenHands 1.24.0 pins agent server
   1.49.6), and record the commit and the licence (MIT).
2. Build its agent server on our runtime and Workstation images with its own image builder. The builder resolves the
   base image by name, and its base stage installs packages as root, while our images run as 1000:1000. So each base
   is our image ID tagged locally, plus a two-line image, `FROM` that tag and `USER root`, and nothing else. No extra
   capabilities are installed (no editor, browser or Docker). Pin both results by image ID.
3. Start each image once with no network, and save its `/openapi.json` and server info. `reference.py`'s shapes are
   those of that description.
4. Run one uncounted attempt, and check that the reference container has no route outside the internal network.

The built images run as their own user (uid 10001). The runner starts them as `0:0`, which under rootless Docker is
the host user who owns the seeded workspace, so the reference can write it.

## Running

```sh
python3 tests/e2e/cmp/runner.py --out tests/e2e/results/<date>/cmp-reference \
  --tqa-e2e <path to tqa-e2e> --runtime-image <id> --workstation-image <id> \
  --reference-image <digest> --workstation-reference-image <id> --forward-image <id> \
  --model-url http://local-model:8000/v1 --model <served name> --model-key-file <0600 file> [--repeat 5]
```

It writes `run.json` (the flags and images), `attempts.jsonl` (one line per attempt: class, check outcome, the
reference's final status, wall time and the tokens it reports), `attempts/<task>-<n>/` (the conversation, the
workspace and the check record) and `summary.md`.

**Classes:**
- `completed`: the check passed.
- `model`: wrong or incomplete work, or the reference gave up as stuck.
- `harness`: the reference reported an error, or the check broke.
- `wall_clock`: stopped at the task's wall clock.
- `setup`: the attempt never started. It isn't counted, and is rerun.

The report compares both sides per task, with Wilson intervals and a paired bootstrap interval on the difference.

Self-test without Docker or a model: `python3 -m unittest tests/e2e/cmp/test_cmp.py`.
