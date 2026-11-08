import { within } from "@testing-library/react";
import { expect } from "vitest";

/**
 * The muted line in `row` whose words include `words` (UI-GUIDELINES §7 Table: a cell is its value and at most one
 * muted line, cut with "…" where its column ends); every time, its tooltip holds the line's words whole.
 */
export function cellNote(row: HTMLElement, words: string): HTMLElement {
  const line = within(row).getByText(
    (_, element) =>
      element?.classList.contains("cell-note") === true &&
      (element.textContent ?? "").includes(words),
  );
  expect(line).toHaveAttribute("title", line.textContent);
  return line;
}
