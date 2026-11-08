import { describe, expect, it } from "vitest";

import { parseHunk } from "./hunk-header";

// A screen review before 17.0: Changes printed unified-diff hunk headers ("@@ -1,4 +1,6 @@"), a developer's notation.
// The header is read once, here, and the page says where the change is in words. A parser: every form, and malformed
// ones (guardrails §2.2: 100 % lines and branches).
describe("WEB-U-164 a hunk header read into where the change is", () => {
  it.each([
    // [header, what it means]
    ["@@ -1,4 +1,6 @@", { kind: "lines", from: 1, to: 6, section: null }],
    ["@@ -12 +12 @@", { kind: "lines", from: 12, to: 12, section: null }],
    ["@@ -0,0 +1,3 @@", { kind: "lines", from: 1, to: 3, section: null }],
    ["@@ -4,2 +3,0 @@", { kind: "removed", after: 3, section: null }],
    ["@@ -1,3 +0,0 @@", { kind: "removed", after: 0, section: null }],
    [
      "@@ -20,7 +21,9 @@ fn handle(request: Request) {",
      {
        kind: "lines",
        from: 21,
        to: 29,
        section: "fn handle(request: Request) {",
      },
    ],
    ["@@ -5,1 +5,1 @@   ", { kind: "lines", from: 5, to: 5, section: null }],
  ] as const)("reads %s", (header, meaning) => {
    expect(parseHunk(header)).toEqual(meaning);
  });

  it.each([
    "@@ -1,4 +1,6",
    "@@ +1,6 -1,4 @@",
    "@@ -a,4 +1,6 @@",
    "@@ -1,4 +1,x @@",
    "@@  @@",
    "@ -1 +1 @",
    "-1,4 +1,6",
    "",
    `@@ -1 +${"9".repeat(400)} @@`,
  ])("refuses %j", (header) => {
    expect(parseHunk(header)).toBeNull();
  });

  // A property, over seeded random headers (deterministic: the seed is fixed): a well-formed one always reads, its
  // range never runs backwards, and a hunk that keeps no lines reads as a removal after its start.
  it("reads every well-formed header, in order", () => {
    let seed = 0x13f9;
    const next = (limit: number) => {
      seed = (seed * 1103515245 + 12345) % 2 ** 31;
      return seed % limit;
    };
    for (let case_ = 0; case_ < 500; case_ += 1) {
      const [oldStart, oldCount] = [next(5000), next(200)];
      const [newStart, newCount] = [next(5000) + 1, next(200)];
      const section = next(3) === 0 ? " def run(self):" : "";
      const meaning = parseHunk(
        `@@ -${oldStart},${oldCount} +${newStart},${newCount} @@${section}`,
      );
      if (meaning === null) throw new Error("A well-formed header must read");
      if (newCount === 0)
        expect(meaning).toMatchObject({ kind: "removed", after: newStart });
      else {
        expect(meaning).toMatchObject({ kind: "lines", from: newStart });
        if (meaning.kind === "lines")
          expect(meaning.to).toBeGreaterThanOrEqual(meaning.from);
      }
    }
  });
});
