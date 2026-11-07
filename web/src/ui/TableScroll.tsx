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

/**
 * A table's own horizontal scroll box (UI-GUIDELINES §6: the page never scrolls sideways). While the table is wider
 * than its column, the box is a named, focusable region so the keyboard can scroll it (WCAG 2.1.1); when the table
 * fits, it adds no Tab stop. Where its column is narrow, the table's rows are cards (WEB-U-190), each cell named by its
 * column.
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
  const [overflowing, setOverflowing] = useState(false);
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    const measure = () =>
      setOverflowing(element.scrollWidth > element.clientWidth);
    measure();
    const table = element.firstElementChild;
    const relabel = () => {
      if (table?.tagName === "TABLE") labelCells(table);
    };
    relabel();
    // Rows come and go with the data: each new one is named too.
    const rows =
      typeof MutationObserver === "undefined"
        ? null
        : new MutationObserver(relabel);
    if (table)
      rows?.observe(table, {
        childList: true,
        subtree: true,
        characterData: true,
      });
    if (typeof ResizeObserver === "undefined") return () => rows?.disconnect();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    // A table that grows (a new row, a longer value) does not resize its box, so the table is watched too.
    if (element.firstElementChild) observer.observe(element.firstElementChild);
    return () => {
      observer.disconnect();
      rows?.disconnect();
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
    </div>
  );
}
