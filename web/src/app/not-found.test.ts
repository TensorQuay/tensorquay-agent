import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const source = readFileSync(join("src", "app", "not-found.tsx"), "utf8");
const providers = readFileSync(join("src", "app", "providers.tsx"), "utf8");
const statics = [...source.matchAll(/^import\s[^;]*?from\s+"([^"]+)"/gm)].map(
  ([, specifier]) => specifier,
);

// TEST: WEB-U-381
describe("the root not-found", () => {
  // Next puts it in every page's error boundary: a static import of the page (and the sign-in copy it brings) loaded
  // 4.6 KiB more on the conversation page, past WEB-U-014/015's budget, and a loader in its own file 442 B (16h).
  it("loads its page only when an address has no page", () => {
    // Only the loader, which rides in the providers' script every page loads already (no script of its own).
    expect(statics).toEqual(["./providers"]);
    expect(providers).toMatch(
      /lazy\(\(\) =>\s*import\("\.\.\/shell\/NotFoundPage"\)/,
    );
    expect(providers).not.toMatch(/^import[^;]*NotFoundPage/m);
  });
});
