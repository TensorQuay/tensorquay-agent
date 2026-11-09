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
    // Under the pointer it keeps its fill (never darker) and gains no underline: no row's title underlines on hover,
    // since the row's backdrop shows it (§7 Conversation list item).
    expect(conversation).not.toMatch(
      /\.conversation-link[^{}]*:hover[^{}]*\{[^}]*underline/,
    );
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

// A live check before 17.0: every task row has one layout, whatever its label: the title in sans on one line with
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

// At phone width the drawer was 260 px, so a row's title and time left too little of the title to tell two tasks apart.
// The drawer is wider on narrow screens, never wider than the screen allows.
describe("WEB-U-070 the drawer at phone width", () => {
  // At 390 px (WEB-U-070): the drawer's rows showed the task icon, which the expanded sidebar hides; it took title room.
  it("shows no task icon in the drawer, as in the expanded sidebar", () => {
    // Hidden, keeping its column, so the titles start where the top items' labels do (WEB-U-453).
    expect(
      declared(
        rule(conversation, ".navigation-drawer .task-icon"),
        "visibility",
      ),
    ).toBe("hidden");
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

// WEB-U-022: at 1280 the floating panel covered the answer mid-word; it docked from 1280 px. A screen review
// before 17.0: at 1024 it still covered the person's own messages. It docks from 1024 px (WEB-U-170), the conversation
// narrowing beside it; below that it takes the whole column, never part of it, and starts where the header ends
// (WEB-U-171).
describe("WEB-U-170 the files panel never covers part of the transcript", () => {
  // By the layout's own width (a container), not the window's, so the sidebar's width counts.
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

/**
 * `rule` for selectors with commas or line breaks of their own (`:where(a, b)`, a long list the formatter wraps): the
 * list is split only at its top-level commas, and selectors and declarations are read with their whitespace collapsed.
 */
function ruleFlat(css: string, selector: string): string {
  const flat = (text: string) =>
    text.replace(/\s+/g, " ").replace(/\( /g, "(").replace(/ \)/g, ")").trim();
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const parts = [""];
    let depth = 0;
    for (const char of match[1] ?? "") {
      if (char === "(") depth += 1;
      if (char === ")") depth -= 1;
      if (char === "," && depth === 0) parts.push("");
      else parts[parts.length - 1] += char;
    }
    if (parts.map(flat).includes(flat(selector)))
      found.push(flat(match[2] ?? ""));
  }
  return found.join(";");
}

/** The bodies of every `@media <query>` block in `css`, braces balanced, joined. */
function media(css: string, query: string): string {
  const bodies: string[] = [];
  let from = css.indexOf(`@media ${query} {`);
  while (from >= 0) {
    const open = css.indexOf("{", from);
    let depth = 0;
    let index = open;
    for (; index < css.length; index += 1) {
      if (css[index] === "{") depth += 1;
      if (css[index] === "}") depth -= 1;
      if (depth === 0) break;
    }
    bodies.push(css.slice(open + 1, index));
    from = css.indexOf(`@media ${query} {`, index);
  }
  return bodies.join("\n");
}

// WEB-U-449 (UI-GUIDELINES §7 Conversation list item): a task row's ⋯ is part of the row, never a chip over it. It sits
// inside the row's end at one even inset, square, its corners concentric with the row's, with no fill of its own; its
// hover and press are the quiet tints inside the row's backdrop; what lies under it gives way while it shows; on a
// touch screen, where it always shows, the title ends before it.
describe("WEB-U-449 the task row's ⋯ is part of the row", () => {
  const tokens = rule(globals, ":root");
  const holder = ruleFlat(conversation, ".conversation-row > .task-more");
  const button = ruleFlat(
    conversation,
    ".conversation-row > .task-more > .button",
  );
  const showing = [
    ".conversation-row:has(> .task-more):hover",
    ".conversation-row:has(> .task-more):has(> :focus-visible, > .task-more :focus-visible)",
    '.conversation-row:has(> .task-more [aria-expanded="true"])',
  ];

  it("sits inside the row's end at one even inset, square, its corners concentric with the row's", () => {
    expect(declared(tokens, "--row-inset")).toBe("var(--space-half)");
    expect(declared(tokens, "--row-action-size")).toBe(
      "calc(var(--row-height-nav) - 2 * var(--row-inset))",
    );
    expect(declared(tokens, "--row-action-radius")).toBe(
      "calc(var(--control-radius) - var(--row-inset))",
    );
    expect(declared(holder, "position")).toBe("absolute");
    expect(declared(holder, "top")).toBe("var(--row-inset)");
    expect(declared(holder, "right")).toBe("var(--row-inset)");
    // On the row's own line: an open lead's ⋯ stays by its title, never half-way down its helpers.
    expect(declared(holder, "transform")).toBe(undefined);
    expect(declared(button, "width")).toBe("var(--row-action-size)");
    expect(declared(button, "height")).toBe("var(--row-action-size)");
    expect(declared(button, "border-radius")).toBe("var(--row-action-radius)");
  });

  it("has no fill of its own: its hover and press are the quiet tints inside the row's backdrop", () => {
    expect(declared(button, "background")).toBe("transparent");
    expect(
      declared(
        ruleFlat(
          conversation,
          ".conversation-row > .task-more > .button:hover:not(:disabled)",
        ),
        "background",
      ),
    ).toBe("var(--hover-tint)");
    expect(
      declared(
        ruleFlat(
          conversation,
          ".conversation-row > .task-more > .button:active:not(:disabled)",
        ),
        "background",
      ),
    ).toBe("var(--press-tint)");
    expect(conversation).not.toMatch(/task-more[^{}]*\{[^}]*--surface/);
  });

  it("shows it for the row's own keyboard focus, never for a focus a click left behind", () => {
    // The owner's report (two rows with a ⋯ at once): a clicked link keeps its focus, so focus-within kept the ⋯.
    expect(
      declared(
        ruleFlat(
          conversation,
          ".conversation-row:has(> :focus-visible, > .task-more :focus-visible) > .task-more > .button",
        ),
        "opacity",
      ),
    ).toBe("1");
    expect(conversation).not.toMatch(
      /\.conversation-row:focus-within[^{}]*task-more/,
    );
    expect(conversation).not.toMatch(/task-more\):focus-within/);
    expect(globals).not.toMatch(/task-more\):focus-within/);
  });

  it("keeps the row's backdrop while it shows by keyboard or for its open menu, below the open task's fill", () => {
    // :where() keeps these as light as the hover rule, so the open task's selected fill, set after, stays.
    expect(
      declared(
        ruleFlat(
          conversation,
          '.conversation-row:where(:has(> :focus-visible, > .task-more :focus-visible), :has(> .task-more [aria-expanded="true"]))',
        ),
        "background",
      ),
    ).toBe("var(--hover-tint)");
  });

  it("fades out what lies under it while it shows: the row's marks, and a title that reaches it", () => {
    const hover = media(conversation, "(hover: hover)");
    for (const state of showing) {
      expect(
        declared(
          ruleFlat(hover, `${state} > .conversation-link > .task-mark`),
          "opacity",
        ),
      ).toBe("0");
      expect(
        declared(
          ruleFlat(
            hover,
            `${state} > .conversation-link:not(:has(> .task-mark)) > .conversation-link-text`,
          ),
          "mask-image",
        ),
      ).toBe(
        "linear-gradient(to left, transparent var(--row-action-reach), var(--ink) calc(var(--row-action-reach) + var(--space-4)))",
      );
    }
    // The fade starts where the ⋯ and its inset reach past the row's own end padding.
    expect(
      declared(tokens, "--row-action-reach")
        ?.replace(/\s+/g, " ")
        .replace(/\( /g, "(")
        .replace(/ \)/g, ")"),
    ).toBe(
      "calc(var(--row-action-size) + var(--row-inset) - var(--nav-pad-inline))",
    );
    // Without hover, titles keep the full width: no mask anywhere else.
    expect(conversation.replace(hover, "")).not.toMatch(/mask-image/);
  });

  it("always shows it on a touch screen, where the title and the marks end before it", () => {
    const touch = media(conversation, "(hover: none)");
    expect(
      declared(
        ruleFlat(touch, ".conversation-row > .task-more > .button"),
        "opacity",
      ),
    ).toBe("1");
    expect(
      declared(
        ruleFlat(
          touch,
          ".conversation-row:has(> .task-more) > .conversation-link",
        ),
        "padding-inline-end",
      ),
    ).toBe("calc(var(--row-action-size) + var(--row-inset) + var(--space-2))");
  });
});

// WEB-U-450 (UI-GUIDELINES §13b and §6): a lead task's row reads as one row. Its chevron spans the row's start padding
// and its icon column, its icon on the top items' icon column, as tall as the row (44 px on touch screens); its title
// starts where every task title does; its helpers' rows start where its title starts.
describe("WEB-U-450 a lead's row reads as one row", () => {
  const flat = (value: string | undefined) =>
    value?.replace(/\s+/g, " ").replace(/\( /g, "(").replace(/ \)/g, ")");

  it("makes the chevron the row's start padding and its icon column, as tall as the row", () => {
    const base = /:root \{([^}]*)\}/.exec(globals)?.[1] ?? "";
    expect(flat(declared(base, "--lead-toggle-width"))).toBe(
      "calc(var(--nav-pad-inline) + var(--icon-text))",
    );
    // One width everywhere, so the lead's title keeps the titles' edge on touch screens too.
    expect(globals).not.toMatch(
      /--lead-toggle-width: var\(--row-height-touch\)/,
    );
    const toggle = rule(teams, ".task-helpers-toggle");
    expect(declared(toggle, "width")).toBe("var(--lead-toggle-width)");
    expect(declared(toggle, "min-height")).toBe("var(--row-height-nav)");
    expect(declared(toggle, "justify-content")).toBe("flex-end");
    expect(declared(toggle, "padding-inline-end")).toBe(undefined);
  });

  it("starts the lead's title where every task title does: no gap, then the icon's 12 px", () => {
    expect(
      declared(
        rule(teams, ".conversation-row:has(> .task-helpers-toggle)"),
        "column-gap",
      ),
    ).toBe("0");
    expect(
      declared(
        rule(
          teams,
          ".conversation-row:has(> .task-helpers-toggle) > .conversation-link",
        ),
        "padding-inline-start",
      ),
    ).toBe("var(--space-3)");
  });

  it("starts the helpers' rows where the lead's title starts, on the spacing scale", () => {
    expect(declared(rule(teams, ".task-helpers"), "padding-inline-start")).toBe(
      "var(--space-6)",
    );
    expect(
      declared(
        rule(teams, ".conversation-row > .task-helpers .conversation-link"),
        "padding-inline-start",
      ),
    ).toBe("var(--space-4)");
    const note = rule(teams, ".task-helpers-note");
    expect(declared(note, "padding-inline-start")).toBe("var(--space-6)");
    expect(declared(note, "margin-inline-start")).toBe("var(--space-4)");
  });
});

// WEB-U-453 (UI-GUIDELINES §6; the owner, 9 Nov 2026, against a reference): the sidebar reads as one tidy list. One left
// edge for the logo, every item's icon and the group labels; the task titles start where the top items' labels do; the
// rows at the scale's height, the top block's 2 px apart, 8 px under the header, one group gap before the first group;
// the drawer's rows inset as the sidebar's.
describe("WEB-U-453 the sidebar's one left edge and rhythm", () => {
  it("puts the logo on the items' icon column, 8 px above the top block", () => {
    // The top-level rule; the rail's own (a column, centred) sets it back to 0.
    const brand = /\n\.sidebar-brand \{([^}]*)\}/.exec(globals)?.[1] ?? "";
    expect(declared(brand, "padding-inline-start")).toBe(
      "var(--nav-pad-inline)",
    );
    expect(declared(rule(globals, ".app-sidebar"), "gap")).toBe(
      "var(--space-2)",
    );
    // The group labels start on the same column (unchanged).
    expect(
      declared(rule(conversation, ".sidebar-label"), "padding-inline-start"),
    ).toBe("var(--nav-pad-inline)");
  });

  it("starts the task titles where the top items' labels do: a row keeps its hidden icon's column", () => {
    // A top item: the rows' padding, its 16 px icon, a 12 px gap, its label. A task row: the same padding and gap, its
    // icon hidden in place (never removed), its title; so both start on one edge, every value on the spacing scale.
    expect(declared(rule(globals, ".nav-item"), "gap")).toBe("var(--space-3)");
    expect(declared(rule(globals, ".nav-item"), "padding")).toBe(
      "var(--nav-row-padding)",
    );
    expect(declared(rule(conversation, ".conversation-link"), "gap")).toBe(
      "var(--space-3)",
    );
    expect(declared(rule(conversation, ".conversation-link"), "padding")).toBe(
      "var(--nav-row-padding)",
    );
    expect(
      declared(rule(conversation, ".app-sidebar .task-icon"), "visibility"),
    ).toBe("hidden");
    expect(
      declared(rule(conversation, ".app-sidebar .task-icon"), "display"),
    ).toBe(undefined);
    // A lead's chevron takes the icon column's place.
    expect(
      declared(
        ruleFlat(
          conversation,
          ".conversation-row:has(> .task-helpers-toggle) > .conversation-link > .task-icon",
        ),
        "display",
      ),
    ).toBe("none");
    // Show more's label starts on the same edge: a hidden icon's width, then the rows' gap.
    expect(declared(rule(conversation, ".task-more-slot"), "width")).toBe(
      "var(--icon-text)",
    );
    expect(
      declared(rule(conversation, ".task-more-button .button-label"), "gap"),
    ).toBe("var(--space-3)");
  });

  it("spaces the top block's rows 2 px apart and the task list one group gap below them", () => {
    expect(declared(rule(globals, ".navigation"), "gap")).toBe(undefined);
    expect(declared(rule(globals, ".nav-items"), "gap")).toBe(
      "var(--nav-row-gap)",
    );
    expect(
      declared(rule(conversation, ".conversation-list"), "margin-block"),
    ).toBe("var(--group-gap) var(--space-4)");
    expect(globals).not.toMatch(/\.sidebar-footer/);
  });

  it("insets the drawer's rows as the sidebar's, 8 px under its header", () => {
    expect(
      declared(rule(globals, ".dialog.navigation-drawer"), "padding-inline"),
    ).toBe("var(--space-3)");
    expect(
      declared(
        rule(globals, ".navigation-drawer .dialog-header"),
        "margin-bottom",
      ),
    ).toBe("var(--space-2)");
  });
});
