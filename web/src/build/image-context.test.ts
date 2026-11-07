import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  candidates,
  ignorePatterns,
  inContext,
  relativeImports,
  typeChecked,
} from "./image-context";

/** The repository, above web/ where the tests run. */
const ROOT = join(process.cwd(), "..") + "/";
const IGNORE = "images/web/Dockerfile.dockerignore";

/** The files of `folders` as a release tree has them (tracked; nothing installed, built or generated). */
function files(...folders: string[]): string[] {
  return execFileSync("git", ["ls-files", "-z", ...folders], {
    cwd: ROOT,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean);
}

// TEST: WEB-U-194
describe("the web image's build context (images/web/Dockerfile)", () => {
  it("reads a .dockerignore as Docker does", () => {
    const patterns = ignorePatterns(
      "# comment\n**\n!web\n!web/**\n\nweb/e2e\n!web/e2e/keep.ts\nweb/**/*.test.ts\n",
    );
    expect(inContext("web/src/a.ts", patterns)).toBe(true);
    expect(inContext("crates/x.rs", patterns)).toBe(false);
    expect(inContext("web/e2e/spec.ts", patterns)).toBe(false);
    expect(inContext("web/e2e/keep.ts", patterns)).toBe(true);
    expect(inContext("web/src/deep/a.test.ts", patterns)).toBe(false);
  });

  it("reads which files a tsconfig type-checks", () => {
    const config = {
      include: ["**/*.ts", "**/*.tsx"],
      exclude: ["node_modules", "e2e/**", "playwright*.config.ts"],
    };
    expect(typeChecked("src/a.ts", config)).toBe(true);
    expect(typeChecked("src/b.tsx", config)).toBe(true);
    expect(typeChecked("src/c.css", config)).toBe(false);
    expect(typeChecked("e2e/spec.ts", config)).toBe(false);
    expect(typeChecked("playwright.stack.config.ts", config)).toBe(false);
    expect(typeChecked("node_modules/x/index.ts", config)).toBe(false);
  });

  it("finds a source's relative imports and the files they may name", () => {
    expect(
      relativeImports(
        'import a from "./a";\nexport { b } from "../b";\nimport "./side";\nconst c = await import("./c");\nimport d from "d";',
      ),
    ).toEqual(["./a", "../b", "./side", "./c"]);
    expect(candidates("web/e2e/x.ts", "../src/y")).toContain("web/src/y.ts");
    expect(candidates("web/a.ts", "./b")).toContain("web/b/index.tsx");
  });

  // The image builds with `next build`, which type-checks the files tsconfig.json selects among those the context
  // keeps: one that imports a file the context drops fails the build, as `deploy/build-images.sh` and the Mac guide
  // build it (a tarball context skips the Dockerfile's ignore file, which hid it). Release 13h's stack config imported
  // its e2e helper this way.
  it("keeps every file that a file the image's build type-checks imports", () => {
    const patterns = ignorePatterns(readFileSync(join(ROOT, IGNORE), "utf8"));
    const config = JSON.parse(
      readFileSync(join(ROOT, "web/tsconfig.json"), "utf8"),
    ) as { include: string[]; exclude: string[] };
    const kept = new Set(
      files("web", "brand").filter((path) => inContext(path, patterns)),
    );
    const missing = [...kept]
      .filter(
        (path) =>
          path.startsWith("web/") && typeChecked(path.slice(4), config),
      )
      .flatMap((path) =>
        relativeImports(readFileSync(join(ROOT, path), "utf8"))
          .filter((specifier) =>
            candidates(path, specifier).every(
              (candidate) => !kept.has(candidate),
            ),
          )
          .map((specifier) => `${path} imports ${specifier}`),
      );
    expect(missing).toEqual([]);
  });
});
