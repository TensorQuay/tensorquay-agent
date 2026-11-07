import type { ServerFrame } from "../protocol/generated/protocol";

import type { StepView, ToolView, Transcript } from "./types";

export type EventFrame = Extract<ServerFrame, { type: "event" }>;

export function updateStep(
  state: Transcript,
  frame: EventFrame,
  update: (step: StepView) => StepView,
): Transcript {
  const runId = frame.run_id ?? state.run.id ?? "unassigned";
  const previous = state.items.findLast(
    (item) => item.kind === "step" && item.runId === runId,
  );
  const step =
    "step" in frame.event
      ? frame.event.step
      : (frame.step ?? (previous?.kind === "step" ? previous.step : 0));
  const id = JSON.stringify([runId, step]);
  const index = state.items.findIndex(
    (item) => item.id === id && item.kind === "step",
  );
  const existing = state.items[index];
  const current =
    existing?.kind === "step" ? existing : blankStep(id, runId, step);
  const updated = update(current);
  const items = [...state.items];
  if (index < 0) items.push(updated);
  else items[index] = updated;
  return { ...state, items };
}

export function blankStep(
  id: string,
  runId: string,
  step: number,
  attempt = 1,
): StepView {
  return {
    kind: "step",
    id,
    runId,
    step,
    attempt,
    text: "",
    reasoning: "",
    tools: {},
    toolOrder: [],
    durationMs: null,
    retries: [],
    waitingSince: null,
    startedAt: null,
  };
}

export function updateTool(
  state: Transcript,
  frame: EventFrame,
  callId: string,
  update: (tool: ToolView) => ToolView,
): Transcript {
  return updateStep(state, frame, (step) => {
    const tool = step.tools[callId];
    if (!tool) return step;
    return { ...step, tools: { ...step.tools, [callId]: update(tool) } };
  });
}

export function duration(start: string | null, end: string): number | null {
  if (!start) return null;
  const elapsed = Date.parse(end) - Date.parse(start);
  return Number.isFinite(elapsed) ? Math.max(0, elapsed) : null;
}
