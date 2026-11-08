// @vitest-environment node
// WEB-U-116 (a first-hour check, superseding WEB-U-082's blue): the overview's row actions look like every other link
// in the app, the text's own colour and underlined, with at least the minimum control height to hit.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const settings = read("settings.css");

/** The declarations of every rule whose selector list names `selector` exactly, joined. */
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

describe("WEB-U-116 the overview's row links", () => {
  it("looks like every other link: no colour of its own, underlined, with room to hit", () => {
    const link = rule(settings, ".overview-line > a");
    expect(declared(link, "color")).toBeUndefined();
    expect(declared(link, "text-decoration")).toBe("underline");
    expect(declared(link, "min-height")).toBe("var(--min-control-height)");
  });
});
