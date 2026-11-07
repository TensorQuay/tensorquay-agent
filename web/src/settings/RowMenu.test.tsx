import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RowMenu } from "./RowMenu";

const kind = (item: HTMLElement) =>
  [...(item.querySelector("svg")?.classList ?? [])].find(
    (name) => name !== "lucide" && name.startsWith("lucide-"),
  );

// A row's "…" menu: a hidden 16 px icon before every action (the UI standard: icons before every menu item; it was only
// where they helped, WEB-U-080), and the accessible names unchanged (UI-GUIDELINES §7).
describe("WEB-U-080 row menu icons", () => {
  it("puts an icon before every action", () => {
    render(
      <RowMenu
        label="More actions for route local"
        actions={[
          { id: "edit", label: "Edit", name: "Edit local", onSelect: vi.fn() },
          {
            id: "download",
            label: "Download",
            name: "Download local",
            onSelect: vi.fn(),
          },
          {
            id: "reads",
            label: "Reads",
            name: "Reads of local",
            onSelect: vi.fn(),
          },
          {
            id: "duplicate",
            label: "Duplicate",
            name: "Duplicate local",
            onSelect: vi.fn(),
          },
          {
            id: "delete",
            label: "Delete",
            name: "Delete local",
            danger: true,
            onSelect: vi.fn(),
          },
        ]}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for route local" }),
    );
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.getAttribute("aria-label"))).toEqual([
      "Edit local",
      "Download local",
      "Reads of local",
      "Duplicate local",
      "Delete local",
    ]);
    expect(items.map(kind)).toEqual([
      "lucide-pencil",
      "lucide-download",
      "lucide-rotate-ccw-clock",
      "lucide-copy",
      "lucide-trash",
    ]);
    for (const item of items)
      for (const svg of item.querySelectorAll("svg"))
        expect(svg).toHaveAttribute("aria-hidden", "true");
  });

  it.each([
    ["bindings", "lucide-users"],
    ["check", "lucide-circle-check"],
    ["default", "lucide-star"],
    ["paste", "lucide-clipboard-paste"],
    ["reminder", "lucide-bell"],
    ["remove", "lucide-trash"],
  ])("gives %s its icon", (id, icon) => {
    render(
      <RowMenu
        label="More actions for key local"
        actions={[{ id, label: id, name: `${id} local`, onSelect: vi.fn() }]}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for key local" }),
    );
    expect(kind(screen.getByRole("menuitem", { name: `${id} local` }))).toBe(
      icon,
    );
  });
});
