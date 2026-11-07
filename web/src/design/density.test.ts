// @vitest-environment node
// WEB-U-080: one density for navigation lists, menus and tables, set once as tokens (UI-GUIDELINES §5c): 32 px nav
// rows, 12 px sans labels, a fill-only active item, 40 px table rows, 32 px menu items, and 44 px on touch screens.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const globals = read("globals.css");
const settings = read("settings.css");
const menu = read("menu.css");
const workspaces = read("workspaces.css");
const all = [globals, settings, menu].join("\n");

/** The declarations of every rule whose selector list names `selector` exactly, top level or nested, joined. */
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
const root = rule(globals, ":root");

describe("WEB-U-080 density tokens", () => {
  it.each([
    ["row-height-nav", "32px"],
    ["row-height-table", "40px"],
    ["row-height-touch", "44px"],
    ["label-size", "12px"],
    ["nav-font-size", "14px"],
    ["nav-line-height", "20px"],
    // On the §5 scale (the workspaces audit of §5b): the row's 32 px comes from its min-height.
    ["nav-pad-block", "var(--space-1)"],
    ["nav-pad-inline", "var(--space-3)"],
    ["nav-row-padding", "var(--nav-pad-block) var(--nav-pad-inline)"],
    ["space-half", "2px"],
    ["nav-row-gap", "var(--space-half)"],
    ["space-4", "16px"],
    ["group-gap", "var(--space-4)"],
    ["space-1", "4px"],
    ["group-label-gap", "var(--space-1)"],
    ["heading-line-height", "28px"],
    // The sign-in pages' wordmark, smaller (the design direction: the same form, a quieter mark).
    ["wordmark-height", "24px"],
  ])("sets --%s to %s", (name, value) => {
    expect(declared(root, `--${name}`)).toBe(value);
  });

  it("grows nav and table rows to the touch height on coarse pointers", () => {
    const touch =
      globals.match(
        /@media \(pointer: coarse\)\s*\{\s*:root\s*\{([^}]*)\}/,
      )?.[1] ?? "";
    expect(declared(touch, "--row-height-nav")).toBe("var(--row-height-touch)");
    expect(declared(touch, "--row-height-table")).toBe(
      "var(--row-height-touch)",
    );
  });
});

describe("WEB-U-080 labels, rows and the active item", () => {
  it("sets labels in the sans face at 12 px, weight 500, never monospace", () => {
    const label = rule(globals, ".type-label");
    expect(declared(label, "font-family")).toBe("var(--font-ui)");
    expect(declared(label, "font-size")).toBe("var(--label-size)");
    expect(declared(label, "font-weight")).toBe("500");
  });

  it("makes navigation rows 32 px with 14/20 text from the tokens", () => {
    const item = rule(globals, ".nav-item");
    expect(declared(item, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(item, "padding")).toBe("var(--nav-row-padding)");
    expect(declared(item, "font-size")).toBe("var(--nav-font-size)");
    expect(declared(item, "line-height")).toBe("var(--nav-line-height)");
  });

  it("marks the active item with a fill and weight 500 only, no accent bar", () => {
    const active = rule(globals, '.nav-item[aria-current="page"]');
    expect(declared(active, "background")).toBe("var(--surface)");
    expect(declared(active, "font-weight")).toBe("500");
    expect(all).not.toMatch(/\.nav-item\[aria-current="page"\]::before/);
  });

  it("spaces the Settings menu's groups 16 px apart, rows 2 px apart", () => {
    expect(declared(rule(settings, ".settings-nav"), "gap")).toBe(
      "var(--group-gap)",
    );
    expect(declared(rule(settings, ".settings-group"), "gap")).toBe(
      "var(--nav-row-gap)",
    );
    expect(
      declared(rule(settings, ".settings-group-label"), "margin-bottom"),
    ).toBe("var(--group-label-gap)");
  });

  it("makes table rows 40 px with 8 px vertical cell padding", () => {
    const cell = rule(settings, ".data-table td");
    expect(declared(cell, "height")).toBe("var(--row-height-table)");
    expect(declared(cell, "padding")).toBe("var(--space-2)");
    expect(globals).not.toMatch(/--table-row-height/);
  });

  it("makes the Overview's summary lines 40 px table rows, with no gap between them", () => {
    const line = rule(settings, ".overview-line");
    expect(declared(line, "min-height")).toBe("var(--row-height-table)");
    expect(declared(line, "padding-block")).toBe("var(--space-2)");
    expect(declared(line, "align-items")).toBe("center");
    expect(declared(rule(settings, ".overview-lines"), "gap")).toBe("0");
  });

  it("makes menu items 32 px with 14 px text", () => {
    const item = rule(menu, ".menu-item");
    expect(declared(item, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(item, "font-size")).toBe("var(--nav-font-size)");
  });

  it("lets a workspace's name take the free width, and makes the filters a light segmented control", () => {
    expect(
      declared(rule(workspaces, ".workspace-table .data-name"), "max-width"),
    ).toBe("none");
    const control = rule(globals, ".segmented");
    expect(declared(control, "background")).toBe("var(--surface)");
    expect(declared(control, "border-radius")).toBe("var(--control-radius)");
    const pressed = rule(globals, '.segmented .button[aria-pressed="true"]');
    expect(declared(pressed, "background")).toBe("var(--paper)");
    expect(declared(pressed, "font-weight")).toBe("500");
    // The page's primary button stays, at a nav row's height.
    expect(declared(rule(workspaces, ".workspaces-new"), "min-height")).toBe(
      "var(--row-height-nav)",
    );
  });

  it("keeps the sign-in wordmark small, close above the form", () => {
    expect(
      declared(rule(globals, ".auth-column .brand-wordmark"), "margin"),
    ).toBe("var(--space-2) 0");
  });

  it("sets the Settings heading at 20/28", () => {
    const title = rule(settings, ".settings-title h1");
    expect(declared(title, "font-size")).toBe("var(--heading-size)");
    expect(declared(title, "line-height")).toBe("var(--heading-line-height)");
  });
});
