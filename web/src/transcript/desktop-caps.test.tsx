import { render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";

import { frame } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { apply, emptyTranscript } from "./reducer";
import { loadDesktopWords } from "./tool-summary";
import { TranscriptView } from "./TranscriptView";

// The Desktop tool's words load with the screen area (WEB-U-422); a page asks for them before its steps render.
beforeAll(() => loadDesktopWords());

/** One `computer` call refused with `code`, as the runtime records it. */
function refused(code: string) {
  return [
    frame(1, { type: "step.started", step: 1, attempt: 1 }),
    frame(2, {
      type: "tool.call",
      call_id: "call-desk",
      name: "computer",
      arguments: { action: "screenshot" },
    }),
    frame(3, { type: "tool.start", call_id: "call-desk" }),
    frame(4, {
      type: "tool.end",
      call_id: "call-desk",
      status: "error",
      failure: { code },
    }),
    frame(5, {
      type: "tool.result",
      call_id: "call-desk",
      status: "error",
      content: "",
    }),
  ].reduce(apply, emptyTranscript());
}

function line(code: string) {
  const { container } = render(<TranscriptView transcript={refused(code)} />);
  return {
    label: container.querySelector(".activity-label")?.textContent ?? "",
    mark: container.querySelector(".activity-mark"),
  };
}

// WEB-U-330 (a pass over the pages before 17.0): a per-run cap is the limit doing its job, not a failure. Its line says
// what happens next first, so a folded line keeps it, and its mark is a muted "Limit reached"; any other failure stays
// a "Failed" in the danger tone.
describe("WEB-U-330 a desktop cap reads as a limit, consequence first", () => {
  it.each([
    [
      "screenshot_limit",
      "No more pictures this run: the screenshot limit was reached",
    ],
    [
      "action_limit",
      "No more desktop actions this run: the action limit was reached",
    ],
    [
      "screenshot_too_large",
      "No picture this time: the screen was too detailed to send",
    ],
  ])("%s", (code, words) => {
    const { label, mark } = line(code);
    expect(label).toBe(words);
    expect(mark?.textContent).toBe("Limit reached");
    expect(mark).not.toHaveClass("activity-error");
    expect(mark).toHaveClass("activity-limit");
  });

  it("keeps Failed, in the danger tone, for any other failure", () => {
    const { mark } = line("timeout");
    expect(mark?.textContent).toBe("Failed");
    expect(mark).toHaveClass("activity-error");
  });

  // WEB-U-330: "Result" read as a heading over nothing. It is a fold, so it shows the product's chevron, as Details does
  // beside it.
  it("shows its Result fold with a chevron", async () => {
    const shown = [
      frame(1, { type: "step.started", step: 1, attempt: 1 }),
      frame(2, {
        type: "tool.call",
        call_id: "c",
        name: "computer",
        arguments: { action: "screenshot" },
      }),
      frame(3, { type: "tool.start", call_id: "c" }),
      frame(4, {
        type: "tool.end",
        call_id: "c",
        status: "error",
        failure: { code: "screenshot_too_large" },
      }),
      frame(5, {
        type: "tool.result",
        call_id: "c",
        status: "error",
        content: "screenshot_too_large: too much detail",
      }),
    ].reduce(apply, emptyTranscript());
    const { container } = render(<TranscriptView transcript={shown} />);
    await settleLazy();
    const summary = [...container.querySelectorAll("summary")].find(
      (each) => each.textContent === "Result",
    );
    expect(summary).toBeDefined();
    expect(summary?.querySelector("svg")).not.toBeNull();
    expect(summary).toHaveClass("tool-disclosure");
  });

  // A pass over the pages before 17.0: an opened cap card had a "Result" heading with nothing under it.
  it("shows no empty Result in its opened card", async () => {
    render(<TranscriptView transcript={refused("screenshot_too_large")} />);
    await settleLazy();
    expect(screen.getByText("Details")).toBeVisible();
    expect(screen.queryByText("Result")).toBeNull();
  });
});
