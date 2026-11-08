import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

import { describe, expect, it } from "vitest";

import { sentenceGap } from "./sentences";

// WEB-U-361 (a pass over the pages before 17.0): two sentences side by side take a space in English, nothing in Chinese
// or Japanese, so the token dialog never reads "…查看。 到期时间…".
describe("WEB-U-361 the gap between two sentences", () => {
  it.each([
    ["en", " "],
    ["fr", " "],
    ["zh-Hans", ""],
    ["ja", ""],
  ])("%s", (locale, gap) => {
    expect(sentenceGap(locale)).toBe(gap);
  });
});

// WEB-U-361: no catalogue sentence is followed by a plain space where its Chinese ends in full-width punctuation
// (。！？：；，): such a join takes `sentenceGap`, which gives zh and ja nothing. Read from the source, so a new join
// can't bring the stray space back.
describe("WEB-U-361 no plain space after a full-width stop", () => {
  it("finds none in the components", () => {
    const zh = (
      JSON.parse(
        readFileSync(join("src", "i18n", "locales", "zh-Hans.json"), "utf8"),
      ) as {
        messages: Record<string, string>;
      }
    ).messages;
    const root = "src";
    const files = (function walk(directory: string): string[] {
      return readdirSync(directory, { withFileTypes: true }).flatMap(
        (entry) => {
          const path = join(directory, entry.name);
          if (entry.isDirectory()) return walk(path);
          return path.endsWith(".tsx") && !path.endsWith(".test.tsx")
            ? [path]
            : [];
        },
      );
    })(root);
    const joins = files.flatMap((file) =>
      [
        ...readFileSync(file, "utf8").matchAll(
          /\{t\("([a-zA-Z0-9.]+)"[^)]*\)\}\{" "\}/g,
        ),
      ].map((match) => ({ file: relative(root, file), key: match[1] ?? "" })),
    );
    const stray = joins.filter(({ key }) =>
      /[。！？：；，]$/.test(zh[key] ?? ""),
    );
    expect(stray).toEqual([]);
  });
});
