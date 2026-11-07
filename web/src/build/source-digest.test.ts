import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { sourceDigest } from "./source-digest";

const roots: string[] = [];
function tree(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "source-digest-"));
  roots.push(root);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, ".."), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true });
});

// 061 addendum 2: the same web source must always produce the same build identity.
describe("reproducible build identity", () => {
  const base = {
    "package.json": "{}",
    "src/app/page.tsx": "export default 1;",
    "src/ui/Button.tsx": "export const b = 2;",
  };

  it("is stable for the same files regardless of creation order and file times", () => {
    const first = tree(base);
    const second = tree(Object.fromEntries(Object.entries(base).reverse()));
    utimesSync(join(second, "package.json"), 0, 0);
    expect(sourceDigest(second, ["package.json", "src"])).toBe(
      sourceDigest(first, ["package.json", "src"]),
    );
    expect(sourceDigest(first, ["package.json", "src"])).toMatch(/^[0-9a-f]{20}$/);
  });

  it("changes when any file's content or path changes", () => {
    const digest = sourceDigest(tree(base), ["package.json", "src"]);
    expect(
      sourceDigest(tree({ ...base, "src/ui/Button.tsx": "export const b = 3;" }), [
        "package.json",
        "src",
      ]),
    ).not.toBe(digest);
    const { "src/ui/Button.tsx": moved, ...rest } = base;
    expect(
      sourceDigest(tree({ ...rest, "src/ui/Other.tsx": moved }), ["package.json", "src"]),
    ).not.toBe(digest);
  });

  it("ignores build output, dependencies and test files", () => {
    const digest = sourceDigest(tree(base), ["package.json", "src"]);
    expect(
      sourceDigest(
        tree({
          ...base,
          "src/.next/cache.json": "x",
          "src/node_modules/a.js": "x",
          "src/app/page.test.tsx": "x",
        }),
        ["package.json", "src"],
      ),
    ).toBe(digest);
  });
});
