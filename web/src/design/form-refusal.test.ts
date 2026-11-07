// @vitest-environment node
// WEB-U-110 (the lead's first hour, F14): every settings form shows where a refused save needs fixing. A settings form
// is a <form> under src/settings; it carries FormRefusal by its Save, or is a page on the Settings page frame, which does.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const DIRECTORY = "src/settings";
const forms = readdirSync(DIRECTORY)
  .filter((name) => /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name))
  .map((name) => [name, readFileSync(join(DIRECTORY, name), "utf8")] as const)
  .filter(([, source]) => source.includes("<form"));

describe("WEB-U-110 every settings form shows where a save was refused", () => {
  it("finds the settings forms", () => {
    expect(forms.length).toBeGreaterThan(10);
  });

  it.each(forms.map(([name]) => name))("%s carries FormRefusal", (name) => {
    const source = forms.find(([file]) => file === name)?.[1] ?? "";
    expect(source).toMatch(/<FormRefusal\b/);
  });

  it("puts it on the Settings page frame, for every page built on it", () => {
    const frame = readFileSync(join(DIRECTORY, "SettingsPage.tsx"), "utf8");
    expect(frame).toMatch(/<FormRefusal\b/);
  });
});
