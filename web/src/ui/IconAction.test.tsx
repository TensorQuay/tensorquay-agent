import { fireEvent, render, screen } from "@testing-library/react";
import { Ellipsis } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { IconAction } from "./IconAction";

// WEB-U-066: an icon-only button's tooltip (UI-GUIDELINES §5b). It names the action on hover, repeats its accessible
// name without being read twice, and never shows over the button's own open menu. Where it is placed (inside the
// window and its panel) is measured in the browser (channels.spec.ts, at 1280 and 390 px).
describe("WEB-U-066 an icon button's tooltip", () => {
  const show = (expanded?: boolean) =>
    render(
      <div data-tip-bounds>
        <IconAction
          glyph={Ellipsis}
          name="More actions for Ben"
          tip="More actions"
          onClick={vi.fn()}
          {...(expanded === undefined ? {} : { expanded })}
        />
      </div>,
    );
  const tooltip = () => document.querySelector(".action-tooltip");

  it("shows the tip on hover, hidden from assistive technology, and hides it again", () => {
    show(false);
    const button = screen.getByRole("button", { name: "More actions for Ben" });
    const holder = button.parentElement as HTMLElement;
    expect(tooltip()).toBeNull();
    fireEvent.pointerEnter(holder);
    expect(tooltip()).toHaveTextContent("More actions");
    expect(tooltip()).toHaveAttribute("aria-hidden", "true");
    fireEvent.pointerLeave(holder);
    expect(tooltip()).toBeNull();
  });

  it("never shows over the button's own open menu", () => {
    show(true);
    const button = screen.getByRole("button", { name: "More actions for Ben" });
    expect(button).toHaveAttribute("aria-expanded", "true");
    fireEvent.pointerEnter(button.parentElement as HTMLElement);
    expect(tooltip()).toBeNull();
  });
});

// WEB-U-267 (the c14b first hour, F4): with the Files panel open, the task header's ⋯ sits at its page's right edge and
// its tooltip ran under the panel. Inside a bounded page it ends at the page's edge, whatever room the window has.
describe("WEB-U-267 a tooltip stays inside its page", () => {
  it("ends at the page's right edge, not the window's", () => {
    render(
      <div data-tip-bounds data-testid="page">
        <IconAction
          glyph={Ellipsis}
          name="More actions for Sales summary fix"
          tip="More actions for Sales summary fix"
          onClick={vi.fn()}
        />
      </div>,
    );
    const rect = (left: number, top: number, width: number, height: number) =>
      ({
        left,
        top,
        width,
        height,
        right: left + width,
        bottom: top + height,
      }) as DOMRect;
    // A page from 260 to 896 px in a 1280 px window, the button at its right edge.
    vi.spyOn(
      screen.getByTestId("page"),
      "getBoundingClientRect",
    ).mockReturnValue(rect(260, 0, 636, 800));
    const button = screen.getByRole("button", {
      name: "More actions for Sales summary fix",
    });
    const holder = button.parentElement as HTMLElement;
    vi.spyOn(holder, "getBoundingClientRect").mockReturnValue(
      rect(836, 10, 36, 36),
    );
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(230);
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(28);
    fireEvent.pointerEnter(holder);
    const tooltip = document.querySelector(".action-tooltip") as HTMLElement;
    // Its left is relative to the button: 836 + left + 230 must not pass 896.
    expect(836 + parseFloat(tooltip.style.left) + 230).toBeLessThanOrEqual(896);
    vi.restoreAllMocks();
  });
});
