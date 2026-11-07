import type { CaptureView, ToolView, Transcript } from "../transcript/types";

/** One desktop action for the replay: its run, its line in words, its pictures and whether they were kept. */
export type ReplayStep = {
  id: string;
  runId: string;
  label: string;
  captures: readonly CaptureView[];
  evidence?: ToolView["evidence"];
};

/**
 * The replay's steps: the task's desktop actions only, in order, each with its run, its line in words and its pictures
 * ("Step 12 of 38" counts these; shell and other tools are skipped).
 */
export function desktopSteps(
  transcript: Transcript,
  label: (tool: ToolView) => string,
): ReplayStep[] {
  return transcript.items.flatMap((item) =>
    item.kind === "step"
      ? item.toolOrder.flatMap((id) => {
          const tool = item.tools[id];
          return tool?.name === "computer"
            ? [
                {
                  id,
                  runId: item.runId,
                  label: label(tool),
                  captures: tool.captures ?? [],
                  ...(tool.evidence ? { evidence: tool.evidence } : {}),
                },
              ]
            : [];
        })
      : [],
  );
}

/** A step's one picture: after it, else what the model saw (the step before's after is its before). */
export function stepPicture(step: ReplayStep): CaptureView | undefined {
  return (
    step.captures.find((each) => each.phase === "after") ??
    step.captures.find((each) => each.phase === "screenshot")
  );
}
