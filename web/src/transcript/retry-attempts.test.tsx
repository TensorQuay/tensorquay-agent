import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { ServerFrame } from "../protocol/generated/protocol";

import { present } from "./present";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

type EventFrame = Extract<ServerFrame, { type: "event" }>;
const stored = (
  seq: number,
  ts: string,
  step: number | null,
  runId: string | null,
  event: EventFrame["event"],
): EventFrame => ({ type: "event", seq, ts, step, run_id: runId, event });

// The 15b integration's WEB-E2E-052, as the gateway stored it (a kept stack, 10 Oct 2026): a route whose model server
// answers 503 every time. The runner retries twice, and each retry is followed by the same step's next attempt; then
// the run fails. Every frame here is that run's own, in order.
const run: EventFrame[] = [
  stored(1, "2026-10-10T10:42:09.935660744Z", null, null, {
    type: "environment.changed",
    template: "workstation",
    capabilities: ["shell", "python", "node", "build", "browser", "data"],
  }),
  stored(2, "2026-10-10T10:42:09.941690483Z", null, null, {
    type: "message.user",
    text: "Answer in one line.",
    attachments: [],
  }),
  stored(3, "2026-10-10T10:42:09.941747485Z", null, "r-2", {
    type: "run.started",
    run_id: "r-2",
  }),
  stored(4, "2026-10-10T10:42:09.94177084Z", null, "r-2", {
    type: "context.loaded",
    skills: [],
    skipped: [],
    instructions: [],
  }),
  stored(5, "2026-10-10T10:42:09.941916473Z", 1, "r-2", {
    type: "step.started",
    step: 1,
    attempt: 1,
  }),
  stored(6, "2026-10-10T10:42:09.947817208Z", 1, "r-2", {
    type: "provider.retry",
    reason: "http 503",
    attempt: 1,
    delay_ms: 1019,
  }),
  stored(7, "2026-10-10T10:42:10.968521682Z", 1, "r-2", {
    type: "step.started",
    step: 1,
    attempt: 2,
  }),
  stored(8, "2026-10-10T10:42:10.989606288Z", 1, "r-2", {
    type: "provider.retry",
    reason: "http 503",
    attempt: 2,
    delay_ms: 3230,
  }),
  stored(9, "2026-10-10T10:42:14.221336075Z", 1, "r-2", {
    type: "step.started",
    step: 1,
    attempt: 3,
  }),
  stored(10, "2026-10-10T10:42:14.234243978Z", 1, "r-2", {
    type: "run.failed",
    run_id: "r-2",
    code: "provider_error",
    reason: "http 503",
  }),
];

// WEB-U-016: a retry belongs to its step and is said there, folded once the run is done. A step's next attempt starts
// its output over, but the retries are why it started over: they stay with the step. (Before the fix, `step.started`
// with a higher attempt replaced the step with a blank one, retries included, so a run whose every attempt failed, or
// that recovered, showed no retry at all on the real stack.)
describe("WEB-U-016 a step's retries survive its next attempt", () => {
  it("keeps both retries of the run that failed after them", () => {
    const transcript = run.reduce(apply, emptyTranscript());
    const [turn] = present(transcript);
    expect(turn?.retries.map((retry) => retry.reason)).toEqual([
      "http 503",
      "http 503",
    ]);
  });

  it("says them on the page, the code only after Details, as WEB-E2E-052 reads it", () => {
    const { container } = render(
      <TranscriptView transcript={run.reduce(apply, emptyTranscript())} />,
    );
    const titled = container.querySelector("[title*='Details:']");
    expect(titled?.getAttribute("title")).toMatch(
      /^The model server answered with an error\. Details: http 503/,
    );
  });
});
