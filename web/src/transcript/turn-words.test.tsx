import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";
import { turnSummary } from "./turn-summary";

// A screen review before 17.0: a turn's Thinking read as an empty label (its summary had lost the disclosure's
// marker), and a quick step read "Worked for 0 s".
describe("WEB-U-157 a turn's words", () => {
  it("marks Thinking as something to open", () => {
    const transcript = [
      frame(1, { type: "message.user", text: "Check it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "step.started", step: 1, attempt: 1 }),
      frame(4, { type: "reasoning.delta", text: "Look at the files first." }),
      frame(5, { type: "text.delta", text: "Checked." }),
      frame(6, { type: "step.completed", step: 1, duration_ms: 400 }),
      frame(7, { type: "run.completed", run_id: runId }),
    ].reduce(apply, emptyTranscript());
    const { container } = render(<TranscriptView transcript={transcript} />);
    const summary = screen.getByText(/^Thought for /).closest("summary");
    expect(summary?.parentElement?.tagName).toBe("DETAILS");
    expect(summary?.querySelector("svg")).not.toBeNull();
    expect(container).toHaveTextContent("Look at the files first.");
  });

  it("says a turn under a second took under a second", () => {
    const summary = turnSummary(
      {
        status: "completed",
        startedAt: "2026-01-01T00:00:00Z",
        finishedAt: "2026-01-01T00:00:00.400Z",
        reason: null,
        code: null,
        providerMessage: null,
        promptTokens: 10,
        completionTokens: 5,
      },
      [],
      [],
    );
    expect(summary.activityLabel).toBe("Worked for under a second");
    expect(summary.activityLabel).not.toMatch(/\b0 s\b/);
  });
});
