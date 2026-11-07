import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ConversationBar } from "./ConversationBar";

type Box = { left: number; top: number; width: number; height: number };
const none: Box = { left: 0, top: 0, width: 0, height: 0 };
/** The layout the test sets, by what each element is. */
let layout: (element: HTMLElement) => Box = () => none;

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const { left, top, width, height } = layout(this);
      return {
        left,
        top,
        width,
        height,
        right: left + width,
        bottom: top + height,
        x: left,
        y: top,
        toJSON: () => ({}),
      };
    },
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  layout = () => none;
});

function show() {
  render(
    <ConversationBar
      title="Quarterly numbers"
      onRename={() => Promise.resolve(true)}
      choices={<span className="header-choices">Local model</span>}
      panel={<button type="button">Files</button>}
    />,
  );
}
/** A header: the bar, the title, where the choices and the Files icon are, and the pickers' wrapper's own box. */
function header(choices: Box, files: Box, wrapper: Box = none) {
  layout = (element) => {
    if (element.classList.contains("conversation-bar-main"))
      return { left: 0, top: 0, width: 390, height: 80 };
    if (element.tagName === "H1")
      return { left: 40, top: 8, width: 120, height: 24 };
    if (element.classList.contains("header-choices")) return choices;
    if (element.textContent === "Files" && element.tagName === "BUTTON")
      return files;
    // On a phone (display: contents) the pickers' wrapper has no box of its own; on a wide header it has one.
    if (element.classList.contains("conversation-pickers")) return wrapper;
    return none;
  };
}
const field = () => screen.getByRole("textbox", { name: "Task title" });

// Regression (TEST-PLAN §19, fixed in c4cf0f5e): on a phone the pickers' wrapper has no box, and the rename field ran
// to the bar's end, over the Files icon and the ⋯. It ends before the first control on the title's row.
describe("WEB-U-159 the rename field ends before what follows the title on its row", () => {
  it("on a phone, before Files, with the pickers on the line below", () => {
    header(
      { left: 40, top: 40, width: 200, height: 24 },
      { left: 300, top: 8, width: 32, height: 32 },
    );
    show();
    fireEvent.click(screen.getByRole("heading", { level: 1 }));
    expect(field().style.left).toBe("40px");
    expect(field().style.width).toBe("260px");
  });

  it("on a wide header, before the pickers on the title's row", () => {
    header(
      { left: 500, top: 8, width: 200, height: 24 },
      { left: 720, top: 8, width: 32, height: 32 },
      { left: 500, top: 8, width: 260, height: 32 },
    );
    show();
    fireEvent.click(screen.getByRole("heading", { level: 1 }));
    expect(field().style.width).toBe("460px");
  });
});
