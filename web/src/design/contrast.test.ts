// @vitest-environment node
// WEB-U-007: recompute text and non-text contrast from the actual theme tokens.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { contrastRatio, parseColor, tint } from "./contrast";

const css = readFileSync("src/app/globals.css", "utf8");
const light = css.match(/:root[^{}]*\{([^}]+)\}/)?.[1] ?? "";
const dark = css.match(/\[data-theme="dark"\][^{}]*\{([^}]+)\}/)?.[1] ?? "";
const tintAmount =
  Number.parseFloat(css.match(/--status-tint:\s*([^;]+)/)?.[1] ?? "") / 100;
function token(block: string, name: string) {
  const value = block.match(new RegExp(`--${name}:\\s*([^;]+)`))?.[1];
  if (!value) throw new Error(`Missing ${name} token`);
  return parseColor(value.trim());
}

describe("WCAG contrast calculation", () => {
  it("reads the shorthand colours emitted by the production CSS optimiser", () => {
    const paper = light.match(/--paper:\s*([^;]+)/)?.[1]?.trim() ?? "";
    const shorthand = paper.replace(/([\da-f])\1/gi, "$1");
    expect(shorthand).toHaveLength(4);
    expect(parseColor(shorthand)).toEqual(parseColor(paper));
  });
  it("matches the black-to-white reference ratio", () => {
    expect(contrastRatio([0, 0, 0], [255, 255, 255])).toBe(21);
  });
});

describe.each([
  ["light", light],
  ["dark", dark],
])("%s token contrast", (_theme, block) => {
  it.each(["paper", "surface", "surface-raised", "code-bg", "selection"])(
    "keeps ink legible on %s",
    (background) => {
      expect(
        contrastRatio(token(block, "ink"), token(block, background)),
      ).toBeGreaterThanOrEqual(4.5);
    },
  );
  it("keeps secondary text legible", () => {
    expect(
      contrastRatio(token(block, "muted"), token(block, "surface")),
    ).toBeGreaterThanOrEqual(4.5);
  });
  it.each(["ok", "warn", "danger", "info", "muted"])(
    "keeps %s labels legible on their actual tint",
    (name) => {
      const foreground = token(block, name);
      expect(
        contrastRatio(
          foreground,
          tint(foreground, token(block, "surface"), tintAmount),
        ),
      ).toBeGreaterThanOrEqual(4.5);
    },
  );
  // WCAG 1.4.11: a field's boundary is how people find it, so it needs 3:1 against every surface a field sits on.
  // --line stays for dividers and cards, which may be quieter.
  it.each(["paper", "surface", "surface-raised"])(
    "keeps field and select borders visible on %s",
    (background) => {
      expect(
        contrastRatio(token(block, "control-border"), token(block, background)),
      ).toBeGreaterThanOrEqual(3);
    },
  );
  it("keeps the accent visible as a non-text indicator", () => {
    expect(
      contrastRatio(token(block, "accent"), token(block, "paper")),
    ).toBeGreaterThanOrEqual(3);
  });
});

describe("control borders", () => {
  it("draws fields, selects and text areas with --control-border, not the quieter --line", () => {
    const rule = css.match(/\n\.form-control \{([^}]+)\}/)?.[1] ?? "";
    expect(rule).toMatch(/border:[^;]*var\(--control-border\)/);
  });
  it("defines --control-border in all three theme blocks", () => {
    expect(css.match(/--control-border:/g)).toHaveLength(3);
  });
});
