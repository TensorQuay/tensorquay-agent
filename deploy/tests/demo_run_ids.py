"""E2E-AUDIT-001: a real run's model calls reach the relay with their run (X-TQA-Run-ID), so the usage meter's rows and
the route key's read-audit row name that run. The basic suite reads them while the stack is still up.

How a row whose run is missing is still found: the relay learns the task from the runtime token, never from the
header, so it records the task (conversation_id) on every usage and read row, run or not. A row with a null run is
therefore one of the task's rows by its conversation_id. As a backstop, a route read with no task at all whose times
fall inside the task's event window (first to last event) counts against the task too."""

import json
from pathlib import Path
import uuid

import compose

ROUTE, REF = "local", "secrets/local"

QUERY = """
WITH task AS (SELECT min(ts) AS first_at, max(ts) AS last_at FROM events WHERE conversation_id = '{task}')
SELECT json_build_object(
  'runs', (SELECT coalesce(json_agg(DISTINCT run_id), '[]') FROM events
           WHERE conversation_id = '{task}' AND run_id IS NOT NULL),
  'usage', (SELECT coalesce(json_agg(json_build_object('run', run_id, 'route', route) ORDER BY created_at), '[]')
            FROM usage WHERE conversation_id = '{task}'),
  'reads', (SELECT coalesce(json_agg(json_build_object('ref', ref, 'kind', consumer_kind, 'consumer', consumer,
                                                       'run', run_id, 'outcome', outcome, 'count', count)
                                     ORDER BY first_at), '[]')
            FROM secret_reads WHERE conversation_id = '{task}'),
  'unowned', (SELECT count(*) FROM secret_reads AS r, task
              WHERE r.conversation_id IS NULL AND r.consumer_kind = 'route' AND r.consumer = '{route}'
                AND r.last_at >= task.first_at AND r.first_at <= task.last_at))
"""


def record(configuration, env):
    """Reads the task's runs, usage rows and read rows while the stack is up; prints them for the log."""
    task = str(uuid.UUID(json.loads(Path(env["TQA_STACK_RESULT"]).read_text())["conversationId"]))
    output = compose.command(configuration + ["exec", "-T", "postgres", "psql", "-v", "ON_ERROR_STOP=1", "-At",
                                              "-U", "tqa", "-d", "gateway", "-c",
                                              QUERY.format(task=task, route=ROUTE)],
                             env, capture=True).stdout.strip()
    print("Run attribution rows: " + output, flush=True)
    env["TQA_RUN_ROWS"] = output


def verify(env):
    rows = json.loads(env["TQA_RUN_ROWS"])
    runs, usage, reads = set(rows["runs"]), rows["usage"], rows["reads"]
    if not runs:
        raise AssertionError("The task has no run in its events, so nothing can be attributed")
    if not usage:
        raise AssertionError("The task has no usage row, so the check would prove nothing")
    unattributed = [row for row in usage if row["run"] not in runs]
    if unattributed:
        raise AssertionError("Usage rows without one of the task's runs {}: {}".format(sorted(runs), unattributed))
    route = [row for row in reads if row["kind"] == "route" and row["consumer"] == ROUTE]
    if not any(row["ref"] == REF and row["outcome"] == "ok" and row["run"] in runs for row in route):
        raise AssertionError("No ok read of {} by route {} names a run of the task: {}".format(REF, ROUTE, reads))
    unknown = [row for row in route if row["run"] not in runs]
    if unknown or rows["unowned"]:
        raise AssertionError("Route reads without the task's run: {} (and {} with no task in its window)".format(
            unknown, rows["unowned"]))
    print("Run attribution: {} usage rows and the route key's read row all name the task's run ({})".format(
        len(usage), ", ".join(sorted(runs))), flush=True)
