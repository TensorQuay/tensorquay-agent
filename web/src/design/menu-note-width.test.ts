// @vitest-environment node
// WEB-U-490 (18.0.1): a menu item's note, and its reason when it can't run, wrap at 24ch (about 27 characters of
// prose in the app's face, whose zero is wide: 36ch measured 325 px), the width the Screen's ⋯ menu wraps them at on a
// phone, so a menu with a long note keeps near the standard width wherever it opens and never runs across the window
// in one line (the Expand dialog's 658 px notes).
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

describe("WEB-U-490 a menu item's note wraps at 24ch", () => {
  it("caps the muted line under an action's word", () => {
    const line = rule(".action-menu-word > .type-small");
    expect(line).toMatch(/(?:^|[;\s])max-inline-size:\s*24ch\b/);
  });
});
