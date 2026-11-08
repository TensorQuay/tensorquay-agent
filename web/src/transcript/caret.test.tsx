import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { AssistantMarkdown } from "./AssistantMarkdown";
import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const start = [
  frame(1, { type: "message.user", text: "Run the tests", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1 }),
  frame(4, { type: "text.delta", text: "I'll run the first test pass." }),
  frame(5, {
    type: "tool.call",
    call_id: "tests",
    name: "shell",
    arguments: { command: "pytest -q" },
  }),
];
const narration = () =>
  screen.getByText(/run the first test pass/).closest(".activity-narration");

// UI-GUIDELINES §8: a thin caret at the end while text streams (17.3). The step's narration is done once its next call
// starts, and the caret follows the last word, never a line of its own under the paragraph.
describe("WEB-U-439 the narration's caret", () => {
  it("stays while the step's next call is only asked for, and ends when it starts", async () => {
    const { unmount } = render(
      <TranscriptView transcript={start.reduce(apply, emptyTranscript())} />,
    );
    await settleLazy();
    expect(narration()?.querySelector(".stream-caret")).not.toBeNull();
    unmount();
    render(
      <TranscriptView
        transcript={[
          ...start,
          frame(6, { type: "tool.start", call_id: "tests" }),
        ].reduce(apply, emptyTranscript())}
      />,
    );
    await settleLazy();
    expect(narration()?.querySelector(".stream-caret")).toBeNull();
  });

  it("sits after the last word: in the last paragraph, or a closing list's last item", () => {
    const { container, unmount } = render(
      <AssistantMarkdown text={"First part.\n\nI'll run the tests."} caret />,
    );
    expect([...container.querySelectorAll("p")].at(-1)?.lastChild).toHaveClass(
      "stream-caret",
    );
    expect(container.querySelectorAll(".stream-caret")).toHaveLength(1);
    unmount();
    const list = render(
      <AssistantMarkdown
        text={"Next:\n\n- read the file\n- run the tests"}
        caret
      />,
    );
    expect(
      list.container.querySelector("li:last-child")?.lastChild,
    ).toHaveClass("stream-caret");
    expect(list.container.querySelectorAll(".stream-caret")).toHaveLength(1);
  });

  it("is hidden from assistive technology, and absent from text that isn't streaming", () => {
    const { container } = render(<AssistantMarkdown text="Done." caret />);
    expect(container.querySelector(".stream-caret")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    const quiet = render(<AssistantMarkdown text="Done." />);
    expect(quiet.container.querySelector(".stream-caret")).toBeNull();
  });
});
