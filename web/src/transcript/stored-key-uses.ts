import type { TaskKeyUse } from "../protocol/generated/api";

import type { KeyUseView, StepView, Transcript } from "./types";

/**
 * A row of the task's stored key uses (GET /api/v1/conversations/{id}/key-uses, ADR-0037): one key, address and
 * outcome in one run, with when its first and last calls were. `run_id` is null for a refusal made while no run was
 * going (a process left behind calling after its run ended).
 */
export type StoredKeyUse = TaskKeyUse;

/**
 * How far the runtime's clock, which stamps the steps, and the gateway's, which stamps the rows, may disagree: a row
 * goes on a step only when it lies this far inside it.
 */
export const KEY_USE_SKEW_MS = 2_000;

/**
 * The same use of a key: the same sender, address and outcome, and the same name or one left out. A name is judged when
 * it is sent (c740e186): someone removed from a shared key mid-run gets its name in the run's live refusal but not in
 * the stored row, which is the same use.
 */
const same = (a: KeyUseView, b: KeyUseView) =>
  a.sender === b.sender &&
  (a.name === b.name || a.name === null || b.name === null) &&
  a.host === b.host &&
  a.port === b.port &&
  a.outcome === b.outcome;

/**
 * A page opened later reads the task's key uses (ADR-0037): each row goes on the step it lies clearly inside, else once
 * at its run's level (never on a guessed step); a refusal made while no run was going, once at the task's level. A use
 * already said, live or by another row, isn't said again; a row of a run this page doesn't show is left out.
 */
export function placeKeyUses(
  state: Transcript,
  rows: readonly StoredKeyUse[],
  marginMs = KEY_USE_SKEW_MS,
): Transcript {
  // The gateway lists the newest first; a step's lines read in the order they happened.
  const oldest = [...rows].sort(
    (a, b) => Date.parse(a.first_at) - Date.parse(b.first_at),
  );
  let items = state.items;
  let runs = state.runs;
  let taskKeys = state.taskKeys ?? [];
  for (const row of oldest) {
    const use: KeyUseView = {
      sender: row.sender_id,
      name: row.name,
      host: row.host,
      port: row.port,
      outcome: row.outcome,
    };
    if (row.run_id === null) {
      if (!taskKeys.some((seen) => same(seen, use)))
        taskKeys = [...taskKeys, use];
      continue;
    }
    const runId = row.run_id;
    const run = runs[runId];
    const steps = items
      .flatMap((item, index) =>
        item.kind === "step" && item.runId === runId ? [{ item, index }] : [],
      )
      .sort((a, b) => a.item.step - b.item.step);
    if (!run && steps.length === 0) continue;
    const said = [
      ...steps.flatMap(({ item }) => item.keys ?? []),
      ...(run?.keys ?? []),
    ];
    if (said.some((seen) => same(seen, use))) continue;
    const on = stepOf(
      steps.map(({ item }) => item),
      run?.finishedAt ?? null,
      row,
      marginMs,
    );
    const target = on === null ? undefined : steps[on];
    if (target) {
      items = [...items];
      items[target.index] = {
        ...target.item,
        keys: [...(target.item.keys ?? []), use],
      };
    } else if (run)
      runs = { ...runs, [runId]: { ...run, keys: [...(run.keys ?? []), use] } };
  }
  return taskKeys.length > 0 || items !== state.items || runs !== state.runs
    ? { ...state, items, runs, ...(taskKeys.length > 0 ? { taskKeys } : {}) }
    : state;
}

/**
 * The step whose window holds all of a row's calls: from its first attempt's start to the next step's, each narrowed
 * by the margin, the last one's to the run's end widened by it (open while the run goes on). A run of one step takes
 * its every row; a time that can't be read places nothing.
 */
function stepOf(
  steps: readonly StepView[],
  finishedAt: string | null,
  row: StoredKeyUse,
  marginMs: number,
): number | null {
  if (steps.length === 1) return 0;
  const first = Date.parse(row.first_at);
  const last = Date.parse(row.last_at);
  const starts = steps.map((step) =>
    Date.parse(step.firstStartedAt ?? step.startedAt ?? ""),
  );
  const end = finishedAt === null ? Infinity : Date.parse(finishedAt);
  const index = starts.findIndex((start, k) => {
    const next = starts[k + 1];
    const until = next === undefined ? end + marginMs : next - marginMs;
    return start + marginMs <= first && last <= until;
  });
  return index === -1 ? null : index;
}
