// @vitest-environment node
// WEB-U-089 (the UI standard: one system): every control shows the same focus ring, one shared rule on three tokens,
// a 2 px accent ring at a 2 px offset. No stylesheet draws its own colour, width or offset.
import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, "");
const sheets = readdirSync("src/app")
  .filter((file) => file.endsWith(".css"))
  .map(
    (file) => [file, strip(readFileSync(`src/app/${file}`, "utf8"))] as const,
  );
const globals = sheets.find(([file]) => file === "globals.css")?.[1] ?? "";
/** A custom property's value where globals.css declares it (its first declaration: the light and base value). */
const token = (name: string) =>
  globals.match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1]?.trim();
/** A custom property's value in each block that declares it, in order: light, then the dark blocks. */
const values = (name: string) =>
  [...globals.matchAll(new RegExp(`--${name}:\\s*([^;]+);`, "g"))].map(
    (match) => (match[1] ?? "").trim(),
  );
/** A token's light and dark values as UI-GUIDELINES §3's colour table lists them: the code follows the guideline. */
const guideline = readFileSync("../docs/UI-GUIDELINES.md", "utf8");
function documented(name: string): [string, string] {
  const row = guideline.match(
    new RegExp(`^\\| \`--${name}\` \\| \`([^\`]+)\` \\| \`([^\`]+)\` \\|`, "m"),
  );
  if (!row?.[1] || !row[2]) throw new Error(`UI-GUIDELINES lists no --${name}`);
  return [row[1], row[2]];
}
/** A hex colour's hue in degrees. */
function hue(hex: string) {
  const [r, g, b] = [1, 3, 5].map(
    (at) => parseInt(hex.slice(at, at + 2), 16) / 255,
  ) as [number, number, number];
  const max = Math.max(r, g, b);
  const span = max - Math.min(r, g, b);
  if (span === 0) return 0;
  const raw =
    max === r
      ? ((g - b) / span) % 6
      : max === g
        ? (b - r) / span + 2
        : (r - g) / span + 4;
  return (raw * 60 + 360) % 360;
}
const apart = (a: number, b: number) =>
  Math.min(Math.abs(a - b), 360 - Math.abs(a - b));

/** Every rule whose selector names a focus state, with its declarations. */
const focusRules = (css: string) =>
  [...css.matchAll(/([^{};]+)\{([^{}]*)\}/g)]
    .filter((match) => (match[1] ?? "").includes(":focus"))
    .map((match) => [(match[1] ?? "").trim(), match[2] ?? ""] as const);

describe("WEB-U-089 one focus ring", () => {
  // A calm blue of its own, never the orange accent: an orange ring on a field reads as an error.
  it("is a 2 px blue ring at a 2 px offset, the same in every theme block", () => {
    const [light, dark] = documented("focus");
    expect(values("focus")).toEqual([light, dark, dark]);
    expect(token("focus-width")).toBe("2px");
    expect(token("focus-offset")).toBe("2px");
  });

  it("is drawn from those tokens on every focus state, never with a stylesheet's own numbers", () => {
    for (const [file, css] of sheets)
      for (const [selector, body] of focusRules(css)) {
        for (const [, value] of body.matchAll(/outline-offset:\s*([^;]+)/g))
          expect((value ?? "").trim(), `${file} ${selector}`).toBe(
            "var(--focus-offset)",
          );
        for (const [, value] of body.matchAll(/(?:^|[\s;])outline:\s*([^;]+)/g))
          expect(
            ["none", "var(--focus-width) solid var(--focus)"],
            `${file} ${selector}: outline ${value}`,
          ).toContain((value ?? "").trim());
      }
  });

  // Danger is red and the accent orange, apart by hue in every theme: in dark mode they had drifted to a few degrees.
  it("keeps danger's hue clearly apart from the accent's", () => {
    const danger = values("danger");
    const [light, dark] = documented("danger");
    expect(danger).toEqual([light, dark, dark]);
    const accent = values("accent");
    expect(danger).toHaveLength(accent.length);
    for (const [index, colour] of danger.entries())
      expect(
        apart(hue(colour), hue(accent[index] ?? "")),
      ).toBeGreaterThanOrEqual(20);
  });
});
