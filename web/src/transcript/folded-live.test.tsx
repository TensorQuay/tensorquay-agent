import { fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { Event } from "../protocol/generated/protocol";
import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { loadStepWords } from "./step-words-loader";
import { TranscriptView } from "./TranscriptView";

const BASE = Date.UTC(2026, 0, 1);
const at = (seq: number, seconds: number, event: Event, step = 1) => ({
  ...frame(seq, event, step),
  ts: new Date(BASE + seconds * 1000).toISOString(),
});
const start = [
  frame(1, { type: "message.user", text: "Check the demo", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
  frame(4, { type: "text.delta", text: "I'll look at the demo first.\n\n" }),
];
const reading = [
  ...start,
  frame(5, {
    type: "tool.call",
    call_id: "read",
    name: "read_file",
    arguments: { path: "demo/README.md" },
  }),
  at(6, 30, { type: "tool.start", call_id: "read" }),
];
const show = (frames: ReturnType<typeof frame>[]) =>
  render(
    <TranscriptView transcript={frames.reduce(apply, emptyTranscript())} />,
  );
const toggle = () =>
  screen
    .getByRole("group", { name: "Activity" })
    .querySelector<HTMLButtonElement>("button.activity-toggle");
/** What the folded heading shows on screen: its words for sighted readers only. */
const shown = () => toggle()?.querySelector(".activity-heading-live");

afterEach(() => vi.useRealTimers());
beforeAll(async () => {
  await loadStepWords();
});

// The owner, 9 Nov 2026: a folded group that only says "Working…" looks stuck. Folded while the run works, its heading
// is the live line: the step it is on, the soft shimmer and the time (WEB-U-462).
describe("WEB-U-462 a folded group shows the step it is on", () => {
  it("shows the running call's step and its time, with the shimmer on the words only", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show(reading);
    const button = toggle();
    if (!button) throw new Error("the group has its toggle");
    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(shown()).toHaveTextContent(/^Reading README\.md… · 4 s$/);
    expect(shown()?.querySelector(".shimmer")).toHaveTextContent(
      /^Reading README\.md…$/,
    );
  });

  it("keeps the button's name steady while the time goes on", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show(reading);
    const button = toggle();
    if (!button) throw new Error("the group has its toggle");
    fireEvent.click(button);
    expect(button).toHaveAccessibleName("Working…");
    act(() => {
      vi.advanceTimersByTime(3_000);
    });
    expect(shown()).toHaveTextContent(/ · 7 s$/);
    expect(button).toHaveAccessibleName("Working…");
  });

  it("says Thinking… with the step's time when no call runs", () => {
    vi.useFakeTimers({ now: BASE + 10_000 });
    // The step started and the model hasn't written a word yet (while it writes, no live line speaks over it).
    show(start.slice(0, 3));
    const button = toggle();
    if (!button) throw new Error("the group has its toggle");
    fireEvent.click(button);
    expect(shown()).toHaveTextContent(/^Thinking… · \d+ s$/);
  });

  it("leaves the heading plain while the group is open: the live line shows once, at its end", () => {
    vi.useFakeTimers({ now: BASE + 34_000 });
    show(reading);
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
    expect(shown()).toBeNull();
    expect(
      screen
        .getByRole("group", { name: "Activity" })
        .querySelector(".activity-live"),
    ).toHaveTextContent(/^Reading README\.md… · 4 s$/);
  });
});
