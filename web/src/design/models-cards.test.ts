// @vitest-environment node
// WEB-U-502 (18.1, the 17.2 review's finding 18, decided on 18 Nov 2026): on a phone a Models card opens its route on a
// tap anywhere, through its Edit link stretched over the card with its word hidden; the ⋯ and the card's own links sit
// above it; one keyboard stop opens the card, its focus ring drawn around the card. WEB-E2E-136 taps it in a browser.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/settings.css", "utf8")
  .replace(/\/\*[\s\S]*?\*\//g, "")
  .replace(/\s+/g, " ");

function rule(selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
const card = ":where(.data-table-scroll[data-cards]) .data-table.models-table";

describe("WEB-U-502 a Models card opens on a tap", () => {
  it("stretches the card's link over it and hides its word", () => {
    expect(rule(`${card} tbody tr`)).toMatch(/position:\s*relative/);
    const link = rule(`${card} .route-open`);
    expect(link).toMatch(/position:\s*absolute/);
    expect(link).toMatch(/inset:\s*0/);
    expect(rule(`${card} .route-open-word`)).toMatch(
      /clip-path:\s*inset\(50%\)/,
    );
  });

  it("keeps the ⋯ and the card's own links above it", () => {
    expect(rule(`${card} td button`)).toMatch(/z-index:\s*1/);
    expect(rule(`${card} td a:not(.route-open)`)).toMatch(/z-index:\s*1/);
  });

  it("draws the keyboard's ring around the card", () => {
    expect(rule(`${card} tbody tr:has(.route-open:focus-visible)`)).toMatch(
      /outline:\s*var\(--focus-width\)/,
    );
  });
});
