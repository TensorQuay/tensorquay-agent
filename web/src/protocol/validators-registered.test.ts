// @vitest-environment node
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { productSources, split } from "../../scripts/validators.mjs";

import * as core from "./generated/validators.core";

// WEB-U-014: an area's validators are registered by the files that decode its definitions, as its copy is; a file
// that decodes one without importing the area's module would throw "No validator" for a user, so it fails here.
describe("WEB-U-014 every decoded definition has its validator registered", () => {
  it("finds each definition a file decodes in the core module or in an area module the file imports", async () => {
    const files = await productSources(process.cwd());
    const modules = split(files);
    const areaOf = new Map(
      Object.entries(modules)
        .filter(([module]) => module !== "core" && module !== "admin")
        .flatMap(([module, names]) => names.map((name) => [name, module])),
    );
    const missing: string[] = [];
    let checked = 0;
    for (const [path, text] of files)
      for (const line of text.split("\n"))
        for (const [, name] of line.matchAll(
          /\bparseApi\([^;]*?"([A-Z][A-Za-z0-9]*)"/g,
        )) {
          checked += 1;
          if (name && name in core.api) continue;
          const area = name ? areaOf.get(name) : undefined;
          if (!area || !text.includes(`/generated/validators.${area}"`))
            missing.push(`${path}: ${name ?? "?"}`);
        }
    expect(missing).toEqual([]);
    expect(checked).toBeGreaterThan(40);
  });

  it("registers the administration validators wherever parseAdminApi is defined", () => {
    expect(
      readFileSync(new URL("./validate-admin.ts", import.meta.url), "utf8"),
    ).toContain('import "./generated/validators.admin";');
  });
});
