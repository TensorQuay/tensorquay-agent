// @vitest-environment node
// WEB-U-080: one density for navigation lists, menus and tables, set once as tokens (UI-GUIDELINES §5c): 36 px nav
// rows, 13 px sans labels, a fill-only active item, 40 px table rows, 36 px menu items, and 44 px on touch screens.
import { readdirSync, readFileSync } from "node:fs";

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
/** `share` of the `ink` colour over the `paper` colour, both #rrggbb, as #rrggbb (each channel rounded). */
function over(ink = "", paper = "", share = 0) {
  const channels = (hex: string) =>
    [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16));
  const [top, base] = [channels(ink), channels(paper)];
  return `#${base
    .map((channel, index) =>
      Math.round(channel + ((top[index] ?? 0) - channel) * share)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

describe("WEB-U-080 density tokens", () => {
  it.each([
    ["row-height-nav", "36px"],
    ["row-height-table", "40px"],
    ["row-height-touch", "44px"],
    ["label-size", "13px"],
    ["nav-font-size", "15px"],
    ["nav-line-height", "22px"],
    // On the §5 scale (the workspaces audit of §5b): the row's 36 px comes from its min-height.
    ["nav-pad-block", "var(--space-1)"],
    ["nav-pad-inline", "var(--space-3)"],
    ["nav-row-padding", "var(--nav-pad-block) var(--nav-pad-inline)"],
    ["space-half", "2px"],
    ["nav-row-gap", "var(--space-half)"],
    ["space-4", "16px"],
    ["group-gap", "var(--space-4)"],
    ["space-1", "4px"],
    ["group-label-gap", "var(--space-1)"],
    ["heading-line-height", "30px"],
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
  it("sets labels in the sans face at 13 px, weight 400, never monospace", () => {
    const label = rule(globals, ".type-label");
    expect(declared(label, "font-family")).toBe("var(--font-ui)");
    expect(declared(label, "font-size")).toBe("var(--label-size)");
    // The app ships Space Grotesk at 400 and 700 only (§4): a label is 400, as it always drew.
    expect(declared(label, "font-weight")).toBe("400");
  });

  it("makes navigation rows 36 px with 15/22 text from the tokens", () => {
    const item = rule(globals, ".nav-item");
    expect(declared(item, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(item, "padding")).toBe("var(--nav-row-padding)");
    expect(declared(item, "font-size")).toBe("var(--nav-font-size)");
    expect(declared(item, "line-height")).toBe("var(--nav-line-height)");
  });

  // 17.3: one selected fill, 8 % ink over paper, so the open item reads at a glance; the body weight (the app ships
  // Space Grotesk at 400 and 700 only); the hover tint is the lighter step, and the selected row keeps its fill under it.
  it("marks the active item with the selected fill at the body weight, no accent bar", () => {
    const active = rule(globals, '.nav-item[aria-current="page"]');
    expect(declared(active, "background")).toBe("var(--selected-fill)");
    expect(declared(active, "font-weight")).toBe(undefined);
    expect(
      declared(
        rule(globals, '.nav-item[aria-current="page"]:hover'),
        "background",
      ),
    ).toBe("var(--selected-fill)");
    expect(all).not.toMatch(/\.nav-item\[aria-current="page"\]::before/);
  });

  it("sets the selected fill to 8 % ink over paper in either theme", () => {
    const dark = rule(globals, '[data-theme="dark"]');
    const system = rule(globals, ':root:not([data-theme="light"])');
    for (const theme of [root, dark, system])
      expect(declared(theme, "--selected-fill")).toBe(
        over(declared(theme, "--ink"), declared(theme, "--paper"), 0.08),
      );
  });

  it("gives the Settings menu's active item the selected fill, kept under the pointer", () => {
    for (const selector of [
      '.settings-nav .nav-item[aria-current="page"]',
      '.settings-nav .nav-item[aria-current="page"]:hover',
    ])
      expect(declared(rule(settings, selector), "background")).toBe(
        "var(--selected-fill)",
      );
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

  it("makes menu items 36 px with 15 px text", () => {
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
    // The chosen segment keeps the segments' body weight: its paper fill marks it (§4: 400 and 700 only).
    expect(declared(pressed, "font-weight")).toBe(undefined);
    expect(declared(rule(globals, ".segmented .button"), "font-weight")).toBe(
      "400",
    );
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

  it("sets the Settings heading at 22/30", () => {
    const title = rule(settings, ".settings-title h1");
    expect(declared(title, "font-size")).toBe("var(--heading-size)");
    expect(declared(title, "line-height")).toBe("var(--heading-line-height)");
  });
});

// 17.3: the app ships Space Grotesk at 400 and 700 only (§4), so a stylesheet asks for one of the two; any other weight
// draws as one of them anyway (500 as 400, 600 as 700), and the rule would say what the screen doesn't show.
describe("WEB-U-448 the weights the app ships", () => {
  it("asks only for 400 or 700 in every stylesheet", () => {
    const asked = readdirSync("src/app")
      .filter((file) => file.endsWith(".css"))
      .flatMap((file) =>
        [...read(file).matchAll(/font-weight:\s*([^;}]+)/g)].map(
          (match) => `${file}: ${(match[1] ?? "").trim()}`,
        ),
      );
    expect(asked.length).toBeGreaterThan(0);
    expect(
      asked.filter((weight) => !/: (400|700|inherit)$/.test(weight)),
    ).toEqual([]);
  });
});

// WEB-U-451 (UI-GUIDELINES §4 and §5c; the owner's ask, 9 Nov 2026: "can we adjust all system wide ... bigger"): the
// whole app one step bigger, set once in the tokens: body 16/24, small 14, the navigation 15/22, labels 13, page
// headings 22/30, a task's title 18, display 30; controls at least 32 px; list and menu rows 36 px; two-line table rows 68 px. Every
// stylesheet takes its sizes and lines from the tokens, so the whole app follows them.
describe("WEB-U-451 one type scale, one step up", () => {
  it.each([
    ["display-size", "30px"],
    ["heading-size", "22px"],
    ["heading-line-height", "30px"],
    ["task-title-size", "18px"],
    ["body-size", "16px"],
    ["body-leading", "1.5"],
    ["small-size", "14px"],
    ["small-leading", "1.5"],
    ["label-size", "13px"],
    ["nav-font-size", "15px"],
    ["nav-line-height", "22px"],
    ["compact-leading", "1.2"],
    ["min-control-height", "32px"],
    ["row-height-nav", "36px"],
    ["pill-height", "22px"],
    ["row-height-table-two", "68px"],
  ])("sets --%s to %s", (name, value) => {
    expect(declared(root, `--${name}`)).toBe(value);
  });

  it("states the scale on the design reference as the tokens set it", () => {
    const reference = readFileSync("src/design/TypeAndSpace.tsx", "utf8");
    const stated = (role: string) =>
      new RegExp(`"${role} · (\\d+) / ([\\d.]+)"`).exec(reference)?.slice(1);
    const token = (name: string) =>
      declared(root, `--${name}`)?.replace(/px$/, "");
    expect(stated("Display")).toEqual([
      token("display-size"),
      token("display-leading"),
    ]);
    expect(stated("Heading")).toEqual([
      token("heading-size"),
      token("heading-leading"),
    ]);
    expect(stated("Body")).toEqual([token("body-size"), token("body-leading")]);
    expect(stated("Small")).toEqual([
      token("small-size"),
      token("small-leading"),
    ]);
    // Labels take the body's leading (the theme's --text-label--line-height); code is set at the small size.
    expect(stated("Label")).toEqual([
      token("label-size"),
      token("body-leading"),
    ]);
    expect(stated("Code")).toEqual([
      token("small-size"),
      token("code-leading"),
    ]);
  });

  it("takes every font size and line height from a token", () => {
    const loose = readdirSync("src/app")
      .filter((file) => file.endsWith(".css"))
      .flatMap((file) =>
        [
          ...read(file).matchAll(
            /(?<![\w-])(font-size|line-height)\s*:\s*([^;}]+)/g,
          ),
        ]
          .map((match) => `${file}: ${match[1]}: ${match[2]?.trim()}`)
          // pdf.js lays its text layer out at line height 1, to sit on the page's own words (files.css).
          .filter((found) => !found.startsWith("files.css: line-height: 1")),
      )
      .filter((found) => !/: (var\(--|calc\(|inherit|normal)/.test(found));
    expect(loose).toEqual([]);
  });
});
