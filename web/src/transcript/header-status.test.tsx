import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { DONE_IN_HEADER, RunStatus } from "./RunStatus";

const started = [
  frame(1, { type: "message.user", text: "List the files", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
];
const done = [
  ...started,
  frame(3, { type: "run.completed", run_id: runId }),
].reduce(apply, emptyTranscript());
const working = started.reduce(apply, emptyTranscript());
const failed = [
  ...started,
  frame(3, { type: "run.failed", run_id: runId, code: "io", reason: "x" }),
].reduce(apply, emptyTranscript());

/** The status word's own element: the one the screen shows unless it is off the screen. */
const word = () => screen.getByRole("status");

// WEB-U-495 (18.1; the owner, 17 Nov): the task header says no "Done" (the transcript's end and the sidebar say it);
// every other status stays on the title's line. The word is still read: it leaves the screen, not the status line.
// Hidden or grey is the owner's call, one constant (DONE_IN_HEADER): grey shows it muted.
describe("WEB-U-495 the header's Done", () => {
  it("is hidden by default, and still read", () => {
    expect(DONE_IN_HEADER).toBe("hidden");
    render(<RunStatus transcript={done} />);
    expect(word()).toHaveTextContent("Done");
    expect(word()).toHaveClass("sr-only");
    expect(screen.getByLabelText("Run status")).not.toHaveAttribute(
      "data-short",
    );
  });

  it("shows muted when the owner chooses grey", () => {
    render(<RunStatus transcript={done} done="muted" />);
    expect(word()).toHaveTextContent("Done");
    expect(word()).not.toHaveClass("sr-only");
    expect(screen.getByLabelText("Run status")).toHaveAttribute(
      "data-tone",
      "muted",
    );
  });

  it("keeps every other status on the line", () => {
    const { rerender } = render(<RunStatus transcript={working} />);
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
    expect(screen.getByLabelText("Run status")).toBeVisible();
    rerender(<RunStatus transcript={failed} />);
    expect(screen.getByLabelText("Run status")).toHaveAttribute(
      "data-tone",
      "danger",
    );
    // A failure keeps its short word on the line (WEB-U-155), the whole one read.
    expect(screen.getByLabelText("Run status")).toHaveAttribute(
      "data-short",
      "Failed",
    );
  });
});
