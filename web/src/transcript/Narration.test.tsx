import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { present } from "./present";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const frames = [
  frame(1, { type: "run.started", run_id: runId }),
  frame(2, { type: "step.started", step: 1 }),
  frame(3, { type: "text.delta", text: "First I will inspect the files." }),
  frame(4, {
    type: "tool.call",
    call_id: "read",
    name: "read_file",
    arguments: { path: "README.md" },
  }),
  frame(5, { type: "step.completed", step: 1, duration_ms: 20 }),
  frame(6, { type: "step.started", step: 2 }, 2),
  frame(
    7,
    { type: "text.delta", text: "## The final answer\nThe project is ready." },
    2,
  ),
  frame(8, { type: "step.completed", step: 2, duration_ms: 10 }, 2),
  frame(9, { type: "run.completed", run_id: runId }, 2),
];

describe("WEB-U-016 narration belongs with its work", () => {
  it("keeps a concurrent running tool last within its step, after the narration", async () => {
    const live = [
      frame(1, { type: "run.started", run_id: runId }),
      frame(2, { type: "step.started", step: 1 }),
      frame(3, { type: "text.delta", text: "I will check both files." }),
      frame(4, {
        type: "tool.call",
        call_id: "slow",
        name: "read_file",
        arguments: { path: "slow.py" },
      }),
      frame(5, { type: "tool.start", call_id: "slow" }),
      frame(6, {
        type: "tool.call",
        call_id: "fast",
        name: "read_file",
        arguments: { path: "fast.py" },
      }),
      frame(7, { type: "tool.start", call_id: "fast" }),
      frame(8, {
        type: "tool.result",
        call_id: "fast",
        status: "ok",
        content: "1\tprint(42)",
      }),
    ].reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={live} />);
    await settleLazy();
    screen.getByText("I will check both files.");
    const rows = within(
      screen.getByRole("group", { name: "Activity" }),
    ).getAllByRole("listitem");
    // A tool row reads by its line; the latest one's open card follows it in the same row.
    expect(
      rows.map(
        (row) =>
          row.querySelector(".activity-toggle")?.textContent ?? row.textContent,
      ),
    ).toEqual([
      "I will check both files.",
      "Read fast.py line 1",
      "Read slow.py",
    ]);
    // The running one turns its mark where a word would be (17.3).
    expect(rows.at(-1)?.querySelector(".activity-working")).not.toBeNull();
  });
  it("streams undecided text in the work group, then shows only the final answer below it", async () => {
    const live = frames.slice(0, 7).reduce(apply, emptyTranscript());
    const view = render(<TranscriptView transcript={live} />);
    const activity = screen.getByRole("group", { name: "Activity" });
    await settleLazy();
    expect(
      within(activity).getByText("First I will inspect the files."),
    ).toBeVisible();
    expect(
      within(activity).getByRole("heading", { name: "The final answer" }),
    ).toBeVisible();
    expect(view.container.querySelectorAll(".markdown-answer")).toHaveLength(0);
    const finished = frames.reduce(apply, emptyTranscript());
    view.rerender(<TranscriptView transcript={finished} />);
    expect(
      screen.getByRole("heading", { name: "The final answer" }),
    ).toBeVisible();
    expect(
      screen.getByText("First I will inspect the files."),
    ).not.toBeVisible();
    expect(view.container.querySelectorAll(".markdown-answer")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    await settleLazy();
    expect(
      within(activity).getByText("First I will inspect the files."),
    ).toBeVisible();
    expect(
      within(activity).queryByRole("heading", { name: "The final answer" }),
    ).toBeNull();
    expect(present(finished)).toEqual(
      present(frames.reduce(apply, emptyTranscript())),
    );
  });
});
