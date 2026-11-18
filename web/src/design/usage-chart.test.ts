// @vitest-environment node
// WEB-U-499 (18.1, the 17.2 review's finding 12, decided on 18 Nov 2026): the usage chart's bars are ink, never the
// brand's accent (kept for the brand and non-text marks), with 2 px rounded tops; each share's number sits on a 4 px
// track, its part of the whole drawn in ink over the line colour.
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

describe("WEB-U-499 the usage chart in ink, with a labelled scale", () => {
  it("draws each day's bar in ink with 2 px rounded tops, never the accent", () => {
    const bar = rule(".usage-day");
    expect(bar).toMatch(/background:\s*var\(--ink\)/);
    expect(bar).not.toMatch(/--accent/);
    expect(bar).toMatch(
      /border-radius:\s*calc\(2 \* var\(--border-width\)\) calc\(2 \* var\(--border-width\)\) 0 0/,
    );
  });

  it("puts each share's number on a 4 px track, in ink over the line colour", () => {
    const share = rule(".usage-share");
    expect(share).not.toMatch(/--accent/);
    expect(share).toMatch(/var\(--ink\)/);
    expect(share).toMatch(/var\(--line\)/);
    expect(share).toMatch(/var\(--space-1\)/);
  });

  it("sets the scale's maximum at the top, beside the bars", () => {
    expect(rule(".usage-scale")).toMatch(/justify-content:\s*space-between/);
  });
});
