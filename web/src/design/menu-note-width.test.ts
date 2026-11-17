// @vitest-environment node
// WEB-U-490 (18.0.1): a menu item's note, and its reason when it can't run, wrap at 36 characters, the line UI-GUIDELINES
// §7 gives a note, so a menu with a long note (the Screen's ⋯) keeps a readable width wherever it opens and never runs
// across the window in one line (the Expand dialog's 720 px menu).
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/menu.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

function rule(selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}

describe("WEB-U-490 a menu item's note wraps at 36 characters", () => {
  it("caps the muted line under an action's word", () => {
    const line = rule(".action-menu-word > .type-small");
    expect(line).toMatch(/(?:^|[;\s])max-inline-size:\s*36ch\b/);
  });
});
