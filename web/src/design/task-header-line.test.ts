// @vitest-environment node
// WEB-U-495 (18.1; the owner, 17 Nov: "not as elegant", "why make it bold"): the task header is one quiet line, the
// title at body size in regular weight, then its profile and workspace muted on the same line; the origin gives way
// first, the title last.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const css = read("globals.css") + read("conversation.css");

function rule(selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
const declared = (block: string, property: string) =>
  [...block.matchAll(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`, "g"))]
    .at(-1)?.[1]
    ?.trim();

describe("WEB-U-495 the header's one quiet line", () => {
  it("sets the title at body size in regular weight", () => {
    const title = rule(".type-task-title");
    expect(declared(title, "font-size")).toBe("var(--body-size)");
    expect(declared(title, "line-height")).toBe("var(--body-leading)");
    expect(declared(title, "font-weight")).toBe("400");
  });

  it("puts the origin beside the title, muted, giving way first", () => {
    const name = rule(".conversation-name");
    expect(declared(name, "flex-direction")).toBe("row");
    expect(declared(name, "align-items")).toBe("baseline");
    expect(declared(rule(".conversation-origin"), "flex-shrink")).toBe("1000");
    // The title never shrinks for the origin: only past the whole line is it cut.
    const title = rule(".conversation-name > h1");
    expect(declared(title, "flex-shrink")).toBe("0");
    expect(declared(title, "max-width")).toBe("100%");
  });
});
