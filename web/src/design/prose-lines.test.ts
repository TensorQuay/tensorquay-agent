// @vitest-environment node
// WEB-U-493 (18.1 polish): a code block marked as prose lines (a PDF read's output) wraps them, a long address or word
// included, instead of scrolling sideways.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/conversation.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

describe("WEB-U-493 prose lines wrap", () => {
  it("wraps the lines and any long word in them", () => {
    const block =
      /\.code-block\.prose-lines\s*\{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(block).toMatch(/white-space:\s*pre-wrap/);
    expect(block).toMatch(/overflow-wrap:\s*anywhere/);
  });
});
