// @vitest-environment node
// WEB-U-511 (18.2): a text button's label wraps inside the button, centred, the button growing from the control height,
// and never ends in an ellipsis that would hide the action's name; an icon-only button keeps its fixed square.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const globals = readFileSync("src/app/globals.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The declarations of every rule whose selector list names `selector` exactly, joined. */
function rule(selector: string): string {
  const found: string[] = [];
  for (const match of globals.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
const declared = (block: string, property: string) =>
  block.match(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`))?.[1]?.trim();

describe("WEB-U-511 a long button label", () => {
  const button = rule(".button");
  const label = rule(".button-label");

  it("grows the button from the control height instead of fixing it", () => {
    expect(declared(button, "min-height")).toBe("var(--control-height)");
    expect(declared(button, "height")).toBeUndefined();
  });

  it("wraps the words inside the button, centred, and keeps the button within its room", () => {
    expect(declared(button, "white-space")).toBe("normal");
    expect(declared(button, "max-width")).toBe("100%");
    expect(declared(button, "text-align")).toBe("center");
    expect(declared(label, "min-width")).toBe("0");
    expect(declared(label, "overflow-wrap")).toBe("anywhere");
  });

  it("never cuts the name with an ellipsis", () => {
    expect(declared(button, "text-overflow")).toBeUndefined();
    expect(declared(label, "text-overflow")).toBeUndefined();
  });

  it("leaves an icon-only button its fixed square", () => {
    const icon = rule(".icon-button");
    expect(declared(icon, "height")).toBe("var(--control-height)");
    expect(declared(icon, "width")).toBe("var(--control-height)");
  });
});
