// @vitest-environment node
// WEB-U-117 (a first-hour check): People's email column is not capped at the name width, and an address never breaks
// inside a word (only at the <wbr> after its @ and dots).
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const settings = readFileSync("src/app/settings.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
function rule(selector: string): string {
  const found: string[] = [];
  for (const match of settings.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}
const declared = (block: string, property: string) =>
  block.match(new RegExp(`(?:^|[;\\s])${property}:\\s*([^;]+)`))?.[1]?.trim();

describe("WEB-U-117 an email reads whole", () => {
  it("takes the room the table has, and breaks only where the page allows", () => {
    expect(declared(rule(".data-name-wrap"), "max-width")).toBe("none");
    const text = rule(".data-name-wrap > span:first-child");
    expect(declared(text, "overflow-wrap")).toBe("normal");
    expect(declared(text, "word-break")).toBe("normal");
  });
});

// A screen review before 17.0: at 1024 an email broke at a dot although the table had room for it. Where its column has
// room it stays on one line (accounts.spec counts its line boxes at 1024 and 1440); since WEB-U-382 a long one may break
// at its <wbr>s in any width, and on a phone it may also shrink.
describe("WEB-U-156 an email stays on one line where there is room", () => {
  it("breaks only at its <wbr>s, and shrinks on a phone only", () => {
    const text = rule(".data-name-wrap > span:first-child");
    expect(declared(text, "white-space")).toBe("normal");
    const phone =
      /@container people \(max-width: 679px\)\s*\{\s*\.data-name-wrap > span:first-child\s*\{([^}]*)\}/.exec(
        settings,
      )?.[1];
    expect(declared(phone ?? "", "white-space")).toBe("normal");
  });
});

// Regression (TEST-PLAN §19): WEB-U-156's nowrap didn't hold at 1024. The email is a flex item beside "You"; squeezed
// below its text, Chromium still breaks it at a <wbr>. From a tablet's width up it keeps its text's width.
describe("WEB-U-168 an email is never squeezed below its text", () => {
  it("doesn't shrink beside its pill, except where People is narrow", () => {
    expect(declared(rule(".people-page"), "container")).toBe(
      "people / inline-size",
    );
    const text = rule(".data-name-wrap > span:first-child");
    expect(declared(text, "flex-shrink")).toBe("0");
    expect(declared(text, "min-width")).toBe("auto");
    const phone =
      /@container people \(max-width: 679px\)\s*\{\s*\.data-name-wrap > span:first-child\s*\{([^}]*)\}/.exec(
        settings,
      )?.[1];
    expect(declared(phone ?? "", "flex-shrink")).toBe("1");
    expect(declared(phone ?? "", "min-width")).toBe("0");
  });
});

// A pass over the pages before 17.0: kept on one line at 1280, a 41-character address ran under its row's Role menu,
// since the table sizes the column by the address's <wbr>s. It now breaks there when its column is narrower than it,
// and "You" goes under it rather than squeezing it (accounts.spec checks the lines against the menu at 1280 and 390).
describe("WEB-U-382 a long email breaks at its <wbr>s rather than run into the next column", () => {
  it("is held to its cell, and its pill wraps", () => {
    const text = rule(".data-name-wrap > span:first-child");
    expect(declared(text, "white-space")).toBe("normal");
    expect(declared(text, "max-width")).toBe("100%");
    expect(declared(text, "flex-shrink")).toBe("0");
    expect(declared(rule(".data-name-wrap"), "flex-wrap")).toBe("wrap");
  });
});
