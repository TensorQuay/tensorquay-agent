import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { frame, runId } from "../test/frames";
import { DesktopSwitchContext } from "../transcript/desktop-switch";
import { apply, emptyTranscript } from "../transcript/reducer";
import { ToolCard } from "../transcript/ToolCard";
import type { ToolView } from "../transcript/types";

import { ScreenPictures } from "./Pictures";

const task = { id: "task-1" } as Conversation;
const show = (tool: ToolView) =>
  render(
    <DesktopSwitchContext.Provider value={{ conversation: task }}>
      <ScreenPictures tool={tool} />
    </DesktopSwitchContext.Provider>,
  );
const call: ToolView = {
  id: "c-1",
  name: "computer",
  arguments: { action: "click", ref: "e7" },
  status: "ok",
  output: "",
  result: "Clicked e7.",
  ran: true,
  startedAt: null,
  durationMs: 10,
  exitCode: null,
};
const capture = (phase: "before" | "after" | "screenshot", marked = false) => ({
  phase,
  id: `shot-${phase}`,
  width: 1280,
  height: 800,
  marked,
});

// WEB-U-246 (ADR-0039 §9, revision 14): a desktop action keeps its element, whether its pictures were kept, and its
// pictures, on its call; its card shows before and after, the model's own screenshot as what it saw, and says what
// wasn't kept or is gone.
describe("WEB-U-246 a desktop action's pictures", () => {
  it("keeps the element, the evidence and each picture on the call", () => {
    const state = [
      frame(1, { type: "message.user", text: "Save it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "tool.call",
        call_id: "c-1",
        name: "computer",
        arguments: { action: "click", ref: "e7" },
      }),
      frame(4, {
        type: "screen.capture",
        call_id: "c-1",
        phase: "before",
        id: "shot-before",
        width: 1280,
        height: 800,
        bytes: 1000,
        sha256: "a".repeat(64),
        sent_to_model: false,
        marked: false,
      }),
      frame(5, { type: "tool.start", call_id: "c-1" }),
      frame(6, { type: "tool.end", call_id: "c-1", status: "ok" }),
      frame(7, {
        type: "tool.result",
        call_id: "c-1",
        status: "ok",
        content: "Clicked e7.",
        target: { role: "button", name: "Save" },
        evidence: { before: true, after: false },
      }),
    ].reduce(apply, emptyTranscript());
    const step = state.items.find((item) => item.kind === "step");
    const tool = step?.kind === "step" ? step.tools["c-1"] : undefined;
    expect(tool).toMatchObject({
      target: { role: "button", name: "Save" },
      evidence: { before: true, after: false },
      captures: [{ phase: "before", id: "shot-before", width: 1280 }],
    });
  });

  it("shows before and after from the task's screenshots, each in its room, and opens one wide", () => {
    show({ ...call, captures: [capture("before"), capture("after")] });
    const before = screen.getByRole("img", { name: "Before" });
    expect(before).toHaveAttribute(
      "src",
      "/api/v1/conversations/task-1/screenshots/shot-before",
    );
    expect(before).toHaveAttribute("width", "1280");
    expect(before).toHaveAttribute("height", "800");
    expect(screen.getByRole("img", { name: "After" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Open After wide" }));
    expect(
      within(screen.getByRole("dialog", { name: "After" })).getByRole("img"),
    ).toBeVisible();
  });

  it("says a picture wasn't kept, or is gone, and names the marks on what the model saw", () => {
    show({
      ...call,
      evidence: { before: true, after: false },
      captures: [capture("before"), capture("screenshot", true)],
    });
    expect(screen.getByText("This picture couldn't be saved.")).toBeVisible();
    expect(
      screen.getByRole("img", { name: "What the model saw" }),
    ).toBeVisible();
    expect(
      screen.getByText("The numbers mark the elements the agent could act on."),
    ).toBeVisible();
    fireEvent.error(screen.getByRole("img", { name: "Before" }));
    expect(
      screen.getByText("This screenshot isn't available any more."),
    ).toBeVisible();
  });

  it("shows nothing for a call with no pictures and nothing missing", () => {
    const { container } = show(call);
    expect(container).toBeEmptyDOMElement();
  });

  // WEB-U-282: a screenshot call's evidence has `after` alone, for what the model saw; "not kept" is only ever that
  // false field, never a capture that didn't come (an older runtime sends none).
  it("WEB-U-282: says what the model saw wasn't kept from a screenshot call's evidence, and infers nothing", () => {
    const screenshot = { ...call, arguments: { action: "screenshot" } };
    show({
      ...screenshot,
      evidence: { after: false },
    });
    const figure = screen.getByText("What the model saw").closest("figure");
    expect(figure).toHaveTextContent(
      "What the model saw" + "This picture couldn't be saved.",
    );
    expect(screen.queryByText("After")).toBeNull();
    cleanup();
    const { container } = show(screenshot);
    expect(container).toBeEmptyDOMElement();
  });
});

// A UX review before 17.0: on a desktop step's card the pictures are what it did, so they come first, then its output,
// and its arguments (an element's ref, "e7", that only the model reads) last, under their own label.
describe("WEB-U-246 a desktop step's card puts its pictures first", () => {
  it("shows the pictures, then the output, then what it was given", () => {
    render(
      <DesktopSwitchContext.Provider value={{ conversation: task }}>
        <ToolCard
          tool={{
            ...call,
            output: "Clicked e7.",
            captures: [capture("before"), capture("after")],
          }}
          initiallyOpen
        />
      </DesktopSwitchContext.Provider>,
    );
    const picture = screen.getByRole("img", { name: "Before" });
    const output = screen.getByText("Output");
    const given = screen.getByText("What it was given", { selector: "p" });
    const follows = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(follows(picture, output)).toBe(true);
    expect(follows(output, given)).toBe(true);
  });
});
