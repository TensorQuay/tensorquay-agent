// @vitest-environment node
// WEB-U-460: the Settings menu stays in view while a long section scrolls (from 768 px, where it shows), and a menu
// taller than the window scrolls inside its own area, with room for the focus ring at its edges.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const settings = readFileSync("src/app/settings.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
const tokens = readFileSync("src/app/globals.css", "utf8");

/** The pixels of a value that is one CSS variable reference, as globals.css defines that variable. */
function px(value: string | undefined): number {
  const name = value?.match(/^var\((--[a-z0-9-]+)\)$/)?.[1];
  const found = name && tokens.match(new RegExp(`${name}:\\s*(\\d+)px;`));
  if (!found) throw new Error(`not one pixel token: ${value}`);
  return Number(found[1]);
}

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

  // 17.4 fix: at the page's top the menu starts under the page's top padding, the title's line and the row gap, while
  // its height is capped as when it sticks (the two gaps of `top`), so the window hides that much of its foot, less its
  // ring margin. Room at its end of at least that, with the ring's room, lets the menu scroll its last section into
  // view by itself, the page left at its top; the room is a step on the spacing scale (UI-GUIDELINES §5).
  it("has room at its foot for the part the window hides while the page is at its top", () => {
    const page = rule(settings, ".settings-page");
    const ring = px("var(--space-1)");
    const hidden =
      px(declared(page, "padding-block")) +
      px("var(--heading-line-height)") +
      px(declared(page, "row-gap")) -
      2 * px(declared(nav, "top")) -
      ring;
    const room = declared(nav, "padding-bottom");
    expect(room).toMatch(/^var\(--space-\d+\)$/);
    expect(px(room)).toBeGreaterThanOrEqual(hidden + ring);
  });

  it("keeps room for the 2 px focus ring at its 2 px offset inside its own scroll area", () => {
    // The padding gives the ring room; the matching negative margin keeps the items where they were.
    expect(declared(nav, "padding")).toBe("var(--space-1)");
    expect(declared(nav, "margin")).toBe("calc(-1 * var(--space-1))");
  });
});
