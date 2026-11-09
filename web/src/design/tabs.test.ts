// @vitest-environment node
// WEB-U-455 (UI-GUIDELINES §7 Tabs, 9 Nov 2026): one tab style, set once in globals.css from the tokens. The
// chosen tab is the selected fill with its label in ink, the others muted labels that take the hover tint; one weight,
// no underline; at least 32 px (44 px on touch screens), the label centred, tabs 4 px apart. A strip wider than its
// place scrolls sideways inside itself, with room for the app's one focus ring (WEB-U-089), so it never cuts it off.
import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const globals = read("globals.css");
const sheets = readdirSync("src/app").filter((file) => file.endsWith(".css"));

/** The declarations of every rule whose selector list names `selector` exactly, top level or nested, joined. */
function rule(css: string, selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "")
      .split(",")
      .map((part) => part.replace(/\s+/g, " ").trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
/** The last value a rule set gives `property`, as the cascade would. */
const declared = (block: string, property: string) =>
  [...block.matchAll(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`, "g"))]
    .at(-1)?.[1]
    ?.trim();
/** The tokens set at the top level, and those a touch screen sets instead. */
const tokens = [...globals.matchAll(/\n:root\s*\{([^}]*)\}/g)]
  .map((match) => match[1] ?? "")
  .join(";");
const touch =
  globals.match(/@media \(pointer: coarse\)\s*\{\s*:root\s*\{([^}]*)\}/)?.[1] ??
  "";
/** Every selector of every sheet, with the sheet it is in. */
const selectors = sheets.flatMap((file) =>
  [...read(file).matchAll(/([^{};]+)\{/g)].flatMap((match) =>
    (match[1] ?? "")
      .split(",")
      .map((part) => ({ file, selector: part.replace(/\s+/g, " ").trim() })),
  ),
);

describe("WEB-U-455 one tab style", () => {
  it("draws the chosen tab as the selected fill in ink, the others as muted labels at one weight", () => {
    const tab = rule(globals, ".tab");
    expect(declared(tab, "background")).toBe("transparent");
    expect(declared(tab, "color")).toBe("var(--muted)");
    expect(declared(tab, "border")).toBe("0");
    expect(declared(tab, "border-radius")).toBe("var(--control-radius)");
    expect(declared(tab, "font-size")).toBe("var(--nav-font-size)");
    expect(declared(tab, "line-height")).toBe("var(--nav-line-height)");
    expect(declared(tab, "font-weight")).toBe("400");
    for (const chosen of [
      '.tab[aria-selected="true"]',
      '.tab[aria-current="page"]',
    ]) {
      const block = rule(globals, chosen);
      expect(declared(block, "background")).toBe("var(--selected-fill)");
      expect(declared(block, "color")).toBe("var(--ink)");
      expect(declared(block, "font-weight")).toBe(undefined);
    }
  });

  it("tints a tab under the pointer, and keeps the chosen tab's fill there", () => {
    const hover = rule(globals, ".tab:hover");
    expect(declared(hover, "background")).toBe("var(--hover-tint)");
    expect(declared(hover, "color")).toBe("var(--ink)");
    // The chosen tab's rule comes later at the same weight, so under the pointer it keeps its fill, never darker.
    expect(globals.indexOf('.tab[aria-selected="true"]')).toBeGreaterThan(
      globals.indexOf(".tab:hover"),
    );
  });

  it("makes a tab at least 32 px, 44 px on touch screens, its label centred, tabs 4 px apart", () => {
    const tab = rule(globals, ".tab");
    expect(declared(tab, "min-height")).toBe("var(--tab-height)");
    expect(declared(tab, "padding")).toBe("0 var(--nav-pad-inline)");
    expect(declared(tab, "align-items")).toBe("center");
    expect(declared(tab, "justify-content")).toBe("center");
    expect(declared(tokens, "--tab-height")).toBe("var(--min-control-height)");
    expect(declared(tokens, "--min-control-height")).toBe("32px");
    expect(declared(touch, "--tab-height")).toBe("var(--row-height-touch)");
    expect(declared(rule(globals, ".tab-list"), "gap")).toBe("var(--space-1)");
  });

  it("scrolls a strip wider than its place inside itself, with room for the one focus ring", () => {
    const list = rule(globals, ".tab-list");
    expect(declared(list, "display")).toBe("flex");
    expect(declared(list, "min-width")).toBe("0");
    expect(declared(list, "overflow-x")).toBe("auto");
    const tab = rule(globals, ".tab");
    expect(declared(tab, "flex")).toBe("none");
    expect(declared(tab, "white-space")).toBe("nowrap");
    // The ring is the app's one ring (WEB-U-089), 2 px at a 2 px offset; the strip keeps that much room around its
    // tabs, which its margin takes back so they stay in place.
    expect(declared(tokens, "--focus-reach")).toBe(
      "calc(var(--focus-width) + var(--focus-offset))",
    );
    expect(declared(list, "padding")).toBe("var(--focus-reach)");
    expect(declared(list, "margin")).toBe("calc(-1 * var(--focus-reach))");
    expect(rule(globals, ".tab:focus-visible")).toBe("");
  });

  it("is the only tab style: no underline or bold, no tab rules in another sheet, the old ones gone", () => {
    const tabRules = selectors.filter(({ selector }) =>
      /\.tab(-list)?(?![\w-])/.test(selector),
    );
    expect(tabRules.length).toBeGreaterThan(0);
    expect(tabRules.filter(({ file }) => file !== "globals.css")).toEqual([]);
    for (const { selector } of tabRules) {
      const block = rule(globals, selector);
      expect(declared(block, "border-bottom")).toBe(undefined);
      expect(declared(block, "text-decoration") ?? "none").not.toMatch(
        /underline/,
      );
      expect(declared(block, "font-weight") ?? "400").toBe("400");
    }
    expect(
      selectors.filter(({ selector }) =>
        /\.(files-tab|channel-tabs?|keys-views)(?![\w-])/.test(selector),
      ),
    ).toEqual([]);
    // The Files panel's strip keeps only its place in the panel's heading: the width left beside its buttons.
    const place = rule(read("files.css"), ".files-tabs");
    expect(
      [...place.matchAll(/([\w-]+)\s*:/g)].map((match) => match[1]),
    ).toEqual(["flex"]);
  });
});
