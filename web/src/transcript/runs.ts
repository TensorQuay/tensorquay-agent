import type { EventFrame } from "./steps";
import type { RunStatus, Transcript } from "./types";

/** Retains recorded turn timing and usage without a wall clock or a second event log. */
export function recordRun(
  state: Transcript,
  frame: EventFrame,
  status: RunStatus,
): Transcript {
  if (!("run_id" in frame.event)) return state;
  const id = frame.event.run_id;
  const previous = state.runs[id];
  const user = state.items.findLast((item) => item.kind === "user");
  return {
    ...state,
    run: { id, status },
    items: state.items.map((item) =>
      item === user && item.kind === "user" && !item.runId
        ? { ...item, runId: id }
        : item,
    ),
    runs: {
      ...state.runs,
      [id]: {
        status,
        startedAt: previous?.startedAt ?? frame.ts,
        finishedAt: status === "running" ? null : frame.ts,
        reason: frame.event.type === "run.failed" ? frame.event.reason : null,
        code: frame.event.type === "run.failed" ? frame.event.code : null,
        providerMessage:
          frame.event.type === "run.failed"
            ? (frame.event.provider_message ?? null)
            : null,
        promptTokens: previous?.promptTokens ?? 0,
        completionTokens: previous?.completionTokens ?? 0,
      },
    },
  };
}

/** Adds usage to its run as well as the existing conversation counters. */
export function recordUsage(state: Transcript, frame: EventFrame): Transcript {
  if (frame.event.type !== "usage") return state;
  const event = frame.event;
  const id = frame.run_id ?? state.run.id;
  const run = id ? state.runs[id] : undefined;
  return {
    ...state,
    runs:
      run && id
        ? {
            ...state.runs,
            [id]: {
              ...run,
              promptTokens: run.promptTokens + event.prompt_tokens,
              completionTokens: run.completionTokens + event.completion_tokens,
            },
          }
        : state.runs,
    usage: {
      prompt: state.usage.prompt + event.prompt_tokens,
      completion: state.usage.completion + event.completion_tokens,
      cached: state.usage.cached + (event.cached_tokens ?? 0),
      reasoning: state.usage.reasoning + (event.reasoning_tokens ?? 0),
    },
  };
}
