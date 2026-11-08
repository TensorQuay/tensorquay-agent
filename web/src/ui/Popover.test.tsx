import { fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Popover } from "./Popover";

function Anchored({ inDialog }: { inDialog: boolean }) {
  const anchor = useRef<HTMLButtonElement>(null);
  const content = (
    <>
      <button ref={anchor} type="button">
        More
      </button>
      <Popover anchor={anchor} label="Actions" onClose={() => undefined}>
        <p>Menu</p>
      </Popover>
    </>
  );
  return inDialog ? <dialog open>{content}</dialog> : content;
}

// WEB-U-071: a task's menu in the phone's navigation drawer (a modal dialog) opens inside the dialog, above it, where it
// can be used; anywhere else it opens on the page.
describe("WEB-U-071 a menu inside a dialog", () => {
  it("renders in the dialog its trigger is in, else in the page", () => {
    const { unmount } = render(<Anchored inDialog />);
    expect(
      screen.getByRole("region", { name: "Actions" }).closest("dialog"),
    ).not.toBeNull();
    unmount();
    render(<Anchored inDialog={false} />);
    expect(screen.getByRole("region", { name: "Actions" }).parentElement).toBe(
      document.body,
    );
  });
});

/** The panel's gutter, as its stylesheet gives it; named, since the test sets it in place of the stylesheet. */
const GUTTER = "16px";
function Bounded() {
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <aside
      data-menu-bounds=""
      data-testid="panel"
      style={{ paddingLeft: GUTTER, paddingRight: GUTTER }}
    >
      <button ref={anchor} type="button">
        Run
      </button>
      <Popover
        anchor={anchor}
        label="Runs"
        placement="below-start"
        onClose={() => undefined}
      >
        <p>A run named by a very long first message</p>
      </Popover>
    </aside>
  );
}
const rect = (left: number, top: number, width: number, height: number) =>
  ({
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
  }) as DOMRect;

// WEB-U-432: a menu that opens in a panel marked as its bounds (the Files panel) stays inside the panel's content: no
// wider than it, moved along into it, never over the transcript beside it; elsewhere the window bounds it, as before.
describe("WEB-U-432 a menu inside its panel", () => {
  afterEach(() => vi.restoreAllMocks());

  it("is no wider than the panel's content and starts inside it", () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1440);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(874);
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(60);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.dataset.testid === "panel") return rect(985, 0, 455, 900);
        if (this.tagName === "BUTTON") return rect(1001, 60, 300, 36);
        return rect(0, 0, 0, 0);
      },
    );
    render(<Bounded />);
    const surface = screen.getByRole("region", { name: "Runs" });
    // The panel's content: 985 + 16 to 1440 - 16, so 423 px wide; the menu starts under its trigger.
    expect(surface.style.maxWidth).toBe("423px");
    expect(surface.style.left).toBe("1001px");
  });
});

// WEB-U-433: a menu taller than its room scrolls inside; its own scrolling must not place it again, which measured it at
// its full height and so put its list back at the top: its last entries could never be reached. A scroll of the page
// around its trigger still places it again.
describe("WEB-U-433 a menu's own scrolling", () => {
  it("doesn't place the menu again; the page's scrolling does", () => {
    render(<Anchored inDialog={false} />);
    const surface = screen.getByRole("region", { name: "Actions" });
    const anchor = screen.getByRole("button", { name: "More" });
    const placed = vi.spyOn(anchor, "getBoundingClientRect");
    fireEvent.scroll(surface);
    expect(placed).not.toHaveBeenCalled();
    fireEvent.scroll(document);
    expect(placed).toHaveBeenCalled();
    vi.restoreAllMocks();
  });
});
