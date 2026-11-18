// @vitest-environment node
// WEB-U-492 (18.1 polish): the Files panel's heading row keeps the file's size on one line ("661.6 kB" wrapped
// beside Expand at 390). Where the row can't hold the size and both actions, the actions move under it together, at
// the row's end; the size never breaks and never shrinks.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/files.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

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

describe("WEB-U-492 the file's size on one line", () => {
  it("never breaks or shrinks the size", () => {
    const size = rule(".file-preview-size");
    expect(declared(size, "white-space")).toBe("nowrap");
    expect(declared(size, "flex-shrink")).toBe("0");
  });

  it("moves the actions under it, at the row's end, when the row is too narrow", () => {
    expect(declared(rule(".file-preview-heading"), "flex-wrap")).toBe("wrap");
    expect(declared(rule(".file-preview-actions"), "margin-inline-start")).toBe(
      "auto",
    );
  });
});
