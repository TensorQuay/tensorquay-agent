// @vitest-environment node
// WEB-U-498 (18.1, the 17.2 review's finding 11, decided on 18 Nov 2026): a form's disclosure (the route form's
// Advanced and Reasoning) is quiet: body size in regular weight, a ghost button's hover tint, its chevron kept, so it
// reads as a toggle beside the bold section labels.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

const css = readFileSync("src/app/settings.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);

function rule(selector: string): string {
  const found: string[] = [];
  for (const match of css.matchAll(/([^{};]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "").split(",").map((part) => part.trim());
    if (selectors.includes(selector)) found.push(match[2] ?? "");
  }
  return found.join(";");
}

describe("WEB-U-498 a form's disclosure is quiet", () => {
  it("is body size in regular weight, with a ghost button's padding and radius", () => {
    const quiet = rule("details.route-group > summary.disclosure-quiet");
    expect(quiet).toMatch(/font-weight:\s*400\b/);
    expect(quiet).toMatch(/font-size:\s*var\(--body-size\)/);
    expect(quiet).toMatch(/border-radius:\s*var\(--control-radius\)/);
    expect(quiet).toMatch(/padding-inline:\s*var\(--space-2\)/);
  });

  it("tints on hover as a ghost button does, and keeps its chevron", () => {
    expect(
      rule("details.route-group > summary.disclosure-quiet:hover"),
    ).toMatch(/var\(--hover-share\)/);
    expect(rule("details.route-group > summary::before")).toMatch(
      /transform:\s*rotate\(-45deg\)/,
    );
  });

  it("shows a focus ring for the keyboard", () => {
    expect(
      rule("details.route-group > summary.disclosure-quiet:focus-visible"),
    ).toMatch(/outline/);
  });
});

// One rule for every disclosure (decided on 18 Nov 2026): every Settings form's <summary>, and its group's chevron.
describe("WEB-U-498 every Settings disclosure is the quiet one", () => {
  it("draws each <summary> under src/settings as a quiet disclosure in a route-group", () => {
    const files = readdirSync("src/settings", {
      recursive: true,
      encoding: "utf8",
    }).filter((name) => name.endsWith(".tsx") && !name.endsWith(".test.tsx"));
    const found: string[] = [];
    for (const name of files) {
      const source = readFileSync(join("src/settings", name), "utf8");
      for (const match of source.matchAll(
        /<details\b([\s\S]*?)<summary\b([^>]*)>/g,
      ))
        found.push(`${name}: ${match[1]!.trim()} ${match[2]!.trim()}`);
    }
    expect(found.length).toBeGreaterThanOrEqual(5);
    const loud = found.filter(
      (entry) =>
        !/className="[^"]*\broute-group\b[^"]*"[\s\S]*className="disclosure-quiet"/.test(
          entry,
        ),
    );
    expect(loud).toEqual([]);
  });
});
