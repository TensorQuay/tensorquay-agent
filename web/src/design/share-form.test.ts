// @vitest-environment node
// WEB-U-251 (the 14b page pass, W4): at 390 the share dialog kept Email, "Can view" and Share on one row, squeezing
// the email field to about 100 px. On a phone the field takes its own row, and the access choice and Share wrap under
// it; from a tablet's width up they stay on one row.
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/workspaces.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
const phone = /@media \(max-width: 767px\)\s*\{([\s\S]*?)\n\}/.exec(css)?.[1];
function block(source: string, selector: string): string {
  const name = selector.replace(/\./g, "\\.");
  const rule = new RegExp(`(?:^|\\})\\s*${name}\\s*\\{([^}]*)\\}`);
  return rule.exec(source)?.[1] ?? "";
}

describe("WEB-U-251 the share dialog at phone width", () => {
  it("gives the email its own row on a phone", () => {
    expect(phone).toBeDefined();
    expect(block(phone ?? "", ".share-form")).toMatch(/flex-wrap:\s*wrap/);
    expect(block(phone ?? "", ".share-form .field")).toMatch(
      /flex-basis:\s*100%/,
    );
  });

  it("keeps one row from a tablet's width up", () => {
    const wide = css.replace(/@media[^{]*\{[\s\S]*?\n\}/g, "");
    expect(block(wide, ".share-form")).not.toMatch(/flex-wrap/);
    expect(block(wide, ".share-form .field")).not.toMatch(/flex-basis/);
  });
});
