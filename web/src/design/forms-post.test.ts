// @vitest-environment node
// SEC-070 (18.2.1; the lead's check of 27 Nov 2026): no form can send what it holds in the address. A form without a
// method is a GET form: clicked before the page's script has loaded, the sign-in form sent the browser to
// `/login?email=…&password=…`, where browser history and any proxy's access log keep it. Every form posts.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx$/.test(name) && !/\.test\.tsx$/.test(name) ? [path] : [];
  });
}

/** Each `<form …>` opening tag of `text`, up to its closing `>` (attributes may span lines). */
function formTags(text: string): string[] {
  return [...text.matchAll(/<form\b[\s\S]*?>/g)].map((match) => match[0]);
}

describe("SEC-070 every form posts", () => {
  const forms = sources("src").flatMap((path) =>
    formTags(readFileSync(path, "utf8")).map((tag) => ({ path, tag })),
  );

  it("finds the app's forms", () => {
    expect(forms.length).toBeGreaterThanOrEqual(20);
  });

  it('gives every form method="post"', () => {
    const without = forms
      .filter(({ tag }) => !/\bmethod="post"/.test(tag))
      .map(
        ({ path, tag }) => `${path}: ${tag.replace(/\s+/g, " ").slice(0, 80)}`,
      );
    expect(without).toEqual([]);
  });
});
