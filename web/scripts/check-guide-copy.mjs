// DEV-GUIDELINES §2.3: the administrator's and user guides quote the product's own words. Every **bold** label, and
// every quoted sentence ending in a full stop, must still be English copy from the catalogue, so a copy change cannot
// leave the guides naming a button that no longer exists. Run from web/:
//   node scripts/check-guide-copy.mjs              check docs/admin-guide and docs/user-guide
//   node scripts/check-guide-copy.mjs --self-test  check the rule on fixtures
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const CATALOGUE = resolve("src", "i18n", "locales", "en.json");
const GUIDES = [
  resolve("..", "docs", "admin-guide"),
  resolve("..", "docs", "user-guide"),
];

/** Bold text in the guides that is prose emphasis, not a label. Keep it short: each entry says why. */
const NOT_LABELS = new Set([
  "bold", // "Labels in **bold** are the words on the screen."
]);

/** Separators a label shares a message with ("Shared · 3", "Quirks, comma-separated", "Extra body (JSON object)"). */
const PARTS = / · |, | \(/;

/**
 * A message's text with each ICU argument replaced: simple arguments by `hole`, and a select or plural by `hole` too,
 * while its branches' texts are collected into `branches`.
 */
function flatten(message, hole, branches) {
  let out = "";
  for (let at = 0; at < message.length; at++) {
    if (message[at] !== "{") {
      out += message[at];
      continue;
    }
    const end = closing(message, at);
    const body = message.slice(at + 1, end);
    const choice = /^\s*\w+\s*,\s*(?:select|plural)\s*,/.exec(body);
    if (choice) {
      for (const branch of body
        .slice(choice[0].length)
        .matchAll(/[^{}\s]+\s*\{/g)) {
        const open = choice[0].length + branch.index + branch[0].length - 1;
        branches.push(
          flatten(
            body.slice(open + 1, closing(body, open)),
            hole,
            branches,
          ).replace("#", hole),
        );
      }
    }
    out += hole;
    at = end;
  }
  return out;
}

/** The index of the brace that closes the one at `open`. */
function closing(text, open) {
  let depth = 0;
  for (let at = open; at < text.length; at++) {
    if (text[at] === "{") depth++;
    else if (text[at] === "}" && --depth === 0) return at;
  }
  throw new Error(`unbalanced braces in ${text}`);
}

/**
 * Every form a guide may quote a message in: the text with arguments as "…", without them, a select or plural
 * branch, and the parts around " · ", ", " and " (".
 * @param {Record<string, string>} messages the English catalogue's messages
 */
export function labelForms(messages) {
  const forms = new Set();
  const add = (text) => {
    const tidy = text.replace(/\s+/g, " ").trim();
    if (!tidy) return;
    forms.add(tidy);
    for (const part of tidy.split(PARTS))
      forms.add(part.replace(/\)$/, "").trim());
  };
  for (const message of Object.values(messages)) {
    const branches = [];
    add(flatten(message, "…", branches));
    add(flatten(message, "", []));
    branches.forEach(add);
  }
  forms.delete("");
  forms.delete("…");
  return forms;
}

/**
 * The guide text that is not catalogue copy, as "line: text".
 * @param {string} markdown one guide page
 * @param {Set<string>} forms from `labelForms`
 */
export function staleLabels(markdown, forms) {
  const quoted = [...forms];
  const known = (text) => forms.has(text) || NOT_LABELS.has(text);
  // A quoted sentence may be a message's first sentence: "Someone changed this route since you opened it."
  const sentence = (text) => quoted.some((form) => form.startsWith(text));
  // The page as one text, code spans removed line by line, so a label or sentence wrapped across a line break is read
  // whole (its break and indentation as one space); never across a blank line, and reported where it starts.
  const prose = markdown
    .split("\n")
    .map((line) => line.replace(/`[^`]*`/g, ""))
    .join("\n");
  const line = (offset) => prose.slice(0, offset).split("\n").length;
  const whole = (text) => text.replace(/[ \t]*\n[ \t]*/g, " ");
  const inParagraph = "(?:(?!\\n[ \\t]*\\n)";
  const bold = [
    ...prose.matchAll(
      new RegExp(`\\*\\*(${inParagraph}(?!\\*\\*)[\\s\\S])+?)\\*\\*`, "g"),
    ),
  ]
    .map((match) => ({
      at: match.index,
      text: whole(match[1]).replace(/:$/, ""),
    }))
    .filter(({ text }) => !known(text));
  const quotes = [
    ...prose.matchAll(new RegExp(`"(${inParagraph}[^"])+\\.)"`, "g")),
  ]
    .map((match) => ({ at: match.index, text: whole(match[1]) }))
    .filter(({ text }) => !sentence(text));
  return [...bold, ...quotes]
    .sort((first, second) => first.at - second.at)
    .map(({ at, text }) => `${line(at)}: ${text}`);
}

/**
 * The pages a guide must cover, as English labels: each Settings section (a `##` heading in the settings chapter) and
 * each page the sidebar links to (a user guide page titled with it). Read from the source, so a new page needs its guide.
 * @param {Record<string, string>} messages the English catalogue's messages
 * @param {{settings: string, links: string[]}} sources the Settings nav's source and each sidebar link's source
 */
export function coveredPages(messages, sources) {
  const keys = (source, pattern) =>
    [...source.matchAll(pattern)].map((match) => messages[match[1]]);
  return {
    settings: keys(sources.settings, /label: t\("(settings\.\w+)"\)/g),
    pages: sources.links.flatMap((source) =>
      keys(source, /nav-label">\{t\("([\w.]+)"\)\}/g),
    ),
  };
}

/**
 * The pages no guide covers, as "where: label".
 * @param {{settings: string[], pages: string[]}} labels from `coveredPages`
 * @param {{settings: string, user: string[]}} guides the settings chapter, and the user guide's pages
 */
export function uncoveredPages(labels, guides) {
  const headings = (markdown, level) =>
    new Set(
      [...markdown.matchAll(new RegExp(`^${level} (.+)$`, "gm"))].map((match) =>
        match[1].trim(),
      ),
    );
  const sections = headings(guides.settings, "##");
  const titles = new Set(
    guides.user.flatMap((page) => [...headings(page, "#")]),
  );
  return [
    ...labels.settings
      .filter((label) => !sections.has(label))
      .map((label) => `settings chapter: ${label}`),
    ...labels.pages
      .filter((label) => !titles.has(label))
      .map((label) => `user guide: ${label}`),
  ];
}

function guidePages() {
  return GUIDES.flatMap((directory) =>
    readdirSync(directory)
      .filter((name) => name.endsWith(".md"))
      .map((name) => join(directory, name)),
  );
}

function selfTest() {
  const forms = labelForms({
    "a.save": "Save route",
    "a.quirks": "Quirks, comma-separated",
    "a.role": "{role, select, admin {Administrator} other {Member}}",
    "a.shared": "Shared · {count, number}",
    "a.blocked":
      "Blocked network access to {destination} - it is not on the allow-list.",
    "a.conflict":
      "Someone changed this route since you opened it. Your edits are kept.",
    "a.scope": "Allow {scope} for this task",
    "a.count": "{count, plural, one {# task} other {# tasks}}",
  });
  const page = [
    "Choose **Save route**, **Quirks** or **Administrator**; a list lead like **Member:** works; so do **Shared**.",
    'It says "Blocked network access to … - it is not on the allow-list." or "Someone changed this route since you opened it."',
    '**Allow … for this task** in labels in **bold**; `"Code quotes."` and "prose without a stop" are skipped.',
    '**Save the route** was renamed, and "Nobody wrote this." is not copy.',
    // A label or sentence wrapped across a line break is still one: checked whole, reported where it starts.
    "Choose **Save",
    "route**; choose **Change",
    'model or environment** in the header, and "Someone changed this route since you',
    'opened it." stays copy.',
  ].join("\n");
  assert.deepEqual(staleLabels(page, forms), [
    "4: Save the route",
    "4: Nobody wrote this.",
    "6: Change model or environment",
  ]);
  assert.ok(forms.has("… tasks") && !forms.has("…"));
  assert.throws(() => labelForms({ "a.bad": "{open" }), /unbalanced/);
  const labels = coveredPages(
    {
      "settings.models": "Models",
      "settings.profiles": "Profiles",
      "rooms.title": "Rooms",
      "desks.title": "Desks",
    },
    {
      settings:
        '{ id: "models", label: t("settings.models") },\n{ id: "profiles", label: t("settings.profiles") }',
      links: [
        '<span className="nav-label">{t("rooms.title")}</span>',
        '<span className="nav-label">{t("desks.title")}</span>',
      ],
    },
  );
  assert.deepEqual(labels, {
    settings: ["Models", "Profiles"],
    pages: ["Rooms", "Desks"],
  });
  assert.deepEqual(
    uncoveredPages(labels, {
      settings: "# Settings pages\n## Models\n### Profiles\n",
      user: ["# Rooms\n## Desks\n"],
    }),
    ["settings chapter: Profiles", "user guide: Desks"],
  );
  console.log(
    "Guide copy self-test passed: labels, branches, parts, sentences, code, prose and page coverage.",
  );
}

if (process.argv.includes("--self-test")) selfTest();
else {
  const forms = labelForms(
    JSON.parse(readFileSync(CATALOGUE, "utf8")).messages,
  );
  const stale = guidePages().flatMap((path) =>
    staleLabels(readFileSync(path, "utf8"), forms).map(
      (found) => `${relative(resolve(".."), path)}:${found}`,
    ),
  );
  if (stale.length) {
    console.error(
      `Guide text that is not English copy (bold labels and quoted sentences; DEV-GUIDELINES §2.3):\n  ${stale.join("\n  ")}`,
    );
    process.exitCode = 1;
  } else console.log("Every label the guides quote is current English copy.");
  const read = (path) => readFileSync(path, "utf8");
  const shell = resolve("src", "shell");
  const uncovered = uncoveredPages(
    coveredPages(JSON.parse(read(CATALOGUE)).messages, {
      settings: read(resolve("src", "settings", "SettingsShell.tsx")),
      links: readdirSync(shell)
        .filter((name) => name.endsWith("Link.tsx"))
        .map((name) => read(join(shell, name))),
    }),
    {
      settings: read(join(GUIDES[0], "web-settings.md")),
      user: readdirSync(GUIDES[1])
        .filter((name) => name.endsWith(".md"))
        .map((name) => read(join(GUIDES[1], name))),
    },
  );
  if (uncovered.length) {
    console.error(
      `Pages the guides don't cover (a Settings section needs a "##" section in docs/admin-guide/web-settings.md; a sidebar page needs a user guide page titled with it):\n  ${uncovered.join("\n  ")}`,
    );
    process.exitCode = 1;
  } else console.log("Every Settings section and sidebar page has its guide.");
}
