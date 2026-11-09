// @vitest-environment node
// WEB-U-460: the Settings menu stays in view while a long section scrolls (from 768 px, where it shows), and a menu
// taller than the window scrolls inside its own area, with room for the focus ring at its edges.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const settings = readFileSync("src/app/settings.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The declarations of every top-level rule whose selector list names `selector` exactly, joined. */
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

describe("WEB-U-460 the Settings menu stays in view", () => {
  const nav = rule(settings, ".settings-nav");

  it("sticks under the page's top padding while the section scrolls", () => {
    expect(declared(nav, "position")).toBe("sticky");
    expect(declared(nav, "top")).toBe("var(--space-6)");
    expect(declared(nav, "align-self")).toBe("start");
  });

  it("scrolls on its own when it is taller than the window, never cut off", () => {
    expect(declared(nav, "max-height")).toBe(
      "calc(100dvh - 2 * var(--space-6))",
    );
    expect(declared(nav, "overflow-y")).toBe("auto");
    expect(declared(nav, "overscroll-behavior")).toBe("contain");
  });

  it("keeps room for the 2 px focus ring at its 2 px offset inside its own scroll area", () => {
    // The padding gives the ring room; the matching negative margin keeps the items where they were.
    expect(declared(nav, "padding")).toBe("var(--space-1)");
    expect(declared(nav, "margin")).toBe("calc(-1 * var(--space-1))");
  });
});
