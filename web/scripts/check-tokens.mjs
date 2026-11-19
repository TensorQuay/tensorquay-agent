import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
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

// UI-GUIDELINES §3 to §5 (WEB-U-509, 19 Nov 2026): a type size, weight, line height, letter spacing or radius is one of
// the tokens, and a colour is never a name. Each rule has a short name, which an allowance comment cites.
const RADIUS_TOKENS = [
  "control-radius",
  "pill-radius",
  "dialog-radius",
  "composer-radius",
  "row-action-radius",
  "thumbnail-radius",
];
const SIZE_TOKENS = [
  "display-size",
  "heading-size",
  "body-size",
  "small-size",
  "label-size",
  "nav-font-size",
];
const NAMED_COLOURS =
  "white|black|red|green|blue|gr[ae]y|yellow|orange|purple|pink|brown|silver|navy|maroon|olive|teal|aqua|fuchsia|lime|gold|indigo|violet|cyan|magenta|beige|ivory|tan|salmon|crimson|coral|khaki|lavender|plum|orchid|turquoise|tomato|wheat|snow|azure|linen|chocolate|firebrick|darkred|darkgr[ae]y|lightgr[ae]y|dimgr[ae]y|whitesmoke|gainsboro";
const TAILWIND_COLOURS =
  "white|black|slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose";
const token = (names) => new RegExp(`^var\\(--(?:${names.join("|")})\\)$`);
/** A value's parts at the top level: `var(--a) 0 calc(…)` is three. */
const parts = (value) =>
  value
    .trim()
    .match(/(?:var|calc|min|max|clamp)\((?:[^()]|\([^()]*\))*\)|[^\s,]+/g) ??
  [];
const each = (allowed) => (value) =>
  parts(value).every((part) => allowed(part));
const keywords =
  (...words) =>
  (part) =>
    words.includes(part.toLowerCase());
const RULES = [
  {
    rule: "font-size",
    name: "font size off the type tokens",
    css: /(?<![\w-])font-size\s*:\s*([^;}]*)/gi,
    script: /\bfontSize\s*:\s*([^,}\n]*)/g,
    tailwind:
      /(?:^|[\s"'`])text-(?:xs|sm|base|lg|[2-9]?xl|\[[^\]]*\])(?=[\s"'`]|$)/gm,
    allowed: each(
      (part) => token(SIZE_TOKENS).test(part) || keywords("inherit")(part),
    ),
  },
  {
    rule: "font-weight",
    name: "font weight other than 400 or 700",
    css: /(?<![\w-])font-weight\s*:\s*([^;}]*)/gi,
    script: /\bfontWeight\s*:\s*([^,}\n]*)/g,
    tailwind:
      /(?:^|[\s"'`])font-(?:thin|extralight|light|medium|semibold|extrabold|black)(?=[\s"'`]|$)/gm,
    allowed: each(
      keywords("400", "700", "normal", "bold", "inherit", '"400"', '"700"'),
    ),
  },
  {
    rule: "line-height",
    name: "line height off the leading tokens",
    css: /(?<![\w-])line-height\s*:\s*([^;}]*)/gi,
    script: /\blineHeight\s*:\s*([^,}\n]*)/g,
    tailwind:
      /(?:^|[\s"'`])leading-(?:none|tight|snug|normal|relaxed|loose|\d+|\[[^\]]*\])(?=[\s"'`]|$)/gm,
    allowed: each(
      (part) =>
        /^var\(--(?:[\w-]+-leading|nav-line-height|heading-line-height)\)$/.test(
          part,
        ) || keywords("inherit", "normal")(part),
    ),
  },
  {
    rule: "tracking",
    name: "letter spacing off the tracking tokens",
    css: /(?<![\w-])letter-spacing\s*:\s*([^;}]*)/gi,
    script: /\bletterSpacing\s*:\s*([^,}\n]*)/g,
    tailwind:
      /(?:^|[\s"'`])tracking-(?:tighter|tight|normal|wide|wider|widest|\[[^\]]*\])(?=[\s"'`]|$)/gm,
    allowed: each(
      (part) =>
        /^var\(--[\w-]+-tracking\)$/.test(part) ||
        keywords("inherit", "normal")(part),
    ),
  },
  {
    rule: "radius",
    name: "radius off the radius tokens",
    css: /(?<![\w-])border(?:-[a-z]+)*-radius\s*:\s*([^;}]*)/gi,
    script: /\bborder(?:[A-Z][a-z]+)*Radius\s*:\s*([^,}\n]*)/g,
    tailwind:
      /(?:^|[\s"'`])rounded(?:-(?:none|sm|md|lg|[2-3]?xl|full|[trblse]{1,2}(?:-(?!control|pill|dialog)[a-z0-9]+)?|\[[^\]]*\]))?(?=["'`]|\s+[\w:[-]+["'`\s])/gm,
    allowed: each(
      (part) =>
        token(RADIUS_TOKENS).test(part) || keywords("0", "inherit")(part),
    ),
  },
  {
    rule: "named-colour",
    name: "named colour",
    css: new RegExp(
      `(?<![\\w-])(?:color|background(?:-color)?|border(?:-[a-z]+)*(?:-color)?|outline(?:-color)?|fill|stroke|caret-color|accent-color|text-decoration-color|box-shadow)\\s*:\\s*([^;}]*\\b(?:${NAMED_COLOURS})\\b[^;}]*)`,
      "gi",
    ),
    script: new RegExp(
      `\\b(?:color|background(?:Color)?|border(?:[A-Z][a-z]+)*Color|fill|stroke)\\s*:\\s*["'](${NAMED_COLOURS})["']`,
      "gi",
    ),
    tailwind: new RegExp(
      `(?:^|[\\s"'\`])(?:bg|text|border|fill|stroke|ring|outline|from|via|to|decoration|accent|caret)-(?:${TAILWIND_COLOURS})(?:-\\d{2,3})?(?=[\\s"'\`/]|$)`,
      "gm",
    ),
    allowed: () => false,
  },
];

/** An allowance: `token-lint: allow <rule> (<reason of three words or more>)` on the line or the line before. */
const ALLOWANCE = /token-lint: allow ([a-z-]+) \(([^)]*)\)/g;
const ALLOWANCE_ANY = /token-lint: allow\b/g;

/** A className's text: a string, or a template literal in braces. */
const CLASS_NAMES =
  /className=(?:"([^"]*)"|'([^']*)'|\{\s*`([^`]*)`\s*\}|\{\s*"([^"]*)"\s*\})/g;

/** The new rules' findings in one file, each with its rule, and whether an allowance covers it. */
function typeRules(source, path = "") {
  const lines = source.split("\n");
  const lineOf = (index) => source.slice(0, index).split("\n").length;
  const scripted = /\.(?:tsx?|jsx?|mjs)$/.test(path) || path === "";
  const found = [];
  for (const { rule, name, css, script, tailwind, allowed } of RULES) {
    for (const match of source.matchAll(css)) {
      if (rule !== "named-colour" && allowed(match[1] ?? "")) continue;
      found.push({ rule, name, line: lineOf(match.index) });
    }
    if (!scripted) continue;
    for (const match of source.matchAll(script)) {
      const value = (match[1] ?? "").trim().replace(/^["'`]|["'`]$/g, "");
      if (rule !== "named-colour" && allowed(value)) continue;
      // A number in a style object is px: never a token.
      found.push({ rule, name, line: lineOf(match.index) });
    }
    // Utility classes count only inside a className, so prose ("rounded to whole pixels") never does.
    for (const classes of source.matchAll(CLASS_NAMES)) {
      const text = classes.slice(1).find((value) => value !== undefined) ?? "";
      const start = classes.index + classes[0].indexOf(text);
      for (const match of ` ${text} `.matchAll(tailwind))
        found.push({
          rule,
          name,
          line: lineOf(start + Math.max(0, match.index - 1)),
        });
    }
  }
  const allowances = [];
  const results = [];
  for (const finding of found) {
    const here = [
      lines[finding.line - 1] ?? "",
      lines[finding.line - 2] ?? "",
    ].join("\n");
    const cited = [...here.matchAll(ALLOWANCE)].find(
      (match) =>
        match[1] === finding.rule && match[2].trim().split(/\s+/).length >= 3,
    );
    if (cited) allowances.push({ ...finding, reason: cited[2].trim() });
    else results.push(finding);
  }
  // An allowance must cite a rule and give a reason; a bare one is itself refused.
  for (const match of source.matchAll(ALLOWANCE_ANY)) {
    const line = lines[lineOf(match.index) - 1] ?? "";
    const full = [...line.matchAll(ALLOWANCE)][0];
    if (
      !full ||
      !RULES.some(({ rule }) => rule === full[1]) ||
      full[2].trim().split(/\s+/).length < 3
    )
      results.push({
        rule: "allowance",
        name: "token-lint allowance without a known rule and a reason of three words or more",
        line: lineOf(match.index),
      });
  }
  return { results, allowances };
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
    // The type, radius and colour-name rules hold in the token file too: its token definitions aren't properties.
    // Tests are left out of them: they carry CSS as fixtures and as the patterns they check (density, disclosures).
    const typed = /\.test\.[jt]sx?$/.test(path)
      ? { results: [], allowances: [] }
      : typeRules(source, path);
    for (const { rule } of typed.allowances) ALLOWED.push(`${path}:${rule}`);
    return [...found, ...typed.results].map(
      ({ name, line }) => `${path}:${line}: ${name}`,
    );
  });
}

/** Every allowance found by the scan, as "path:rule". */
const ALLOWED = [];
const ALLOWANCES_FILE = join("scripts", "token-allowances.txt");

/** The allowances found must be the committed list's, so a new one shows in a diff and is reviewed. */
function allowanceList() {
  const committed = existsSync(ALLOWANCES_FILE)
    ? readFileSync(ALLOWANCES_FILE, "utf8")
        .split("\n")
        .filter((line) => line && !line.startsWith("#"))
    : [];
  const found = [...new Set(ALLOWED)].sort();
  const extra = found.filter((entry) => !committed.includes(entry));
  const gone = committed.filter((entry) => !found.includes(entry));
  return [
    ...extra.map(
      (entry) =>
        `${entry}: an allowance missing from ${ALLOWANCES_FILE} (add it, so the review sees it)`,
    ),
    ...gone.map(
      (entry) =>
        `${entry}: listed in ${ALLOWANCES_FILE} but no longer allowed (remove it)`,
    ),
  ];
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
  // WEB-U-509: the type, radius and colour-name rules, in CSS, style objects and class names.
  const typedPass = [
    ".a { font-size: var(--small-size); font-weight: 700; line-height: var(--small-leading); }",
    ".b { letter-spacing: var(--label-tracking); border-radius: var(--control-radius) var(--control-radius) 0 0; }",
    ".c { border-radius: var(--pill-radius); font-weight: inherit; line-height: var(--nav-line-height); }",
    ".d { color: var(--ink); border: 1px solid var(--line); background: transparent; }",
    "<p style={{ fontSize: 'var(--body-size)', fontWeight: 400, borderRadius: 'var(--dialog-radius)' }} />",
    '<p className="flex items-center gap-2" /> // rounded to a whole percent, in red ink',
  ].join("\n");
  assert.deepEqual(typeRules(typedPass, "a.tsx").results, []);
  for (const [literal, rule] of [
    [".a { font-size: 15px; }", "font-size"],
    [".a { font-size: var(--space-4); }", "font-size"],
    ["<p style={{ fontSize: 14 }} />", "font-size"],
    ['<p className="text-sm" />', "font-size"],
    [".a { font-weight: 500; }", "font-weight"],
    ['<p className="font-semibold" />', "font-weight"],
    [".a { line-height: 22px; }", "line-height"],
    [".a { line-height: var(--min-control-height); }", "line-height"],
    ['<p className="leading-tight" />', "line-height"],
    [".a { letter-spacing: 0.02em; }", "tracking"],
    [".a { border-radius: 50%; }", "radius"],
    [".a { border-radius: var(--space-1); }", "radius"],
    [".a { border-top-left-radius: var(--radius-control); }", "radius"],
    ['<p className="rounded-md p-2" />', "radius"],
    ["<p style={{ borderRadius: 4 }} />", "radius"],
    [".a { color: white; }", "named-colour"],
    [".a { border: 1px solid black; }", "named-colour"],
    ['<p className="bg-white" />', "named-colour"],
  ]) {
    const { results } = typeRules(literal, "a.tsx");
    assert.deepEqual(
      results.map((result) => result.rule),
      [rule],
      literal,
    );
  }
  // An allowance cites the rule and a reason of three words or more, on the line or the line before.
  const cited = typeRules(
    "/* token-lint: allow line-height (the PDF text layer matches its glyph boxes) */\n.a { line-height: 1; }",
    "a.css",
  );
  assert.deepEqual(cited.results, []);
  assert.deepEqual(
    cited.allowances.map(({ rule }) => rule),
    ["line-height"],
  );
  for (const refused of [
    "/* token-lint: allow radius (the PDF text layer matches) */\n.a { line-height: 1; }",
    "/* token-lint: allow line-height (because) */\n.a { line-height: 1; }",
    "/* token-lint: allow */\n.a { line-height: 1; }",
  ])
    assert.ok(typeRules(refused, "a.css").results.length > 0, refused);
  console.log(
    "Token self-test passed: compliant fixture and 13 forbidden fixtures; undefined variables found; 18 type, radius and colour-name fixtures; allowances cited, refused when wrong.",
  );
}

if (process.argv.includes("--self-test")) selfTest();
else {
  const failures = [
    ...scan("src"),
    ...undefinedVariables(sources("src")),
    ...allowanceList(),
  ];
  const allowed = [...new Set(ALLOWED)].length;
  if (allowed)
    console.log(
      `Token allowances: ${allowed} (${ALLOWANCES_FILE}), each with its reason beside it.`,
    );
  for (const failure of failures) console.error(failure);
  if (failures.length) process.exitCode = 1;
  else
    console.log("Token check passed: all source styles use the shared tokens.");
}
