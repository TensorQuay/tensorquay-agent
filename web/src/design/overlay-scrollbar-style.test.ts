// @vitest-environment node
// WEB-U-485 (18.1 polish): the transcript hides the browser's own bar (it takes width) and shows its overlay one
// instead, in Firefox and in Chromium and WebKit; with forced colours the system bar stays and the overlay goes.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/conversation.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

/** The text inside the at-rule that starts with `head`, braces balanced. */
function block(head: string): string {
  const start = css.indexOf(head);
  if (start < 0) return "";
  let depth = 0;
  for (let at = css.indexOf("{", start); at < css.length; at++) {
    if (css[at] === "{") depth++;
    if (css[at] === "}" && --depth === 0)
      return css.slice(css.indexOf("{", start) + 1, at);
  }
  return "";
}

describe("WEB-U-485 the overlay scrollbar's style", () => {
  it("hides the browser's bar on the transcript, unless colours are forced", () => {
    const unforced = block("@media not (forced-colors: active)");
    expect(unforced).toMatch(
      /\.transcript-scroll\s*\{[^}]*scrollbar-width:\s*none/,
    );
    expect(unforced).toMatch(
      /\.transcript-scroll::-webkit-scrollbar\s*\{[^}]*display:\s*none/,
    );
  });

  it("draws no overlay when colours are forced", () => {
    expect(block("@media (forced-colors: active)")).toMatch(
      /\.overlay-scrollbar\s*\{[^}]*display:\s*none/,
    );
  });

  it("scrolls the whole main area, its text a centred column inside it", () => {
    expect(css).toMatch(
      /\.conversation-page > \.transcript-scroll\s*\{[^}]*max-width:\s*none/,
    );
    expect(css).toMatch(
      /\.transcript-column\s*\{[^}]*max-width:\s*calc\(var\(--transcript-width\) - 2 \* var\(--space-1\)\)[^}]*margin-inline:\s*auto/,
    );
  });

  it("takes no width and fades in and out", () => {
    const bar = /\.overlay-scrollbar\s*\{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(bar).toMatch(/position:\s*absolute/);
    expect(bar).toMatch(/opacity:\s*0/);
    expect(bar).toMatch(/transition:\s*opacity/);
    expect(css).toMatch(
      /\.overlay-scrollbar\[data-shown\]\s*\{[^}]*opacity:\s*1/,
    );
  });
});
