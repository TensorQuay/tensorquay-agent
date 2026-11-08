import { fireEvent, render, screen } from "@testing-library/react";
import { Ellipsis } from "lucide-react";
import type { ReactNode } from "react";
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
    // It is placed in the window's coordinates (WEB-U-429): left + 230 must not pass 896.
    expect(parseFloat(tooltip.style.left) + 230).toBeLessThanOrEqual(896);
    vi.restoreAllMocks();
  });
});

// WEB-U-429: the tip is a layer of its own, never its button's content (a 36 px holder would overflow with it), and
// it goes once the pointer moves on, even when the browser sent no leave (a Send clicked with the pointer left on it
// through Sending, the run and Send again, in WEB-U-024's run-changes test).
describe("WEB-U-429 an icon button's tip is a layer of its own", () => {
  const rect = (left: number, top: number, width: number, height: number) =>
    ({
      left,
      top,
      width,
      height,
      right: left + width,
      bottom: top + height,
    }) as DOMRect;
  const show = (wrap?: (node: ReactNode) => ReactNode) => {
    const action = (
      <IconAction
        glyph={Ellipsis}
        name="More actions for Ben"
        tip="More actions"
        onClick={vi.fn()}
      />
    );
    render(<div data-testid="row">{wrap ? wrap(action) : action}</div>);
    const button = screen.getByRole("button", { name: "More actions for Ben" });
    return { button, holder: button.parentElement as HTMLElement };
  };
  const tooltip = () => document.querySelector<HTMLElement>(".action-tooltip");

  it("shows its tip on the page, outside its holder and its row", () => {
    const { holder } = show();
    fireEvent.pointerEnter(holder);
    expect(tooltip()).toHaveTextContent("More actions");
    expect(holder.contains(tooltip())).toBe(false);
    expect(screen.getByTestId("row").contains(tooltip())).toBe(false);
    expect(tooltip()?.parentElement).toBe(document.body);
  });

  it("shows it inside the modal dialog its button is in, where the page beneath is inert", () => {
    const { holder } = show((node) => <dialog open>{node}</dialog>);
    fireEvent.pointerEnter(holder);
    expect(tooltip()?.parentElement).toBe(holder.closest("dialog"));
    expect(holder.contains(tooltip())).toBe(false);
  });

  it("goes when the pointer moves on outside its button, with or without a leave", () => {
    const { button, holder } = show();
    fireEvent.pointerEnter(holder);
    fireEvent.pointerMove(button);
    expect(tooltip()).not.toBeNull();
    // No pointerleave: the next move elsewhere on the page ends it.
    fireEvent.pointerMove(screen.getByTestId("row"));
    expect(tooltip()).toBeNull();
  });

  it("follows its button when the page scrolls while it shows", () => {
    const { holder } = show();
    const place = vi
      .spyOn(holder, "getBoundingClientRect")
      .mockReturnValue(rect(400, 300, 36, 36));
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(28);
    fireEvent.pointerEnter(holder);
    // Centred over the button, above it: 418 - 50 = 368; 300 - 28 = 272.
    expect(tooltip()?.style.left).toBe("368px");
    expect(tooltip()?.style.top).toBe("272px");
    place.mockReturnValue(rect(400, 200, 36, 36));
    fireEvent.scroll(window);
    expect(tooltip()?.style.top).toBe("172px");
    vi.restoreAllMocks();
  });
});
