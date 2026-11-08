import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, applyAt, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const start = [
  frame(1, { type: "message.user", text: "Plan the report", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
];
/** The time of frame `seq` (the helper's clock), plus `seconds`. */
const after = (seq: number, seconds: number) =>
  Date.UTC(2026, 0, 1) + seq * 100 + seconds * 1000;
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );

afterEach(() => vi.useRealTimers());

// Found before 17.0: "Thinking…" alone for 90 s looks hung. The live line says how long the model has been at it, on
// the reader's clock, and opens what it is thinking.
describe("WEB-U-123 a long think shows its time and its thinking", () => {
  it("counts the current step's time live", async () => {
    vi.useFakeTimers({ now: after(4, 90) });
    show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see." }),
    ]);
    const group = screen.getByRole("group", { name: "Activity" });
    const live = within(group).getByRole("button", { name: /^Thinking…/ });
    expect(live).toHaveTextContent("Thinking… · 1 min 30 s");
    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
    });
    expect(live).toHaveTextContent("Thinking… · 1 min 35 s");
  });

  // 17.3: the thinking shows itself, open under the live line; the line folds it.
  it("shows the current step's thinking open under the live line, once, and folds it from there", () => {
    vi.useFakeTimers({ now: after(4, 3) });
    show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see the totals." }),
    ]);
    const live = screen.getByRole("button", { name: /^Thinking…/ });
    expect(live).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Let me see the totals.")).toBeVisible();
    expect(screen.getAllByText("Let me see the totals.")).toHaveLength(1);
    fireEvent.click(live);
    expect(live).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("Let me see the totals.")).not.toBeVisible();
  });

  it("keeps earlier steps' thinking in the group's Thinking while the next step thinks", () => {
    vi.useFakeTimers({ now: after(7, 2) });
    show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "First thought." }),
      frame(5, { type: "text.delta", text: "Checking." }),
      frame(6, { type: "step.completed", step: 1, duration_ms: 400 }),
      frame(7, { type: "step.started", step: 2 }, 2),
      frame(8, { type: "reasoning.delta", text: "Second thought." }, 2),
    ]);
    const group = screen.getByRole("group", { name: "Activity" });
    const earlier = group.querySelector("details.reasoning");
    expect(earlier).toHaveTextContent("First thought.");
    expect(earlier).not.toHaveTextContent("Second thought.");
    expect(screen.getAllByText("Second thought.")).toHaveLength(1);
  });

  it("is plain words with its time when there is no thinking to open", () => {
    vi.useFakeTimers({ now: after(3, 4) });
    show(start);
    expect(screen.queryByRole("button", { name: /^Thinking…/ })).toBeNull();
    expect(screen.getByText("Thinking…").parentElement).toHaveTextContent(
      "Thinking… · 4 s",
    );
  });

  it("counts on the reader's clock when the runtime's runs ahead", () => {
    // The runtime's clock is 50 s ahead: each frame arrives, on the reader's clock, 50 s before its stamp.
    const skew = -50_000;
    const transcript = [
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see." }),
    ].reduce(
      (state, next) => applyAt(state, next, Date.parse(next.ts) + skew),
      emptyTranscript(),
    );
    vi.useFakeTimers({ now: after(3, 30) + skew });
    render(<TranscriptView transcript={transcript} />);
    expect(
      screen.getByRole("button", { name: /^Thinking…/ }),
    ).toHaveTextContent("Thinking… · 30 s");
  });

  it("leaves no live line once the turn is done, with all its thinking in the group's Thinking", () => {
    show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see." }),
      frame(5, { type: "text.delta", text: "Done." }),
      frame(6, { type: "step.completed", step: 1, duration_ms: 400 }),
      frame(7, { type: "run.completed", run_id: runId }),
    ]);
    expect(screen.queryByRole("button", { name: /^Thinking…/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    expect(
      screen
        .getByRole("group", { name: "Activity" })
        .querySelector("details.reasoning"),
    ).toHaveTextContent("Let me see.");
  });
});
