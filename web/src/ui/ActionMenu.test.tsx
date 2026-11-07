import { fireEvent, render, screen, within } from "@testing-library/react";
import { Pencil, Trash2 } from "lucide-react";
import { useRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { ActionMenu, type MenuAction } from "./ActionMenu";

function Row({ actions }: { actions: readonly MenuAction[] }) {
  const row = useRef<HTMLDivElement>(null);
  return (
    <div ref={row} data-testid="row">
      <span>Quarterly report</span>
      <ActionMenu
        label="More actions for Quarterly report"
        actions={actions}
        context={row}
      />
    </div>
  );
}
const trigger = () =>
  screen.getByRole("button", { name: "More actions for Quarterly report" });
const menu = () =>
  screen.getByRole("menu", { name: "More actions for Quarterly report" });

// The UI standard's Menu: actions on one thing, from a ⋯ or a right-click, a line icon before every item; it closes
// after a choice, on Esc or a click elsewhere, and focus goes back to the ⋯.
describe("WEB-U-086 the action menu", () => {
  const rename = vi.fn();
  const remove = vi.fn();
  const actions: MenuAction[] = [
    { id: "rename", label: "Rename", icon: Pencil, onSelect: rename },
    {
      id: "delete",
      label: "Delete",
      icon: Trash2,
      danger: true,
      onSelect: remove,
    },
  ];

  it("opens from the ⋯ with an icon before every item, the first focused", () => {
    render(<Row actions={actions} />);
    expect(trigger()).toHaveAttribute("aria-haspopup", "menu");
    fireEvent.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    const items = within(menu()).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Rename", "Delete"]);
    for (const item of items)
      expect(item.firstElementChild?.tagName.toLowerCase()).toBe("svg");
    expect(items[0]).toHaveFocus();
    expect(items[1]).toHaveAttribute("data-danger", "true");
  });

  it("closes after a choice, gives focus back to the ⋯, then acts", () => {
    rename.mockImplementation(() => expect(trigger()).toHaveFocus());
    render(<Row actions={actions} />);
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
    expect(rename).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger()).toHaveFocus();
  });

  it("closes on Esc and on a click elsewhere", () => {
    render(<Row actions={actions} />);
    fireEvent.click(trigger());
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "Rename" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger()).toHaveFocus();
    fireEvent.click(trigger());
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens from a right-click on the thing it acts on", () => {
    render(<Row actions={actions} />);
    // fireEvent answers false when the page kept the browser's own menu from opening.
    expect(fireEvent.contextMenu(screen.getByText("Quarterly report"))).toBe(
      false,
    );
    expect(menu()).toBeInTheDocument();
  });

  it("keeps an unavailable action reachable, says why, and does nothing", () => {
    const blocked = vi.fn();
    render(
      <Row
        actions={[
          {
            id: "delete",
            label: "Delete",
            icon: Trash2,
            unavailable: "A model still uses it.",
            onSelect: blocked,
          },
        ]}
      />,
    );
    fireEvent.click(trigger());
    const item = screen.getByRole("menuitem", { name: "Delete" });
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item).toHaveAccessibleDescription("A model still uses it.");
    fireEvent.click(item);
    expect(blocked).not.toHaveBeenCalled();
    expect(menu()).toBeInTheDocument();
  });

  it("opens a link item as a link", () => {
    render(
      <Row
        actions={[
          {
            id: "open",
            label: "Open task",
            icon: Pencil,
            href: "/c/0195d8c0-0000-7000-8000-00000000c0de",
          },
        ]}
      />,
    );
    fireEvent.click(trigger());
    expect(screen.getByRole("menuitem", { name: "Open task" })).toHaveAttribute(
      "href",
      "/c/0195d8c0-0000-7000-8000-00000000c0de",
    );
  });
});
