// @vitest-environment node
// WEB-U-461: words for screen readers only leave the screen through one class, Tailwind's `sr-only`, set where the
// words are: a short count's exact number, a folded group's label while its live words show (WEB-U-462), the sidebar
// toggle's names (WEB-U-007), a field's label where the place says what the field is for, a run's whole status beside
// its short word (WEB-U-155), a table's actions header. No stylesheet of ours hides words by a rule of its own, except
// where a class can't: inside a media or container query, or on a state the markup can't know (`:empty`; a table's
// header as cards, `[data-cards]`, which TableScroll sets by measuring the table, 17.4.1).
import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, "");

/** Every rule that clips its element to nothing: its sheet, its selector and the queries around it. */
function clipped(file: string, css: string) {
  const found: { file: string; selector: string; queries: string[] }[] = [];
  const open: string[] = [];
  let start = 0;
  for (let at = 0; at < css.length; at++) {
    if (css[at] === "{") {
      open.push(css.slice(start, at).trim());
      start = at + 1;
    } else if (css[at] === "}") {
      const body = css.slice(start, at);
      const selector = open.pop() ?? "";
      if (/clip-path:\s*inset\(50%\)/.test(body))
        found.push({
          file,
          selector,
          queries: open.filter((prelude) =>
            /^@(media|container)\b/.test(prelude),
          ),
        });
      start = at + 1;
    }
  }
  return found;
}

const rules = readdirSync("src/app")
  .filter((file) => file.endsWith(".css"))
  .flatMap((file) =>
    clipped(file, strip(readFileSync(`src/app/${file}`, "utf8"))),
  );

describe("WEB-U-461 words for screen readers only", () => {
  it("leave the screen through sr-only, never a rule of ours but where a class can't", () => {
    expect(
      rules
        .filter(
          (rule) =>
            rule.queries.length === 0 &&
            !/:empty|\[data-cards\]/.test(rule.selector),
        )
        .map((rule) => `${rule.file} ${rule.selector}`),
    ).toEqual([]);
  });
});
