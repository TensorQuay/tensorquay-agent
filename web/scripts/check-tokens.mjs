import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

function violations(source) {
  const rules = [
    ["colour literal", /#[\da-f]{3,8}\b|\b(?:rgba?|hsla?|oklch)\s*\(/gi],
    ["font-family declaration", /\bfont-?family["']?\s*:/gi],
    [
      "radius or shadow literal",
      /[\w-]*(?:radius|shadow)["']?\s*:[^;}]*?\d+(?:\.\d+)?px|\b(?:rounded|shadow)(?:-[a-z]+)?-\[[^\]]*\d+(?:\.\d+)?px[^\]]*\]/gi,
    ],
  ];
  return rules.flatMap(([name, pattern]) =>
    [...source.matchAll(pattern)].map((match) => ({
      name,
      line: source.slice(0, match.index).split("\n").length,
    })),
  );
}

// UI-GUIDELINES §5 and §5b: spacing comes only from the --space-* scale, never from a literal length.
const SPACING_DECLARATION =
  /\b(?:padding|margin|gap|row-gap|column-gap)(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?\s*:\s*([^;}]*)/gi;
const SPACING_PROPERTY =
  /\b(?:padding|margin|gap|rowGap|columnGap)(?:Top|Right|Bottom|Left|Inline|Block)?\s*:\s*["']?(?:[1-9]|0\.\d)/g;
const SPACING_UTILITY = /\b(?:p[xytrbl]?|m[xytrbl]?|gap(?:-[xy])?)-\[[^\]]*\d/g;
const LENGTH = /(?<![\w-])(?:\d*\.)?\d+(?:px|rem|em)\b/;

function spacing(source) {
  const lines = (index) => source.slice(0, index).split("\n").length;
  const literal = [...source.matchAll(SPACING_DECLARATION)]
    .filter((match) => LENGTH.test(match[1] ?? ""))
    .map((match) => lines(match.index));
  const scripted = [
    ...source.matchAll(SPACING_PROPERTY),
    ...source.matchAll(SPACING_UTILITY),
  ].map((match) => lines(match.index));
  return [...literal, ...scripted].map((line) => ({
    name: "spacing literal off the --space-* scale",
    line,
  }));
}

// A custom property's definitions: a CSS declaration (`--name:`), or one a script sets (`"--name": …` in a style
// object, or `setProperty("--name", …)`).
const DEFINED = [
  /(?<![\w-])(--[\w-]+)\s*:/g,
  /["'](--[\w-]+)["']\s*:/g,
  /setProperty\(\s*["'](--[\w-]+)["']/g,
];
// A use with no fallback of its own: `var(--name)`.
const USED = /var\(\s*(--[\w-]+)\s*\)/g;

/** Each use of a custom property that no source defines, as "path:line: …" (sources are [text, path] pairs). */
function undefinedVariables(sources) {
  const defined = new Set(
    sources.flatMap(([text]) =>
      DEFINED.flatMap((pattern) =>
        [...text.matchAll(pattern)].map((match) => match[1]),
      ),
    ),
  );
  return sources.flatMap(([text, path]) =>
    [...text.matchAll(USED)]
      .filter((match) => !defined.has(match[1]))
      .map(
        (match) =>
          `${path}:${text.slice(0, match.index).split("\n").length}: ${match[1]} is used but never defined`,
      ),
  );
}

/** Every source file under a directory, as [text, path], leaving generated code out. */
function sources(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === "generated") return [];
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sources(path);
    return [[readFileSync(path, "utf8"), path]];
  });
}

function scan(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === "generated") return [];
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return scan(path);
    const source = readFileSync(path, "utf8");
    // The token file defines the literals; spacing declarations there still use the scale.
    const found =
      path === join("src", "app", "globals.css")
        ? spacing(source)
        : [...violations(source), ...spacing(source)];
    return found.map(({ name, line }) => `${path}:${line}: ${name}`);
  });
}

function selfTest() {
  const pass =
    "color: var(--ink); border-radius: var(--control-radius); padding: 0 var(--space-3); gap: var(--space-2);";
  const fail = "color: #abcdef; font-family: serif; border-radius: 6px;";
  assert.deepEqual(violations(pass), []);
  assert.equal(violations(fail).length, 3);
  for (const literal of [
    "rgb(1 2 3)",
    "hsl(0 0% 0%)",
    "oklch(0 0 0)",
    "boxShadow: '0 2px 4px'",
    "rounded-[6px]",
    "padding: 10px;",
    "margin-top: 0.5rem;",
    "gap: 6px;",
    "column-gap: 1em;",
    "style={{ paddingLeft: 10 }}",
    "p-[10px]",
    "gap-[6px]",
  ]) {
    assert.ok(
      violations(literal).length + spacing(literal).length > 0,
      literal,
    );
  }
  assert.deepEqual(spacing(pass), []);
  // A variable used but never defined draws nothing (the settings callout's --border did): each use needs a definition
  // somewhere in the sources, a CSS declaration or a style a script sets; a use with its own fallback stands alone.
  assert.deepEqual(
    undefinedVariables([
      [":root { --ink: #000; } [data-tone] { --status-color: red; }", "a.css"],
      ["a { color: var(--ink); border-color: var(--status-color); }", "b.css"],
      ['<p style={{ "--progress": "40%" }} />', "c.tsx"],
      [
        "b { width: var(--progress); color: var(--later, var(--ink)); }",
        "d.css",
      ],
      ["el.style.setProperty('--dragged', '1');", "e.ts"],
      ["i { opacity: var(--dragged); }", "f.css"],
    ]),
    [],
  );
  assert.deepEqual(
    undefinedVariables([
      [":root { --line: #ccc; }", "a.css"],
      ["a {\n  border: 1px solid var(--border);\n}", "b.css"],
      ["<i style={{ color: 'var(--nope)' }} />", "c.tsx"],
    ]),
    [
      "b.css:2: --border is used but never defined",
      "c.tsx:1: --nope is used but never defined",
    ],
  );
  console.log(
    "Token self-test passed: compliant fixture and 13 forbidden fixtures; undefined variables found.",
  );
}

if (process.argv.includes("--self-test")) selfTest();
else {
  const failures = [...scan("src"), ...undefinedVariables(sources("src"))];
  for (const failure of failures) console.error(failure);
  if (failures.length) process.exitCode = 1;
  else
    console.log("Token check passed: all source styles use the shared tokens.");
}
