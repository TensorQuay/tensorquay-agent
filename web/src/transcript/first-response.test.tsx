import { act, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const start = [
  frame(1, { type: "message.user", text: "Summarize", attachments: [] }),
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
const WAITING = "Waiting for the model to start answering…";

afterEach(() => vi.useRealTimers());

// WEB-U-099: a model that takes long to start (a CPU model server reading a long prompt) says so, instead of an
// unexplained "Thinking…" for minutes.
describe("WEB-U-099 waiting for the model's first answer", () => {
  it("says so once a model call has had nothing back for 10 seconds", async () => {
    vi.useFakeTimers({ now: after(3, 5) });
    show(start);
    const group = screen.getByRole("group", { name: "Activity" });
    expect(within(group).getByText("Thinking…")).toBeVisible();
    expect(within(group).queryByText(WAITING)).toBeNull();
    await act(async () => {
      vi.advanceTimersByTime(6000);
      await Promise.resolve();
    });
    expect(within(group).getByText(WAITING)).toBeVisible();
    expect(within(group).queryByText("Thinking…")).toBeNull();
  });

  it("goes back to Thinking once anything arrives, and waits again after a retry", () => {
    vi.useFakeTimers({ now: after(4, 30) });
    const { unmount } = show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see." }),
    ]);
    expect(screen.queryByText(WAITING)).toBeNull();
    unmount();
    // A retry is a new attempt: the model has said nothing yet in it.
    vi.setSystemTime(after(6, 12));
    show([
      ...start,
      frame(4, { type: "reasoning.delta", text: "Let me see." }),
      frame(5, {
        type: "provider.retry",
        attempt: 1,
        reason: "timeout",
        delay_ms: 0,
      }),
      frame(6, { type: "step.started", step: 1, attempt: 2 }),
    ]);
    expect(screen.getByText(WAITING)).toBeVisible();
  });
});
