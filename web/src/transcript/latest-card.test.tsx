import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const shell = (seq: number, id: string, command: string, output: string) => [
  frame(seq, {
    type: "tool.call",
    call_id: id,
    name: "shell",
    arguments: { command },
  }),
  frame(seq + 1, { type: "tool.start", call_id: id }),
  frame(seq + 2, {
    type: "tool.output",
    call_id: id,
    chunk: 0,
    data: output,
    last: true,
  }),
  frame(seq + 3, { type: "tool.end", call_id: id, status: "ok", exit_code: 0 }),
  frame(seq + 4, {
    type: "tool.result",
    call_id: id,
    status: "ok",
    content: output,
  }),
];

// Seeing a command's output took three clicks. The latest tool card opens with the group; earlier ones
// stay folded to one line each, and each still opens and closes on demand.
describe("WEB-U-016 the latest tool card is open", () => {
  it("opens the latest card with its group, leaving the earlier ones folded", async () => {
    render(
      <TranscriptView
        transcript={[
          frame(1, { type: "message.user", text: "Check", attachments: [] }),
          frame(2, { type: "run.started", run_id: runId }),
          ...shell(3, "c-1", "ls", "README.md"),
          ...shell(8, "c-2", "pytest -q", "3 passed"),
          frame(13, { type: "run.completed", run_id: runId }),
        ].reduce(apply, emptyTranscript())}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Worked for/ }));
    await settleLazy();
    const first = screen.getByRole("button", { name: /^Ran a command: ls/ });
    const latest = screen.getByRole("button", {
      name: /^Ran a command: pytest -q/,
    });
    expect(first).toHaveAttribute("aria-expanded", "false");
    expect(latest).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("3 passed")).toBeVisible();
    fireEvent.click(latest);
    expect(latest).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(first);
    expect(first).toHaveAttribute("aria-expanded", "true");
  });
});
