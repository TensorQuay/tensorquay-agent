// @vitest-environment node
// WEB-U-134 (F19's rest): one design system: every choice in the app is the standard picker (the shared Select in a
// form, MenuButton elsewhere, grouped for the Settings sections on a phone), never a native <select>.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

function sources(directory: string): string[] {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name) ? [path] : [];
  });
}

describe("WEB-U-134 no native select", () => {
  it("renders no <select> anywhere in the app", () => {
    const found = sources("src").filter((path) =>
      /<select\b/.test(readFileSync(path, "utf8")),
    );
    expect(found).toEqual([]);
  });
});
