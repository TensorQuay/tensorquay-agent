import { describe, expect, it } from "vitest";

import type { Event, ServerFrame } from "../protocol/generated/protocol";
import { contextLoaded, laterContext } from "../test/context";
import { frame, ready } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";

import { contextState } from "./context-state";

const at = (seq: number, run: string, event: Event): ServerFrame => ({
  ...frame(seq, event),
  run_id: run,
});
const replay = (frames: ServerFrame[]) =>
  contextState(frames.reduce(apply, emptyTranscript()));
const report = {
  instructions: contextLoaded.instructions,
  skills: contextLoaded.skills,
  skipped: contextLoaded.skipped,
};
const run1 = [
  ready,
  at(1, "run-1", { type: "run.started", run_id: "run-1" }),
  at(2, "run-1", contextLoaded),
];
const startedAt = (seq: number) => frame(seq, laterContext).ts;

// The panel shows the latest run's report, and says plainly when there is none (WEB-U-046, FR-11).
describe("WEB-U-046 the context panel's state", () => {
  it("has nothing to show before any run", () => {
    expect(replay([ready])).toEqual({ kind: "none" });
  });

  it("shows the latest run's report with the time that run started", () => {
    expect(replay(run1)).toEqual({
      kind: "report",
      startedAt: startedAt(1),
      report,
    });
  });

  it("keeps the previous report while a new run starts, then shows the new one", () => {
    const next = [
      ...run1,
      at(3, "run-1", { type: "run.completed", run_id: "run-1" }),
      at(4, "run-2", { type: "run.started", run_id: "run-2" }),
    ];
    expect(replay(next)).toEqual({
      kind: "report",
      startedAt: startedAt(1),
      report,
    });
    expect(replay([...next, at(5, "run-2", laterContext)])).toMatchObject({
      kind: "report",
      startedAt: startedAt(4),
      report: { skills: [] },
    });
  });

  it("is loading when the first run has started without a report yet", () => {
    expect(replay(run1.slice(0, 2))).toEqual({ kind: "loading" });
  });

  it("says the run loaded nothing once it has a step, or has ended, without a report", () => {
    const started = run1.slice(0, 2);
    const stepped = [
      ...started,
      at(2, "run-1", { type: "step.started", step: 1 }),
    ];
    expect(replay(stepped)).toEqual({
      kind: "nothing",
      startedAt: startedAt(1),
    });
    const ended = [
      ...started,
      at(2, "run-1", { type: "run.completed", run_id: "run-1" }),
    ];
    expect(replay(ended)).toEqual({ kind: "nothing", startedAt: startedAt(1) });
  });

  it("treats a report with every list empty as nothing loaded", () => {
    const empty = { ...laterContext, instructions: [] };
    expect(replay([...run1.slice(0, 2), at(2, "run-1", empty)])).toEqual({
      kind: "nothing",
      startedAt: startedAt(1),
    });
  });
});
