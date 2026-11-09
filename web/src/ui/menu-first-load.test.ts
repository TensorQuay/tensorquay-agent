// @vitest-environment node
// WEB-U-014's budget, the task page's first load (WEB-U-176 for the files panel): a menu's open state loads when a menu
// is about to open, never with the page. Nothing the task page imports statically reaches it: the shared surface that
// draws an open menu, a picker's list and the account menu, nor the Popover and Menu it is drawn with.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

import { describe, expect, it } from "vitest";

/** Where a relative import resolves, as a source file, or nothing (a stylesheet, a JSON file). */
function resolve(from: string, specifier: string): string | null {
  const base = join(dirname(from), specifier);
  for (const candidate of [
    `${base}.tsx`,
    `${base}.ts`,
    join(base, "index.tsx"),
    join(base, "index.ts"),
  ])
    if (existsSync(candidate)) return candidate;
  return null;
}

/** The modules a file imports statically: type-only imports and `import()` are left out, as the bundle leaves them. */
function imports(file: string): string[] {
  const text = readFileSync(file, "utf8");
  const specifiers: string[] = [];
  for (const [, typeOnly, clause, specifier] of text.matchAll(
    /\bimport\s+(type\s+)?([^;"'()]*?)\s*from\s*"([^"]+)"/g,
  )) {
    const names = clause?.match(/\{([^}]*)\}/)?.[1];
    const allTypes =
      !!names &&
      !clause?.replace(/\{[^}]*\}/, "").replace(/[\s,]/g, "") &&
      names
        .split(",")
        .map((name) => name.trim())
        .filter(Boolean)
        .every((name) => name.startsWith("type "));
    if (!typeOnly && !allTypes && specifier) specifiers.push(specifier);
  }
  for (const [, specifier] of text.matchAll(
    /\bexport\s+(?:\{[^}]*\}|\*(?:\s+as\s+\w+)?)\s*from\s*"([^"]+)"/g,
  ))
    if (specifier) specifiers.push(specifier);
  for (const [, specifier] of text.matchAll(/\bimport\s+"([^"]+)"/g))
    if (specifier) specifiers.push(specifier);
  return specifiers
    .filter((specifier) => specifier.startsWith("."))
    .map((specifier) => resolve(file, specifier))
    .filter((path): path is string => path !== null);
}

/** Every source module reached from these by static imports alone. */
function reached(entries: readonly string[]): Set<string> {
  const seen = new Set<string>();
  const queue = [...entries];
  while (queue.length) {
    const file = queue.pop() as string;
    if (seen.has(file)) continue;
    seen.add(file);
    queue.push(...imports(file));
  }
  return new Set([...seen].map((file) => relative(".", file)));
}

/** The task page: the root layout and its providers, the app's layout (the shell, the sidebar) and the page. */
const TASK_PAGE = [
  "src/app/layout.tsx",
  "src/app/providers.tsx",
  "src/app/(app)/layout.tsx",
  "src/app/(app)/c/[id]/page.tsx",
];

describe("WEB-U-014 a menu's open state is off the task page's first load", () => {
  it("reaches the menus' triggers but never their open state", () => {
    const first = reached(TASK_PAGE);
    // The walk sees the page: the ⋯ menus, the pickers and the account row are all on it.
    for (const trigger of [
      "src/ui/ActionMenu.tsx",
      "src/ui/MenuButton.tsx",
      "src/auth/AccountMenu.tsx",
      "src/conversations/ConversationPage.tsx",
    ])
      expect(first.has(trigger), trigger).toBe(true);
    for (const open of [
      "src/ui/MenuSurfaces.tsx",
      "src/ui/Popover.tsx",
      "src/ui/Menu.tsx",
    ])
      expect(first.has(open), open).toBe(false);
  });

  it("loads the open state with import(), from one place", () => {
    const loader = readFileSync("src/ui/lazy-menus.ts", "utf8");
    expect(loader).toMatch(/import\("\.\/MenuSurfaces"\)/);
  });
});
