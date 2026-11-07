import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TableScroll } from "./TableScroll";

let resized: (() => void) | null = null;
class FakeObserver {
  constructor(callback: () => void) {
    resized = callback;
  }
  observe() {}
  disconnect() {}
}
function sizes(element: HTMLElement, scrollWidth: number, clientWidth: number) {
  Object.defineProperty(element, "scrollWidth", {
    value: scrollWidth,
    configurable: true,
  });
  Object.defineProperty(element, "clientWidth", {
    value: clientWidth,
    configurable: true,
  });
}
afterEach(() => {
  vi.unstubAllGlobals();
  resized = null;
});

// A table wider than its column scrolls in its own box, which the keyboard can reach only then (WCAG 2.1.1, WEB-U-007).
describe("WEB-U-007 a table's scroll box", () => {
  it("is a named, focusable region only while its table is wider than the column", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    const { container } = render(
      <TableScroll label="Usage by day">
        <table aria-label="Usage by day" />
      </TableScroll>,
    );
    const box = container.querySelector<HTMLElement>(".data-table-scroll");
    if (!box) throw new Error("the scroll box must render");
    expect(box).not.toHaveAttribute("tabindex");
    expect(screen.queryByRole("region")).toBeNull();
    sizes(box, 900, 390);
    act(() => resized?.());
    expect(screen.getByRole("region", { name: "Usage by day" })).toBe(box);
    expect(box).toHaveAttribute("tabindex", "0");
    sizes(box, 700, 700);
    act(() => resized?.());
    expect(box).not.toHaveAttribute("tabindex");
    expect(screen.queryByRole("region")).toBeNull();
  });

  // A row's ⋯ at the column's edge shows its tooltip after its menu closes (focus returns to it): the tooltip stays
  // inside the table's box, so it never widens the box into a sideways scroll (WEB-U-047's table fits its column).
  it("keeps a row action's tooltip inside the box", () => {
    const { container } = render(
      <TableScroll label="Environments">
        <table aria-label="Environments" />
      </TableScroll>,
    );
    expect(container.querySelector(".data-table-scroll")).toHaveAttribute(
      "data-tip-bounds",
    );
  });
});

// WEB-U-190: where a table's column is narrow, its rows are cards (settings.css), each cell read with its column's
// name, which the box copies from the header onto the cell. The table keeps its roles, which some browsers drop once a
// table's display changes.
describe("WEB-U-190 a table's cells carry their column's name", () => {
  function table(rows: string[][]) {
    return (
      <TableScroll label="People">
        <table aria-label="People">
          <thead>
            <tr>
              <th scope="col">Email</th>
              <th scope="col">Role</th>
              <th scope="col">Last active</th>
              <th scope="col">
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([email = "", role = "", active = ""]) => (
              <tr key={email}>
                <td>{email}</td>
                <td>{role}</td>
                <td>{active}</td>
                <td>
                  <button type="button">More</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableScroll>
    );
  }

  it("labels each cell but the row's name and its actions", () => {
    const { container } = render(table([["ana@acme.test", "Member", "Today"]]));
    const cells = [...container.querySelectorAll("tbody td")];
    expect(cells.map((cell) => cell.getAttribute("data-label"))).toEqual([
      null,
      "Role",
      "Last active",
      null,
    ]);
  });

  it("keeps the table's roles, so a card is still read as a row of cells", () => {
    render(table([["ana@acme.test", "Member", "Today"]]));
    expect(screen.getByRole("table", { name: "People" })).toBeInTheDocument();
    expect(screen.getAllByRole("columnheader")).toHaveLength(4);
    expect(screen.getAllByRole("row")).toHaveLength(2);
    expect(screen.getAllByRole("cell")).toHaveLength(4);
    expect(screen.getByRole("table")).toHaveAttribute("role", "table");
    expect(screen.getAllByRole("cell")[0]).toHaveAttribute("role", "cell");
  });

  it("labels a row that comes later", async () => {
    const { container, rerender } = render(
      table([["ana@acme.test", "Member", "Today"]]),
    );
    rerender(
      table([
        ["ana@acme.test", "Member", "Today"],
        ["sam@acme.test", "Admin", "Yesterday"],
      ]),
    );
    await act(() => Promise.resolve());
    const second = container.querySelectorAll("tbody tr")[1];
    expect(second?.querySelectorAll("td")[1]).toHaveAttribute(
      "data-label",
      "Role",
    );
    expect(second).toHaveAttribute("role", "row");
  });
});
