import type { ContextReport, Transcript } from "../transcript/types";

export type { ContextReport };

/** What the Context tab shows (design-context-panel): the latest run's report, or why there is none. */
export type ContextState =
  | { kind: "none" }
  | { kind: "loading" }
  | { kind: "nothing"; startedAt: string }
  | { kind: "report"; startedAt: string; report: ContextReport };

const ENDED = new Set(["completed", "cancelled", "failed"]);

const loadedSomething = (report: ContextReport | undefined) =>
  !!report &&
  report.instructions.length + report.skills.length + report.skipped.length > 0;

/**
 * The latest run's report. The runtime sends one straight after the run starts, and none when the run loaded
 * nothing, so a run with a step or an end but no report loaded nothing. Until the report arrives, the previous run's
 * report stays in view.
 */
export function contextState(transcript: Transcript): ContextState {
  const id = transcript.run.id;
  const run = id === null ? undefined : transcript.runs[id];
  if (id === null || !run) return { kind: "none" };
  const report = transcript.context[id];
  if (loadedSomething(report) && report)
    return { kind: "report", startedAt: run.startedAt, report };
  const settled =
    report !== undefined ||
    ENDED.has(run.status) ||
    transcript.items.some((item) => item.kind === "step" && item.runId === id);
  if (settled) return { kind: "nothing", startedAt: run.startedAt };
  const previous = Object.keys(transcript.runs)
    .filter(
      (other) => other !== id && loadedSomething(transcript.context[other]),
    )
    .at(-1);
  const earlier =
    previous === undefined ? undefined : transcript.context[previous];
  return previous !== undefined && earlier
    ? {
        kind: "report",
        startedAt: transcript.runs[previous]?.startedAt ?? run.startedAt,
        report: earlier,
      }
    : { kind: "loading" };
}
