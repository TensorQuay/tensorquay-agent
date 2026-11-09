// @vitest-environment node
// WEB-U-458 (UI-GUIDELINES §7 Composer, decided on 9 Nov 2026): the composer's look, set from the tokens. One row at
// rest, about 52 px: the card's 8 px around +, the text and Send. The text keeps the round buttons' height and its one
// line at their middle, and grows to eight lines above them, the row keeping them at its bottom ends. Its focus is the
// card's thin --focus border, with no 2 px ring. What waits for the agent is a row of chips above the text.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const read = (file: string) =>
  readFileSync(`src/app/${file}`, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const conversation = read("conversation.css");

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

describe("WEB-U-458 the compact composer's look", () => {
  it("is one row at rest: 8 px around +, the text and Send, their bottoms on one line", () => {
    expect(declared(rule(conversation, ".composer-card"), "padding")).toBe(
      "var(--space-2)",
    );
    const row = rule(conversation, ".composer-row");
    expect(declared(row, "display")).toBe("flex");
    expect(declared(row, "align-items")).toBe("flex-end");
    expect(declared(row, "gap")).toBe("var(--space-2)");
  });

  it("keeps the text as tall as the round buttons, its one line at their middle", () => {
    const field = rule(conversation, ".composer-row > .field");
    expect(declared(field, "flex")).toBe("1");
    expect(declared(field, "min-width")).toBe("0");
    expect(declared(field, "min-height")).toBe("var(--composer-button-size)");
    expect(declared(field, "display")).toBe("flex");
    expect(declared(field, "align-items")).toBe("center");
    const text = rule(conversation, ".composer-card textarea.composer-input");
    expect(declared(text, "padding")).toBe("var(--space-1) 0");
    expect(declared(text, "min-height")).toBe(
      "calc(var(--body-size) * var(--body-leading) + 2 * var(--space-1))",
    );
    expect(declared(text, "max-height")).toBe(
      "calc(var(--body-size) * var(--body-leading) * 8 + 2 * var(--space-1))",
    );
  });

  it("shows its focus as the card's thin --focus border, with no 2 px ring", () => {
    const focused = rule(conversation, ".composer-card:focus-within");
    expect(declared(focused, "border-color")).toBe("var(--focus)");
    expect(declared(focused, "outline")).toBe(undefined);
    expect(declared(focused, "outline-offset")).toBe(undefined);
    expect(declared(focused, "box-shadow")).toBe(undefined);
  });

  it("lays what waits for the agent out as a row of chips above the text", () => {
    const list = rule(conversation, ".composer-held");
    expect(declared(list, "display")).toBe("flex");
    expect(declared(list, "flex-wrap")).toBe("wrap");
    expect(declared(list, "list-style")).toBe("none");
    const chip = rule(conversation, ".composer-queued");
    expect(declared(chip, "border-radius")).toBe("var(--pill-radius)");
    expect(declared(chip, "background")).toBe("var(--surface)");
    expect(declared(chip, "max-width")).toBe("100%");
    const words = rule(conversation, ".composer-queued > span");
    expect(declared(words, "text-overflow")).toBe("ellipsis");
    expect(declared(words, "white-space")).toBe("nowrap");
  });
});
