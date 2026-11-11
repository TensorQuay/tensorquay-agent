"use client";

import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/**
 * Names each body cell after its column (`data-label`), for the narrow layout where rows are cards (settings.css,
 * WEB-U-190), and gives the table's parts their roles, which some browsers drop once a table's display changes. The
 * row's name (its first cell) and a column whose header is only for screen readers (its actions) take no label, nor
 * does a column whose header says `data-card-label="none"`, where the row's name already says what the cell is (the
 * limits' values, WEB-U-361).
 */
function labelCells(table: Element) {
  table.setAttribute("role", "table");
  const headers = [...table.querySelectorAll(":scope > thead th")].map(
    (header) => {
      header.setAttribute("role", "columnheader");
      const shown = [...header.childNodes].some(
        (node) =>
          node.nodeType === Node.TEXT_NODE ||
          (node instanceof Element && !node.classList.contains("sr-only")),
      );
      if (header.getAttribute("data-card-label") === "none") return "";
      return shown ? (header.textContent ?? "").trim() : "";
    },
  );
  for (const group of table.querySelectorAll(
    ":scope > thead, :scope > tbody, :scope > tfoot",
  ))
    group.setAttribute("role", "rowgroup");
  for (const row of table.querySelectorAll(":scope > * > tr")) {
    row.setAttribute("role", "row");
    let column = 0;
    for (const cell of row.children) {
      if (cell.tagName === "TD") {
        cell.setAttribute("role", "cell");
        const label = column > 0 ? headers[column] : "";
        if (label) cell.setAttribute("data-label", label);
        else cell.removeAttribute("data-label");
      }
      column += cell instanceof HTMLTableCellElement ? cell.colSpan : 1;
    }
  }
}

/** Under this much area a table's rows are always cards (UI-GUIDELINES §7 Table, WEB-U-190). */
const CARDS_UNDER = 600;

/** `length` (a CSS length, such as `12ch` in `face`'s font) in px, as the box would draw it. */
function lengthIn(box: HTMLElement, length: string, face?: Element) {
  const ruler = document.createElement("span");
  ruler.style.cssText = `position:absolute;visibility:hidden;height:0;width:${length}`;
  if (face) {
    const font = getComputedStyle(face);
    ruler.style.fontFamily = font.fontFamily;
    ruler.style.fontSize = font.fontSize;
    ruler.style.fontWeight = font.fontWeight;
  }
  box.append(ruler);
  const px = parseFloat(getComputedStyle(ruler).width);
  ruler.remove();
  return Number.isFinite(px) ? px : 0;
}

/** How many lines `element`'s text takes. */
function lines(element: Element) {
  const range = document.createRange();
  range.selectNodeContents(element);
  return new Set(
    [...range.getClientRects()]
      .filter((line) => line.width > 0)
      .map((line) => Math.round(line.top)),
  ).size;
}

/**
 * Whether the name column (each row's first cell) keeps what §7 Table gives it: a value that wraps there
 * (`.cell-whole`) takes one line or has `--table-name-least` of room, so an everyday name stays on one line; a
 * reference cut there (`.secret-ref-line`) has 12ch of room in its own face. A name held to one line (`.data-name`)
 * keeps its 12ch in CSS, where a narrower area makes the table wider than it.
 */
function nameKeeps(box: HTMLElement, table: Element) {
  let least: number | null = null;
  for (const cell of table.querySelectorAll<HTMLElement>(
    ":scope > tbody > tr > td:first-child",
  )) {
    const padding = getComputedStyle(cell);
    const room =
      cell.clientWidth -
      parseFloat(padding.paddingLeft || "0") -
      parseFloat(padding.paddingRight || "0");
    for (const value of cell.querySelectorAll(".cell-whole")) {
      if (value.closest(".sr-only") || lines(value) < 2) continue;
      least ??= lengthIn(box, "var(--table-name-least)");
      if (room < least - 0.5) return false;
    }
    for (const value of cell.querySelectorAll(".secret-ref-line"))
      if (
        value.scrollWidth > value.clientWidth &&
        room < lengthIn(box, "12ch", value) - 0.5
      )
        return false;
  }
  return true;
}

/**
 * The table's form (UI-GUIDELINES §7 Table): its rows are cards (`data-cards` on the box) under 600 px of area, and
 * from there whenever the table, at its least width, is wider than its area or can't give its name column what it
 * keeps; otherwise a table. It measures the real table each time, in its row form, before the page is drawn again.
 */
function chooseForm(box: HTMLElement, table: Element | null) {
  const area = parseFloat(getComputedStyle(box).width);
  // Not laid out (hidden, or a document without layout): nothing to measure.
  if (!(area > 0)) return null;
  let cards = true;
  let rowWidth: number | null = null;
  if (area >= CARDS_UNDER && table) {
    box.removeAttribute("data-cards");
    rowWidth = table.getBoundingClientRect().width;
    cards = box.scrollWidth > box.clientWidth || !nameKeeps(box, table);
  }
  box.toggleAttribute("data-cards", cards);
  return rowWidth;
}

/**
 * A table's own horizontal scroll box (UI-GUIDELINES §6: the page never scrolls sideways). While the table is wider
 * than its column, the box is a named, focusable region so the keyboard can scroll it (WCAG 2.1.1); when the table
 * fits, it adds no Tab stop. Where the table does not fit its area, or its area is narrow, its rows are cards
 * (WEB-U-190), each cell named by its column; the scroll region stays as the last resort.
 */
export function TableScroll({
  label,
  labelledBy,
  children,
}: {
  /** The table's name, or the id of the heading that names it. */
  label?: string;
  labelledBy?: string;
  children: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const width = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    const table = element.querySelector(":scope > table");
    // The table's width in its row form when the form was last chosen.
    let rowWidth: number | null = null;
    const measure = () => {
      rowWidth = chooseForm(element, table);
      setOverflowing(element.scrollWidth > element.clientWidth);
    };
    const relabel = () => {
      if (table) labelCells(table);
    };
    relabel();
    measure();
    // Rows come and go with the data, and a cell's text changes (the language, a relative time): each new row is
    // named too, and the form is chosen again. labelCells' own attributes (role, data-label) are not watched.
    const changes =
      typeof MutationObserver === "undefined"
        ? null
        : new MutationObserver(() => {
            relabel();
            measure();
          });
    if (table)
      changes?.observe(table, {
        childList: true,
        subtree: true,
        characterData: true,
        attributes: true,
        attributeFilter: ["class", "hidden", "style"],
      });
    // A face that loads after the first paint changes the table's widths.
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    fonts?.addEventListener("loadingdone", measure);
    const stop = () => {
      changes?.disconnect();
      fonts?.removeEventListener("loadingdone", measure);
    };
    if (typeof ResizeObserver === "undefined") return stop;
    // The area's width, read from a line of no height across it: the form's own change of height never resizes what is
    // watched, so it is decided in the same frame, before the page is drawn. The table's size too, through its body:
    // an image in a cell loading, or a field's value, widens it with no change the mutation filter sees. The body is
    // a level deeper than the width line, so when a form chosen for the width resizes it, the browser delivers that
    // in the same frame instead of reporting a "ResizeObserver loop"; it finds the row form's width unchanged, or
    // cards, and stops. In its row form, a width other than the one last measured is decided again in the next
    // frame, outside the delivery. As cards the table keeps the area's width, so nothing there starts a decision.
    const body = table?.querySelector(":scope > tbody") ?? table;
    let frame = 0;
    const observer = new ResizeObserver((entries?: ResizeObserverEntry[]) => {
      const tableOnly =
        entries?.length && entries.every((entry) => entry.target === body);
      if (!tableOnly) {
        measure();
        return;
      }
      if (!table || element.hasAttribute("data-cards") || frame) return;
      const now = table.getBoundingClientRect().width;
      if (rowWidth !== null && Math.abs(now - rowWidth) < 0.5) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        measure();
      });
    });
    if (width.current) observer.observe(width.current);
    if (body) observer.observe(body);
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
      stop();
    };
  }, []);
  return (
    <div
      ref={box}
      className="data-table-scroll"
      // A row action's tooltip stays inside the box, so showing it never makes the table scroll sideways.
      data-tip-bounds=""
      {...(overflowing
        ? {
            role: "region",
            tabIndex: 0,
            ...(labelledBy
              ? { "aria-labelledby": labelledBy }
              : { "aria-label": label }),
          }
        : {})}
    >
      {children}
      <div ref={width} className="data-table-width" aria-hidden="true" />
    </div>
  );
}
