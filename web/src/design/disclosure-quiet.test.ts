// @vitest-environment node
// WEB-U-498 (18.1, the 17.2 review's finding 11, decided on 18 Nov 2026): a form's disclosure (the route form's
// Advanced and Reasoning) is quiet: body size in regular weight, a ghost button's hover tint, its chevron kept, so it
// reads as a toggle beside the bold section labels.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/settings.css", "utf8").replace(
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

describe("WEB-U-498 a form's disclosure is quiet", () => {
  it("is body size in regular weight, with a ghost button's padding and radius", () => {
    const quiet = rule("details.route-group > summary.disclosure-quiet");
    expect(quiet).toMatch(/font-weight:\s*400\b/);
    expect(quiet).toMatch(/font-size:\s*var\(--body-size\)/);
    expect(quiet).toMatch(/border-radius:\s*var\(--control-radius\)/);
    expect(quiet).toMatch(/padding-inline:\s*var\(--space-2\)/);
  });

  it("tints on hover as a ghost button does, and keeps its chevron", () => {
    expect(
      rule("details.route-group > summary.disclosure-quiet:hover"),
    ).toMatch(/var\(--hover-share\)/);
    expect(rule("details.route-group > summary::before")).toMatch(
      /transform:\s*rotate\(-45deg\)/,
    );
  });

  it("shows a focus ring for the keyboard", () => {
    expect(
      rule("details.route-group > summary.disclosure-quiet:focus-visible"),
    ).toMatch(/outline/);
  });
});
