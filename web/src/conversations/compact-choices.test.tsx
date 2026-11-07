import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ConversationBar } from "./ConversationBar";

// Layout the test sets: the bar's width, and the pickers' and controls' own widths while the names show.
let barWidth = 800;
let pickersWidth = 300;
const observers: (() => void)[] = [];

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("conversation-bar-main") ? barWidth : 0;
    },
  );
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("conversation-pickers") ? pickersWidth : 0;
    },
  );
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private readonly callback: () => void) {
        observers.push(() => this.callback());
      }
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  observers.length = 0;
  barWidth = 800;
  pickersWidth = 300;
});

function show(name = "Local model") {
  return render(
    <ConversationBar
      title="Quarterly numbers"
      choices={
        <span className="header-choices">
          <span className="picker-name">{name}</span>
        </span>
      }
    />,
  );
}
const bar = () => screen.getByRole("region", { name: "Task header" });
const resize = (width: number) =>
  act(() => {
    barWidth = width;
    for (const notify of observers) notify();
  });

// The lead's F6 note: beside the Files panel at 1280 the header fell back to icons although the names fitted. The
// icons are for when the names don't fit beside the title's and status's least room (8rem and 4rem).
describe("WEB-U-125 the header's pickers show their names whenever they fit", () => {
  it("shows the names when they fit, and the icons when they don't", () => {
    // Needed: the pickers and controls (300) + the title's least (128) + the status's least (64) = 492.
    show();
    expect(bar()).not.toHaveAttribute("data-compact");
    resize(491);
    expect(bar()).toHaveAttribute("data-compact");
    resize(492);
    expect(bar()).not.toHaveAttribute("data-compact");
  });

  it("measures again when a name changes", async () => {
    const { rerender } = show();
    resize(520);
    expect(bar()).not.toHaveAttribute("data-compact");
    // A longer model name: the pickers need 360 now, 552 in all.
    pickersWidth = 360;
    rerender(
      <ConversationBar
        title="Quarterly numbers"
        choices={
          <span className="header-choices">
            <span className="picker-name">A much longer model name</span>
          </span>
        }
      />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(bar()).toHaveAttribute("data-compact");
  });

  it("never goes compact without pickers", () => {
    render(<ConversationBar title="Quarterly numbers" />);
    resize(100);
    expect(bar()).not.toHaveAttribute("data-compact");
  });
});
