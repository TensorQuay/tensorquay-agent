# The agent-teams evaluation (slice 7)

The harness for [docs/evals/agent-teams.md](../../../docs/evals/agent-teams.md) section 2: 20 tasks, each run three
times as a single agent and three times with agent teams on, at the same budget, judged blind.

| File | What it does |
|---|---|
| `generate.py` | writes `tasks/` from `fixtures/` (`--verify` fails when they differ) |
| `fixtures/` | each task's ground truth: seed, check, key facts, a passing and a failing answer, the scripted sessions |
| `stack_dry_run.py` | both arms on the scripted model through `runner.py`, before GPU time |
| `runner.py` | runs the attempts through the public API and keeps the team's record |
| `report.py` | `packets` for the judges, then `report` from their sheets |
| `api.py`, `judging.py`, `stats.py` | the API calls, blind packets and settling, the numbers |

## The dry run first (the scripted model, no GPU)
```sh
TQA_DOCKER_TESTS=1 python3 tests/e2e/teams_eval/stack_dry_run.py [--only <task>] [--keep <dir>] [--logs <dir>]
```
It brings up tests/e2e/dry_run.py's stack, serves each task's `dry-run/single.json` or `dry-run/team.json` before its
attempt, switches the arms by restarting the gateway, and runs every task once per side through `runner.py`. On the
team side the lead starts one helper, takes its report and calls `done`. It passes when every attempt passes its
check with no failure class, every team ended by `done` with its helper, and the packets build.

## Running it (needs the GPU and the owned stack)
```sh
python3 tests/e2e/teams_eval/runner.py --out <run> --base-url http://127.0.0.1:8090 --token-file <token> \
  --route <local route> --storage-root <gateway storage root> --tqa-e2e target/release/tqa-e2e \
  --runtime-image <runtime image> --switch "<command that restarts the gateway with agent teams off|on>"
python3 tests/e2e/teams_eval/report.py packets <run> [<run> ...] <judging>
#   judges fill <judging>/first.csv and second.csv; third.csv for the packets the report lists as waiting
python3 tests/e2e/teams_eval/report.py report <run> [<run> ...] <judging>
```
**Batches.** A night may run its tasks in batches, each with its own `--out` (for example `fri-d`, `fri-s`, `sat-l`
and `sat-d`, a second day's attempts at the same tasks). Give every batch's `--out` to `packets` and `report`, the
same list each time, and make the packets once, after the last batch:
- a batch is named by its directory, so two batches never share a name;
- each batch numbers its attempts from #1, so an attempt is known by its batch with its task, side and number, and
  the side map records the batch;
- a rerun replaces an attempt of its own batch only;
- the report combines every batch's final attempts by task, and says which batch holds which tasks and attempts; its
  reruns and gateway restarts name their batch.
A stopped run goes on with the same command plus `--resume` (the same settings, or it refuses): finished attempts are
skipped, an owed rerun runs, and an attempt cut off without its line runs again, its partial evidence kept as
`<attempt>.interrupted-<n>` and listed in `interrupted.jsonl`; a half-written last line goes to `attempts.partial`.
The full design (20 tasks x 2 arms x 3) takes a night, so a stop and a resume are expected.

**The team side's brief (from night 2, 12 Oct 2026).** The team side's lead reads one paragraph after the task text,
the same for every task (`runner.TEAM_BRIEF`): "You can lead a team on this: start helpers, each taking one part of
the work from your brief and reporting back to you. Use them where splitting the work helps." It is in a person's
words, and it never says which part to hand out. On night 1 (11 Oct, candidate16b, the local 27B) the lead was offered
`start_member` on all 13 team attempts and started no helper, so that night's team arm measured one agent ("the lead
never chose to form a team on its own"). `run.json` records the brief, so a night-1 run doesn't resume under it, and
night 1's results aren't pooled with later nights' team arm.

**One side, and a side left out (night 3, 13 Oct 2026).** `runner.py --side team` runs the team side only (the sides
run are a setting a resume keeps): night 3 runs fri-d's and fri-s's tasks as `fri-d-team` and `fri-s-team` with the
brief. Night 1's team side is then left out of the judging and the numbers, and night 3's team attempts are compared
with night 1's single attempts, task by task (decided on 12 Oct 2026):
```sh
python3 tests/e2e/teams_eval/report.py packets fri-d fri-s sat-l sat-d fri-d-team fri-s-team <judging> \
  --leave-out fri-d:team --leave-out fri-s:team
```
The packets step keeps the choice in `<judging>/left-out.json`; `report` reads it from there and says it.

The switch command gets `single` or `team` appended and returns once the gateway answers. Before each team attempt
the runner sets `agent_teams.token_budget` and `agent_teams.deadline_minutes` to the task's budget and wall clock.
The judges see only `<judging>/packets/`, never `side-map.json` or the run. On each row they also fill `side_guess`
(single, team, or blank when unsure); the report gives each judge's accuracy, so the blindness is measured.

The report also gives each group of the results page (docs/evals/agent-teams.md section 3) on its own: short
(research, report, code, mixed), dependent and long. For each it gives each side's pass rate with its Wilson
interval, its mean score, and its medians per attempt (tokens, minutes, minutes of the attempts that passed). Then
the paired token and wall-time ratios, and two team measures read from each attempt's records: hand-backs that
didn't go in as handed (`take_files` refused, or a file placed under its next free name), the lead's reads that
waited on its helpers (`read_reports` with `wait` above 0 that found nothing new; a read with wait 0 is a poll), and
its reads told to yield (for another task's slot, or to a helper waiting to work on its files). Its limits name the
run's own tasks and attempts.

A collision can be missed, never invented: a file name the upload sanitises or shortens, or one holding ", ", doesn't
match `<helper>-<path>-N` as read from the `take_files` result, so it isn't counted. The count is a lower bound.

## Offline (no stack, a few seconds)
```sh
python3 -m unittest discover -s tests/e2e/teams_eval -p 'test_*.py'
```
It runs every task's check on the seed (fails), the reference answer (passes) and the wrong answer (fails), plus the
runner against a fake API, the judging and the statistics.
