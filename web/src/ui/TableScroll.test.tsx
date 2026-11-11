import { act, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TableScroll } from "./TableScroll";

let resized: ((entries?: { target: Element }[]) => void) | null = null;
/** What the observer watches, as a browser's would: only a watched element's size is ever reported. */
const observed = new Set<Element>();
class FakeObserver {
  constructor(callback: (entries?: { target: Element }[]) => void) {
    resized = callback;
  }
  observe(target: Element) {
    observed.add(target);
  }
  disconnect() {
    observed.clear();
  }
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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  resized = null;
  observed.clear();
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

// §7 Table (17.4.1): from 600 px of area a table's rows are cards whenever the table, at its least width, is wider than
// its area or can't give its name column what it keeps; under 600 px always. jsdom draws nothing, so each test gives the
// box its area and the table's least width in row form; as cards the table takes the area. A form measured in cards
// would always "fit", so these also show the measurement is of the row form.
describe("WEB-U-190 a table is cards where it does not fit its area", () => {
  /** The box's area, and the table's least width in its row form (changed later through `needs.px`). */
  function laidOut(box: HTMLElement, area: number, needs: { px: number }) {
    box.style.width = `${String(area)}px`;
    Object.defineProperty(box, "clientWidth", {
      value: area,
      configurable: true,
    });
    Object.defineProperty(box, "scrollWidth", {
      configurable: true,
      get: () =>
        box.hasAttribute("data-cards") ? area : Math.max(area, needs.px),
    });
  }
  function render6(rows: ReactNode = <td>Main</td>) {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    const { container } = render(
      <TableScroll label="Environments">
        <table aria-label="Environments">
          <tbody>
            <tr>{rows}</tr>
          </tbody>
        </table>
      </TableScroll>,
    );
    const box = container.querySelector<HTMLElement>(".data-table-scroll");
    if (!box) throw new Error("the scroll box must render");
    return box;
  }

  it("is cards under 600 px of area, though the table fits", () => {
    const box = render6();
    laidOut(box, 599, { px: 300 });
    act(() => resized?.());
    expect(box).toHaveAttribute("data-cards");
  });

  it("is a table from 600 px where it fits, cards where it doesn't, and a table again only once it fits", () => {
    const box = render6();
    const needs = { px: 640 };
    laidOut(box, 700, needs);
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
    laidOut(box, 626, needs);
    act(() => resized?.());
    expect(box).toHaveAttribute("data-cards");
    // As cards the box never scrolls, but the row form still needs 640 px: it stays cards at 639.
    laidOut(box, 639, needs);
    act(() => resized?.());
    expect(box).toHaveAttribute("data-cards");
    expect(screen.queryByRole("region")).toBeNull();
    laidOut(box, 640, needs);
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
  });

  it("is chosen again when a cell's text grows after the first paint (a relative time)", async () => {
    const box = render6(
      <td>
        <time>…</time>
      </td>,
    );
    const needs = { px: 590 };
    laidOut(box, 600, needs);
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
    needs.px = 603;
    const time = box.querySelector("time");
    if (!time?.firstChild) throw new Error("the time must render");
    time.firstChild.textContent = "just now";
    await act(() => Promise.resolve());
    expect(box).toHaveAttribute("data-cards");
  });

  // An image in a cell loading, a field's value or an attribute outside the mutation filter widens the table with no
  // change the box is told of: the table's own size is watched, and a row form wider than last measured is decided
  // again, in the next frame, outside the observer's delivery (no "ResizeObserver loop").
  it("is chosen again when the table widens without a change it watches (an image loading)", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback) => {
      frames.push(frame);
      return frames.length;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});
    const box = render6(<td>Main</td>);
    const table = box.querySelector("table");
    if (!table) throw new Error("the table must render");
    const needs = { px: 590 };
    laidOut(box, 600, needs);
    Object.defineProperty(table, "getBoundingClientRect", {
      configurable: true,
      value: () =>
        new DOMRect(
          0,
          0,
          box.hasAttribute("data-cards") ? 600 : Math.max(600, needs.px),
          40,
        ),
    });
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
    // The table's body (a level below the width line) reports its size only if it is watched.
    const body = table.tBodies[0];
    if (!body) throw new Error("the table must have a body");
    const tableResized = () => {
      if (observed.has(body)) resized?.([{ target: body }]);
    };
    needs.px = 603;
    act(() => {
      tableResized();
      for (const frame of frames.splice(0)) frame(0);
    });
    expect(box).toHaveAttribute("data-cards");
    expect(box).not.toHaveAttribute("tabindex");
  });

  it("starts no new decision for a size the box gave the table itself", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback) => {
      frames.push(frame);
      return frames.length;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});
    const box = render6(<td>Main</td>);
    const table = box.querySelector("table");
    if (!table) throw new Error("the table must render");
    laidOut(box, 700, { px: 640 });
    Object.defineProperty(table, "getBoundingClientRect", {
      configurable: true,
      value: () => new DOMRect(0, 0, 700, 40),
    });
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
    const body = table.tBodies[0];
    if (!body) throw new Error("the table must have a body");
    expect(observed.has(body)).toBe(true);
    act(() => resized?.([{ target: body }]));
    expect(frames).toHaveLength(0);
  });

  it("decides nothing for a box that isn't laid out", () => {
    const box = render6();
    laidOut(box, 0, { px: 900 });
    act(() => resized?.());
    expect(box).not.toHaveAttribute("data-cards");
  });

  /** A name cell of `room` px whose value takes `lines` lines, or is cut (a reference). */
  function nameCell(kind: "whole" | "reference", room: number, lines = 1) {
    const real = window.getComputedStyle.bind(window);
    // The rulers: `--table-name-least` is 128 px and 12ch 96 px in the value's face.
    vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
      const style = real(element);
      const width = (element as HTMLElement).style.width;
      const px =
        width === "var(--table-name-least)"
          ? "128px"
          : width === "12ch"
            ? "96px"
            : null;
      return px
        ? new Proxy(style, {
            get: (target, key): unknown =>
              key === "width" ? px : Reflect.get(target, key),
          })
        : style;
    });
    vi.spyOn(document, "createRange").mockImplementation(() => {
      let node: Node | null = null;
      return {
        selectNodeContents: (at: Node) => {
          node = at;
        },
        getClientRects: () =>
          Array.from(
            { length: node === value ? lines : 1 },
            (_, line) => ({ width: 40, top: line * 20 }) as DOMRect,
          ),
      } as unknown as Range;
    });
    const box = render6(
      <td style={{ padding: 0 }}>
        {kind === "whole" ? (
          <code className="cell-whole">WIKI_API_TOKEN</code>
        ) : (
          <span className="secret-ref-line">OPENAI_API_KEY_FOR_REPORTS</span>
        )}
      </td>,
    );
    const cell = box.querySelector("td");
    const value = box.querySelector<HTMLElement>(
      ".cell-whole, .secret-ref-line",
    );
    if (!cell || !value) throw new Error("the name cell must render");
    Object.defineProperty(cell, "clientWidth", { value: room });
    Object.defineProperty(value, "scrollWidth", { value: 200 });
    Object.defineProperty(value, "clientWidth", {
      value: kind === "reference" ? room : 200,
    });
    laidOut(box, 700, { px: 500 });
    act(() => resized?.());
    return box;
  }

  it("is cards where a name that wraps has less than --table-name-least of room", () => {
    expect(nameCell("whole", 120, 2)).toHaveAttribute("data-cards");
  });

  it("is a table where a name that wraps has --table-name-least, or takes one line", () => {
    expect(nameCell("whole", 128, 2)).not.toHaveAttribute("data-cards");
    vi.restoreAllMocks();
    expect(nameCell("whole", 90, 1)).not.toHaveAttribute("data-cards");
  });

  it("is cards where a reference cut in the name column has less than 12ch of room", () => {
    expect(nameCell("reference", 95)).toHaveAttribute("data-cards");
    vi.restoreAllMocks();
    expect(nameCell("reference", 96)).not.toHaveAttribute("data-cards");
  });
});
