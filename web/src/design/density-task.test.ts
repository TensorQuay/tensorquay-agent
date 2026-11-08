// @vitest-environment node
// WEB-U-080 on the task side: the sidebar's tasks and the task page's lists follow the density tokens (UI-GUIDELINES
// §5c): 32 px rows with 14/20 text, 2 px apart, 12 px sans group labels 4 px above their rows, and a fill-only active
// task, as the Settings menu does.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { DOCK_WIDTH } from "../files/use-task-layout";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const conversation = read("conversation.css");
const files = read("files.css");
const globals = read("globals.css");
const teams = read("teams.css");

/** The declarations of every rule whose selector list names `selector` exactly, top level or nested, joined. */
function rule(css: string, selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
/** The last value a rule set gives `property`, as the cascade would. */
const declared = (block: string, property: string) =>
  [...block.matchAll(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`, "g"))]
    .at(-1)?.[1]
    ?.trim();

describe("WEB-U-080 the sidebar's tasks", () => {
  it("makes a task row 32 px with 14/20 text from the tokens", () => {
    const link = rule(conversation, ".conversation-link");
    expect(declared(link, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(link, "padding")).toBe("var(--nav-row-padding)");
    expect(declared(link, "font-size")).toBe("var(--nav-font-size)");
    expect(declared(link, "line-height")).toBe("var(--nav-line-height)");
  });

  it("marks the open task and the open helper with the selected fill at the body weight, no accent bar", () => {
    const active = rule(
      conversation,
      '.conversation-row:has([aria-current="page"])',
    );
    expect(declared(active, "background")).toBe("var(--selected-fill)");
    expect(declared(active, "font-weight")).toBe(undefined);
    const helper = rule(teams, '.task-helper:has([aria-current="page"])');
    expect(declared(helper, "background")).toBe("var(--selected-fill)");
    expect(declared(helper, "font-weight")).toBe(undefined);
    // Under the pointer it keeps its fill (never darker) and gains no underline.
    expect(
      declared(
        rule(conversation, '.conversation-link[aria-current="page"]:hover'),
        "text-decoration",
      ),
    ).toBe("none");
    expect(conversation).not.toMatch(/border-left[^;]*var\(--accent\)/);
    expect(
      declared(rule(conversation, ".conversation-row"), "border-left"),
    ).toBe(undefined);
  });

  it("spaces rows 2 px apart, groups 16 px apart, and a label 4 px above its rows", () => {
    expect(
      declared(rule(conversation, ".task-group > .conversation-rows"), "gap"),
    ).toBe("var(--nav-row-gap)");
    expect(
      declared(rule(conversation, ".task-group + .task-group"), "margin-top"),
    ).toBe("var(--group-gap)");
    // The group holds its label and its rows: its own gap is the space under the label.
    expect(declared(rule(conversation, ".task-group"), "gap")).toBe(
      "var(--group-label-gap)",
    );
  });
});

describe("WEB-U-080 the task page's lists", () => {
  it("makes a file or change row 32 px with 14 px text", () => {
    const row = rule(files, ".file-open");
    expect(declared(row, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(row, "padding")).toBe("0 var(--nav-pad-inline)");
    expect(declared(row, "font-size")).toBe("var(--nav-font-size)");
  });

  it("makes a step's line 32 px with 14/20 text", () => {
    const line = rule(conversation, ".activity-toggle");
    expect(declared(line, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(line, "padding-block")).toBe("var(--nav-pad-block)");
    expect(declared(line, "font-size")).toBe("var(--nav-font-size)");
    expect(declared(line, "line-height")).toBe("var(--nav-line-height)");
  });
});

// The live review of c7 (L1-L2): every task row has one layout, whatever its label: the title in sans on one line with
// an ellipsis, the muted time right-aligned after it, and the status mark in its own space at the end. "Show more" is a
// small ghost button (17.3, UI-GUIDELINES §7 Conversation list item), so it reads as an action, never as a muted task.
describe("WEB-U-070 one task row layout", () => {
  it("lets the title take the room and cut with an ellipsis, in every list", () => {
    const text = rule(conversation, ".conversation-link-text");
    expect(declared(text, "flex")).toBe("1");
    expect(declared(text, "min-width")).toBe("0");
    expect(declared(text, "flex-direction")).toBe("row");
    const title = rule(conversation, ".task-title");
    expect(declared(title, "flex")).toBe("1");
    expect(declared(title, "min-width")).toBe("0");
    expect(declared(title, "text-overflow")).toBe("ellipsis");
    expect(declared(title, "white-space")).toBe("nowrap");
  });

  it("keeps the status whole, in its own space after the title", () => {
    // No time on the row (the sidebar design, WEB-U-070): its tooltip holds it.
    expect(conversation).not.toMatch(/\.task-when/);
    const status = rule(conversation, ".task-status");
    expect(declared(status, "flex-shrink")).toBe("0");
    expect(declared(status, "margin-left")).not.toBe("auto");
    expect(declared(status, "position")).toBe(undefined);
  });

  it("makes Show more a small ghost button at its own width, in the list's text size", () => {
    // Compound, so it outweighs the button's own rule: §5b's small button, 28 px with 12 px each side.
    const more = rule(conversation, ".button.task-more-button");
    expect(declared(more, "height")).toBe("var(--min-control-height)");
    expect(declared(more, "min-height")).toBe("var(--min-control-height)");
    expect(declared(more, "padding")).toBe("0 var(--space-3)");
    expect(declared(more, "align-self")).toBe("flex-start");
    expect(declared(more, "font-size")).toBe("var(--nav-font-size)");
    // The ghost button's own ink and weight: no muted colour or body weight of a row.
    expect(declared(more, "color")).toBe(undefined);
    expect(declared(more, "font-weight")).toBe(undefined);
  });
});

// The review of 7e282fc0 (SHOULD 1): at phone width the drawer was 260 px, so a row's title and time left too little of
// the title to tell two tasks apart. The drawer is wider on narrow screens, never wider than the screen allows.
describe("WEB-U-070 the drawer at phone width", () => {
  // At 390 px (WEB-U-070): the drawer's rows showed the task icon, which the expanded sidebar hides; it took title room.
  it("shows no task icon in the drawer, as in the expanded sidebar", () => {
    expect(
      declared(rule(conversation, ".navigation-drawer .task-icon"), "display"),
    ).toBe("none");
  });

  it("is up to 320 px wide, and at most 85% of the screen", () => {
    expect(declared(rule(globals, ":root"), "--drawer-width")).toBe("320px");
    // The drawer is the sheet at the start edge (WEB-U-230); its width rule moved there, unchanged.
    expect(
      declared(rule(globals, '.dialog-sheet[data-placement="start"]'), "width"),
    ).toBe("min(var(--drawer-width), 85vw)");
  });
});

// The sidebar design (WEB-U-080): one left edge. The date groups' headings start where the task titles and the nav icons do,
// at the row's inner padding, as in the familiar chat apps.
describe("WEB-U-080 one left edge in the sidebar", () => {
  it("indents a group heading by the rows' inner padding", () => {
    expect(
      declared(rule(conversation, ".sidebar-label"), "padding-inline-start"),
    ).toBe("var(--nav-pad-inline)");
    expect(declared(rule(conversation, ".conversation-link"), "padding")).toBe(
      "var(--nav-row-padding)",
    );
  });
});

// The UX review of the dev tip (WEB-U-022): at 1280 the floating panel covered the answer mid-word; it docked from
// 1280 px. The dev screen review (011e8972), #1: at 1024 it still covered the person's own messages. It docks from
// 1024 px (WEB-U-170), the conversation narrowing beside it; below that it takes the whole column, never part of it,
// and starts where the header ends (WEB-U-171).
describe("WEB-U-170 the files panel never covers part of the transcript", () => {
  // The fix's review: by the layout's own width (a container), not the window's, so the sidebar's width counts.
  const narrow =
    new RegExp(
      `@container conversation-layout \\(max-width: ${DOCK_WIDTH - 1}px\\) \\{\\s*\\.files-panel \\{([^}]*)\\}`,
    ).exec(files)?.[1] ?? "";
  it("docks where the layout has room, and otherwise takes the whole column", () => {
    expect(declared(rule(files, ".conversation-layout"), "container")).toBe(
      "conversation-layout / inline-size",
    );
    expect(files).not.toMatch(/max-width: 1023px/);
    expect(declared(narrow, "position")).toBe("absolute");
    expect(declared(narrow, "left")).toBe("0");
    expect(declared(narrow, "right")).toBe("0");
    expect(files).not.toMatch(/1279px|1439px/);
  });

  it("WEB-U-171 starts where the header ends", () => {
    expect(declared(narrow, "top")).toBe(
      "var(--bar-offset, var(--bar-height))",
    );
  });
});
