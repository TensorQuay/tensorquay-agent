import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Event } from "../protocol/generated/protocol";
import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const BASE = Date.UTC(2026, 0, 1);
/** A frame stamped `seconds` after the helper's clock starts. */
const at = (seq: number, seconds: number, event: Event, step = 1) => ({
  ...frame(seq, event, step),
  ts: new Date(BASE + seconds * 1000).toISOString(),
});
const start = [
  at(1, 0, { type: "message.user", text: "Plan the report", attachments: [] }),
  at(2, 0, { type: "run.started", run_id: runId }),
  at(3, 0, { type: "step.started", step: 1 }),
];
const thought = "The totals come from three sheets; I'll compare them first.";
const transcriptOf = (frames: ReturnType<typeof frame>[]) =>
  frames.reduce(apply, emptyTranscript());
const group = () => screen.getByRole("group", { name: "Activity" });

afterEach(() => vi.useRealTimers());

// The owner: "a nice progressing indicator instead of just thinking". While the model thinks, its thinking streams in
// an open panel under the live line, fading in as it arrives; once the answer starts, it folds to "Thought for 12 s"
// where it was (17.3; it used to fold by default, WEB-U-123).
describe("WEB-U-440 thinking shows itself while the model thinks", () => {
  it("streams in an open panel under the live line", () => {
    vi.useFakeTimers({ now: BASE + 4_000 });
    render(
      <TranscriptView
        transcript={transcriptOf([
          ...start,
          at(4, 1, { type: "reasoning.delta", text: thought }),
        ])}
      />,
    );
    const live = within(group()).getByRole("button", { name: /^Thinking…/ });
    expect(live).toHaveTextContent(/^Thinking… · 4 s$/);
    expect(live).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(thought)).toBeVisible();
    expect(screen.getAllByText(thought)).toHaveLength(1);
  });

  it("fades in each part of it as it arrives, keeping what was there", () => {
    vi.useFakeTimers({ now: BASE + 4_000 });
    const first = [
      ...start,
      at(4, 1, { type: "reasoning.delta", text: "The totals come" }),
    ];
    const { rerender } = render(
      <TranscriptView transcript={transcriptOf(first)} />,
    );
    const panel = group().querySelector(".reasoning-live") as HTMLElement;
    const before = panel.querySelector(".arriving");
    rerender(
      <TranscriptView
        transcript={transcriptOf([
          ...first,
          at(5, 2, { type: "reasoning.delta", text: " from three sheets." }),
        ])}
      />,
    );
    const parts = [...panel.querySelectorAll(".arriving")];
    expect(parts.map((part) => part.textContent)).toEqual([
      "The totals come",
      " from three sheets.",
    ]);
    // What was there stays as it was; only the new part arrives.
    expect(parts[0]).toBe(before);
  });

  it("follows its newest line while the reader is at its end, and stays where they scrolled up", () => {
    vi.useFakeTimers({ now: BASE + 4_000 });
    const parts = [
      "The totals come",
      " from three sheets.",
      " Then the regions.",
    ];
    const upTo = (count: number) =>
      transcriptOf([
        ...start,
        ...parts
          .slice(0, count)
          .map((text, index) =>
            at(4 + index, 1 + index, { type: "reasoning.delta", text }),
          ),
      ]);
    const { rerender } = render(<TranscriptView transcript={upTo(1)} />);
    const panel = group().querySelector(".reasoning-live") as HTMLElement;
    // A browser's layout, which the test's document has none of: 100 px tall, scrolling over what it holds.
    let height = 300;
    Object.defineProperty(panel, "scrollHeight", { get: () => height });
    Object.defineProperty(panel, "clientHeight", { value: 100 });
    rerender(<TranscriptView transcript={upTo(2)} />);
    expect(panel.scrollTop).toBe(300);
    // Scrolled up to read: the next part leaves them there.
    panel.scrollTop = 50;
    height = 400;
    rerender(<TranscriptView transcript={upTo(3)} />);
    expect(panel.scrollTop).toBe(50);
  });

  it("lets the person fold it from the live line", () => {
    vi.useFakeTimers({ now: BASE + 4_000 });
    render(
      <TranscriptView
        transcript={transcriptOf([
          ...start,
          at(4, 1, { type: "reasoning.delta", text: thought }),
        ])}
      />,
    );
    const live = within(group()).getByRole("button", { name: /^Thinking…/ });
    fireEvent.click(live);
    expect(live).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText(thought)).not.toBeVisible();
  });

  it("folds to “Thought for 12 s” where it was once the answer starts, with no live line while the answer streams", () => {
    vi.useFakeTimers({ now: BASE + 13_000 });
    render(
      <TranscriptView
        transcript={transcriptOf([
          ...start,
          at(4, 1, { type: "reasoning.delta", text: thought }),
          at(5, 12, { type: "text.delta", text: "Here is the plan." }),
        ])}
      />,
    );
    expect(group().querySelector(".activity-live")).toBeNull();
    const folded = group().querySelector("details.reasoning") as HTMLElement;
    expect(folded).not.toHaveAttribute("open");
    expect(folded.querySelector("summary")).toHaveTextContent(
      /^Thought for 12 s$/,
    );
    expect(folded).toHaveTextContent(thought);
    expect(screen.getAllByText(thought)).toHaveLength(1);
    // Where it was: before the step's own words.
    const items = [...group().querySelectorAll(".activity-lines > li")];
    expect(items.map((item) => item.className)).toEqual([
      "activity-thought",
      "activity-narration markdown-body",
    ]);
  });

  it("says a quick think took under a second", () => {
    render(
      <TranscriptView
        transcript={transcriptOf([
          ...start,
          at(4, 0.2, { type: "reasoning.delta", text: thought }),
          at(5, 0.6, { type: "text.delta", text: "Done." }),
          at(6, 1, { type: "step.completed", step: 1, duration_ms: 1000 }),
          at(7, 1, { type: "run.completed", run_id: runId }),
        ])}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^Worked for/ }));
    expect(
      group().querySelector("details.reasoning > summary"),
    ).toHaveTextContent(/^Thought for under a second$/);
  });

  it("keeps each step's thinking in its own place, the live step's in the panel", () => {
    vi.useFakeTimers({ now: BASE + 20_000 });
    render(
      <TranscriptView
        transcript={transcriptOf([
          ...start,
          at(4, 1, { type: "reasoning.delta", text: "First thought." }),
          at(5, 3, { type: "text.delta", text: "Checking." }),
          at(6, 4, { type: "step.completed", step: 1, duration_ms: 4000 }),
          at(7, 4, { type: "step.started", step: 2 }, 2),
          at(8, 5, { type: "reasoning.delta", text: "Second thought." }, 2),
        ])}
      />,
    );
    const folded = [...group().querySelectorAll("details.reasoning")];
    expect(folded).toHaveLength(1);
    expect(folded[0]).toHaveTextContent(/^Thought for 3 sFirst thought\.$/);
    expect(group().querySelector(".reasoning-live")).toHaveTextContent(
      "Second thought.",
    );
  });
});
