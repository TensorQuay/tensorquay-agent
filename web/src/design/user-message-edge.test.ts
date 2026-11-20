// @vitest-environment node
// WEB-U-503 (18.1, the 17.2 review's finding 19, decided on 18 Nov 2026): in the dark theme the user's message stands
// apart from the page by a 1 px edge in the line colour, as the composer's, drawn inset so nothing moves; the light theme keeps its edge clear. The
// edge's contrast with the page is measured from the theme tokens: more than the bubble's own fill gives.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { contrastRatio, parseColor } from "./contrast";

const globals = readFileSync("src/app/globals.css", "utf8");
const conversation = readFileSync("src/app/conversation.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
const light = globals.match(/:root[^{}]*\{([^}]+)\}/)?.[1] ?? "";
const dark = globals.match(/\[data-theme="dark"\][^{}]*\{([^}]+)\}/)?.[1] ?? "";
const media =
  globals.match(/:root:not\(\[data-theme="light"\]\)[^{}]*\{([^}]+)\}/)?.[1] ??
  "";
const raw = (block: string, name: string) =>
  block.match(new RegExp(`--${name}:\\s*([^;]+)`))?.[1]?.trim();
const color = (block: string, name: string) => {
  const value = raw(block, name);
  if (!value) throw new Error(`Missing ${name} token`);
  return parseColor(value);
};

describe("WEB-U-503 the dark theme's user message stands apart", () => {
  it("draws the message's edge from a theme token", () => {
    const rule = conversation.match(/\.user-message\s*\{([^}]*)\}/)?.[1] ?? "";
    // Inset, so the bubble keeps its size in both themes (a border would move every light screen by 2 px).
    expect(rule).toMatch(
      /box-shadow:\s*inset 0 0 0 var\(--border-width\)\s+var\(--message-edge\)/,
    );
  });

  it("is the line colour in both dark blocks, clear in the light theme", () => {
    expect(raw(dark, "message-edge")).toBe("var(--line)");
    expect(raw(media, "message-edge")).toBe("var(--line)");
    expect(raw(light, "message-edge")).toBe("transparent");
  });

  it("gives the edge more contrast with the page than the bubble's fill has", () => {
    const paper = color(dark, "paper");
    const edge = contrastRatio(color(dark, "line"), paper);
    const fill = contrastRatio(color(dark, "surface"), paper);
    expect(edge).toBeGreaterThan(fill);
    expect(edge).toBeGreaterThanOrEqual(1.3);
  });
});
