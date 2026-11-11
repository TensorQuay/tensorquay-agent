import { within } from "@testing-library/react";
import { expect } from "vitest";

/**
 * The muted line in `row` whose words include `words` (UI-GUIDELINES §7 Table: a cell is its value and at most one
 * muted line, cut with "…" where its column ends); every time, its tooltip holds the line's words whole, or `title`
 * where the line is short for figures the tooltip gives exactly (§10, WEB-U-461). It is the line's one tooltip.
 */
export function cellNote(
  row: HTMLElement,
  words: string,
  title?: string,
): HTMLElement {
  const line = within(row).getByText(
    (_, element) =>
      element?.classList.contains("cell-note") === true &&
      (element.textContent ?? "").includes(words),
  );
  expect(line).toHaveAttribute("title", title ?? line.textContent);
  expect(line.querySelector("[title]")).toBeNull();
  return line;
}
