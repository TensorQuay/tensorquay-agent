// @vitest-environment node
// WEB-U-090 (the UI standard's Page), one column since the owner's review (WEB-U-131): each field's label above it at
// every width, fields at most --field-width wide, lists the whole column; Save with Cancel pinned at the bottom of the
// view, within the Settings content column.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const settings = readFileSync("src/app/settings.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
const globals = readFileSync("src/app/globals.css", "utf8");

/** The declarations of every rule whose selector list names `selector` exactly, joined. */
function rule(css: string, selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
const declared = (block: string, property: string) =>
  block.match(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`))?.[1]?.trim();
/** The rules inside `@media (min-width: 1024px)`. */
const wide =
  settings.match(/@media \(min-width: 1024px\)\s*\{([\s\S]*?\})\s*\}/)?.[1] ??
  "";

describe("WEB-U-090 the page frame's layout", () => {
  it("has no label column at any width, and caps a field's width", () => {
    expect(wide).not.toMatch(/\.page-section\b/);
    expect(settings).not.toMatch(/--page-label-width/);
    expect(globals).not.toMatch(/--page-label-width/);
    expect(declared(rule(settings, ".page-section .field"), "max-width")).toBe(
      "var(--field-width)",
    );
    expect(globals).toMatch(/--field-width:\s*640px;/);
  });

  // WEB-U-097: the ID's quiet line reads as one line under the display name.
  it("keeps a note under a field as one line", () => {
    expect(declared(rule(settings, ".field-note"), "display")).toBe("block");
  });

  it("stacks every section's parts in one column", () => {
    expect(declared(rule(settings, ".page-section"), "display")).toBe("flex");
    expect(declared(rule(settings, ".page-section"), "flex-direction")).toBe(
      "column",
    );
  });

  // A sticky box stops at its scroller's padding edge: at "bottom: 0" the page showed through the content box's own
  // bottom padding under the bar (WEB-U-097). The bar reaches past that padding to the view's edge.
  it("pins Save and Cancel flush on the bottom edge of the view", () => {
    const footer = rule(settings, ".settings-form-footer");
    expect(declared(footer, "position")).toBe("sticky");
    expect(declared(footer, "bottom")).toBe("calc(-1 * var(--content-end))");
    expect(globals).toMatch(/--content-end:\s*var\(--space-\d+\);/);
    for (const block of [
      rule(globals, ".app-content"),
      // Phones: the content box's own padding, the same bottom.
      globals.match(/@media[^{]*\{[^@]*?\.app-content\s*\{([^}]*)\}/)?.[1] ??
        "",
    ])
      expect(declared(block, "padding")).toMatch(/ var\(--content-end\)$/);
  });
});
