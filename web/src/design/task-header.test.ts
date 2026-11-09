// @vitest-environment node
// WEB-U-456 (UI-GUIDELINES §7 Conversation bar, decided on 9 Nov 2026): the task header spans the main area, and only
// the header does. The page around it fills the main area, everything under the header keeps the centred reading
// column, and the header's line gives way in its order (the title first, then the pickers' names), never wrapping. A
// channel's header does the same over its log and side; the environment's question under the task header keeps the
// column's width, at the header's end.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const conversation = read("conversation.css");
const channels = read("channels.css");

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
/** The last value a rule set gives `property`, as the cascade would, its white space as one line writes it. */
const declared = (block: string, property: string) =>
  [...block.matchAll(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`, "g"))]
    .at(-1)?.[1]
    ?.replace(/\s+/g, " ")
    .replace(/\( /g, "(")
    .replace(/ \)/g, ")")
    .trim();

describe("WEB-U-456 the task header spans the main area", () => {
  it("lets the task page fill the main area, and keeps everything under its header in the reading column", () => {
    expect(
      declared(rule(conversation, ".conversation-page"), "max-width"),
    ).toBe(undefined);
    const under = rule(
      conversation,
      ".conversation-page > :not(.conversation-bar)",
    );
    expect(declared(under, "width")).toBe("100%");
    expect(declared(under, "max-width")).toBe("var(--transcript-width)");
    expect(declared(under, "margin-inline")).toBe("auto");
    // The task list keeps its own column.
    expect(
      declared(rule(conversation, ".conversation-list"), "max-width"),
    ).toBe("var(--transcript-width)");
  });

  it("keeps the header on one line, its title giving way before the pickers, which stay at the end", () => {
    expect(
      declared(rule(conversation, ".conversation-bar-main"), "flex-wrap"),
    ).toBe("nowrap");
    expect(declared(rule(conversation, ".conversation-title"), "flex")).toBe(
      "0 1 auto",
    );
    const heading = rule(conversation, ".conversation-title h1");
    expect(declared(heading, "text-overflow")).toBe("ellipsis");
    expect(declared(heading, "white-space")).toBe("nowrap");
    // Whole: the pickers give way only by showing their icons (WEB-U-106, WEB-U-125).
    const pickers = rule(conversation, ".conversation-pickers");
    expect(declared(pickers, "flex-shrink")).toBe("0");
    expect(declared(pickers, "margin-left")).toBe("auto");
  });

  it("floats the environment's question under the header's end, at most the column's width", () => {
    const notice = rule(conversation, ".choices-notice");
    expect(declared(notice, "inset-inline")).toBe(undefined);
    expect(declared(notice, "inset-inline-end")).toBe("var(--space-2)");
    expect(declared(notice, "width")).toBe(
      "min(100% - 2 * var(--space-2), var(--transcript-width) - 2 * var(--space-2))",
    );
  });

  it("does the same on a channel's page: its header across, its log and side in their centred columns", () => {
    for (const page of [".channel-page", ".channel-missing"]) {
      expect(declared(rule(channels, page), "max-width"), page).toBe(undefined);
      const under = rule(channels, `${page} > :not(.channel-bar)`);
      expect(declared(under, "width"), page).toBe("100%");
      expect(declared(under, "max-width"), page).toBe(
        "var(--transcript-width)",
      );
      expect(declared(under, "margin-inline"), page).toBe("auto");
    }
    // From 1280 px: the log and the side between two flexible tracks, which take what is left; the header across all.
    const wide =
      channels.match(/@media \(min-width: 1280px\) \{([\s\S]*?)\n\}/)?.[1] ??
      "";
    const grid = rule(wide, ".channel-page");
    expect(declared(grid, "grid-template-columns")).toBe(
      "[wide-start] minmax(0, 1fr) [log-start] minmax(0, var(--transcript-width)) [log-end] var(--space-6) [side-start] var(--channel-side-width) [side-end] minmax(0, 1fr) [wide-end]",
    );
    expect(declared(grid, "column-gap")).toBe("0");
    expect(declared(grid, "max-width")).toBe(undefined);
    // Everything reads in the log's column unless placed (the idle agents, the first step, the composer), so nothing
    // lands in the gap between the log and the side.
    expect(declared(rule(wide, ".channel-page > *"), "grid-column")).toBe(
      "log",
    );
    expect(
      declared(rule(wide, ".channel-page > .channel-bar"), "grid-column"),
    ).toBe("wide");
    expect(
      declared(rule(wide, ".channel-page > .channel-side"), "grid-column"),
    ).toBe("side");
  });
});
