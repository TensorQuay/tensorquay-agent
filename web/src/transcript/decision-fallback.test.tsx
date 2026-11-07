import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import en from "../i18n/locales/en.json";
import { english } from "../i18n/translate";
import type { DecisionFallbackReason } from "../protocol/generated/protocol";
import protocolSchema from "../protocol/generated/protocol.schema.json";
import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const reasons = protocolSchema.definitions.DecisionFallbackReason.oneOf.map(
  (entry) => entry.const as DecisionFallbackReason,
);
const fallback = (seq: number, reason: DecisionFallbackReason) =>
  frame(seq, {
    type: "decision.fallback",
    engine: "house-rules",
    point: "before_step",
    reason,
  });
const said = (transcript: ReturnType<typeof emptyTranscript>) =>
  render(<TranscriptView transcript={transcript} />).container.querySelectorAll(
    ".transcript-notice",
  );

// A task's decision engine that stops answering never fails the run: the agent goes on deciding by itself
// (ADR-0013). The person hears it once per run, in plain words, with why and never a code (the review of 7c39d20b).
describe("WEB-U-093 the decision engine falls back", () => {
  it("says so once per run, with why", () => {
    const transcript = [
      frame(1, { type: "message.user", text: "Do it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      fallback(3, "unreachable"),
      fallback(4, "timeout"),
      fallback(5, "circuit_open"),
    ].reduce(apply, emptyTranscript());
    const notices = said(transcript);
    expect([...notices].map((notice) => notice.textContent)).toEqual([
      "The decision engine couldn't be reached, so the agent is deciding on its own for this run.",
    ]);
  });

  it("says so again in the next run", () => {
    const transcript = [
      frame(1, { type: "message.user", text: "Do it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      fallback(3, "unreachable"),
      frame(4, { type: "run.completed", run_id: runId }),
      frame(5, { type: "message.user", text: "Again", attachments: [] }),
      {
        ...frame(6, { type: "run.started", run_id: "run-second" }),
        run_id: "run-second",
      },
      { ...fallback(7, "budget_spent"), run_id: "run-second" },
    ].reduce(apply, emptyTranscript());
    const notices = [...said(transcript)].map((notice) => notice.textContent);
    expect(notices).toEqual([
      "The decision engine couldn't be reached, so the agent is deciding on its own for this run.",
      "The decision engine's budget is spent, so the agent is deciding on its own for this run.",
    ]);
  });

  it.each(reasons)("words the reason %s, never as a code", (reason) => {
    const text = english("notice.decisionFallback", { reason });
    expect(text).toMatch(
      /^The decision engine.*, so the agent is deciding on its own for this run\.$/,
    );
    if (reason.includes("_")) expect(text).not.toContain(reason);
    // Each listed reason has its own words, not the fallback for a reason this page doesn't know.
    expect(text).not.toBe(
      english("notice.decisionFallback", { reason: "brand_new" }),
    );
  });

  it("has words for a reason newer than this page", () => {
    expect(english("notice.decisionFallback", { reason: "brand_new" })).toBe(
      "The decision engine couldn't be used, so the agent is deciding on its own for this run.",
    );
    expect(
      (en.messages as Record<string, string>)["notice.decisionFallback"],
    ).toBeTruthy();
  });
});
