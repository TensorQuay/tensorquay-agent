// TEST: GW-U-027
// GW-U-027 (FR-26): the web calls only documented gateway routes, and every URL it sends comes from one table.
//   node scripts/api-parity.mjs              self-test, then check src/ against crates/tqa-gateway/openapi.json
//   node scripts/api-parity.mjs --self-test  only the negative controls
// Rules:
//   1. Each entry of the operations tables names a path and method in the OpenAPI document: `operations`
//      (src/api/operations.ts, every page) and `settingsOperations` (src/settings/operations.ts, only the settings pages
//      load it, so the conversation page's scripts carry none of them: WEB-U-014).
//   2. No other source text holds a gateway path: a string or template literal starting with a gateway prefix (a
//      hand-built fetch URL, an EventSource URL, an href or src) is refused outside the table.
//   3. Network primitives (fetch, EventSource, XMLHttpRequest, WebSocket, sendBeacon) appear only in src/api/, so a URL
//      built at runtime cannot reach the network except through the audited client.
//   5. An operation documented ahead of its gateway code carries `x-tqa-planned` naming the slice that builds it
//      ("ADR-0039 slice 5"; GW-U-022 keeps it unserved until then): the table may call it, and a marker that names no
//      slice is refused.
//   4. Every error or reason code the web gives its own words (compared as `error.code === "…"`, or a key of a
//      `Readonly<Record<string, MessageKey>>` map) is named in the OpenAPI document or the protocol schema, unless it
//      is one of the web's own client-side codes: a code the gateway doesn't document, or never sends, is refused.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";

import ts from "typescript";

const ROOT = "src";
const TABLE = join("src", "api", "operations.ts");
const SETTINGS_TABLE = join("src", "settings", "operations.ts");
const TABLES = [TABLE, SETTINGS_TABLE];
const CLIENT = `${join("src", "api")}${sep}`;
const OPENAPI = join("..", "crates", "tqa-gateway", "openapi.json");
const PROTOCOL = join(
  "..",
  "crates",
  "tqa-types",
  "schema",
  "tqa-protocol-v1.json",
);
/** Codes the web makes itself, never the gateway's (src/api/error.ts and the clipboard). */
const WEB_CODES = new Set([
  "invalid_response", // an answer that fails its schema
  "request_failed", // the network failed before an answer
  "randomness_unavailable", // no crypto.getRandomValues for an idempotency key
  "copy_failed", // the clipboard refused
]);
// A gateway path starts with one of its top-level segments, alone or followed by "/" or "?" ("/api" + "/v1/…" counts).
const GATEWAY = /^\/(api|readyz|healthz|relay|runtime|metrics)(\/|\?|$)/;
const METHODS = ["get", "post", "put", "patch", "delete"];
/** Rule 5: what a planned operation's marker says. */
const PLANNED = /^ADR-\d{4} slice \d+$/;
const PRIMITIVES = new Set([
  "fetch",
  "EventSource",
  "XMLHttpRequest",
  "WebSocket",
  "sendBeacon",
]);

/** Every product source file: no tests, no generated code. */
function sources(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory())
      return entry.name === "generated" ? [] : sources(path);
    return /\.(ts|tsx)$/.test(entry.name) &&
      !/\.test\.(ts|tsx)$/.test(entry.name)
      ? [path]
      : [];
  });
}

const gatewayPath = (text) => GATEWAY.test(text);

/** The `{ method, path }` entries of the operations table, read from its object literal. */
function tableEntries(file, text) {
  const entries = [];
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const visit = (node) => {
    if (
      ts.isPropertyAssignment(node) &&
      ts.isObjectLiteralExpression(node.initializer)
    ) {
      const fields = Object.fromEntries(
        node.initializer.properties
          .filter(
            (property) =>
              ts.isPropertyAssignment(property) &&
              ts.isStringLiteral(property.initializer),
          )
          .map((property) => [
            property.name.getText(source),
            property.initializer.text,
          ]),
      );
      if (fields.method && fields.path)
        entries.push({
          name: node.name.getText(source),
          method: fields.method.toLowerCase(),
          path: fields.path,
          line:
            source.getLineAndCharacterOfPosition(node.getStart(source)).line +
            1,
        });
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return entries;
}

/** Gateway path literals and network primitives outside where they belong. */
function misuse(file, text) {
  const problems = [];
  const source = ts.createSourceFile(
    file,
    text,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const where = (node) =>
    `${file}:${source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1}`;
  const inClient = file.startsWith(CLIENT);
  const visit = (node) => {
    const literal =
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node)
        ? node.text
        : null;
    if (literal !== null && !TABLES.includes(file) && gatewayPath(literal))
      problems.push(
        `${where(node)} builds a gateway URL outside the operations table: ${JSON.stringify(literal.slice(0, 60))}`,
      );
    const callee =
      ts.isCallExpression(node) || ts.isNewExpression(node)
        ? node.expression
        : null;
    const name =
      callee &&
      (ts.isIdentifier(callee)
        ? callee.text
        : ts.isPropertyAccessExpression(callee)
          ? callee.name.text
          : null);
    if (name && PRIMITIVES.has(name) && !inClient)
      problems.push(
        `${where(node)} uses ${name} outside src/api/; send requests through the API client`,
      );
    ts.forEachChild(node, visit);
  };
  visit(source);
  return problems;
}

/** Checks a set of files against an OpenAPI document; returns problems and unused documented operations. */
export function analyse(files, openapi) {
  const problems = [];
  const table = files.find((file) => file.path === TABLE);
  if (!table || tableEntries(table.path, table.text).length === 0)
    problems.push(`${TABLE} has no operations table`);
  const entries = files
    .filter((file) => TABLES.includes(file.path))
    .flatMap((file) =>
      tableEntries(file.path, file.text).map((entry) => ({
        ...entry,
        file: file.path,
      })),
    );
  const names = entries.map((entry) => entry.name);
  for (const entry of entries) {
    if (!openapi.paths?.[entry.path]?.[entry.method])
      problems.push(
        `${entry.file}:${entry.line} ${entry.name}: ${entry.method.toUpperCase()} ${entry.path} is not in the OpenAPI document`,
      );
    if (names.indexOf(entry.name) !== names.lastIndexOf(entry.name))
      problems.push(
        `${entry.file}:${entry.line} ${entry.name} is named in both operations tables`,
      );
  }
  for (const file of files) problems.push(...misuse(file.path, file.text));
  for (const [path, item] of Object.entries(openapi.paths ?? {}))
    for (const method of METHODS) {
      const marker = item[method]?.["x-tqa-planned"];
      if (
        marker !== undefined &&
        !(typeof marker === "string" && PLANNED.test(marker))
      )
        problems.push(
          `${method.toUpperCase()} ${path}: x-tqa-planned must name an ADR's slice ("ADR-0039 slice 5"), not ${JSON.stringify(marker)}`,
        );
    }
  const used = new Set(entries.map((entry) => `${entry.method} ${entry.path}`));
  const unused = Object.entries(openapi.paths ?? {}).flatMap(([path, item]) =>
    METHODS.filter(
      (method) => item[method] && !used.has(`${method} ${path}`),
    ).map((method) => `${method.toUpperCase()} ${path}`),
  );
  return { problems, unused, operations: entries.length };
}

/** Rule 4: the codes the web gives its own words, with where; each must be named in `documented` (a document's text). */
export function codeProblems(files, documented) {
  const seen = new Map();
  const note = (code, where) =>
    seen.set(code, [...(seen.get(code) ?? []), where]);
  for (const file of files) {
    for (const match of file.text.matchAll(
      /(?:error|refused\??|refusal|frame|failure)\.code [!=]== "([a-z0-9_]+)"/g,
    ))
      note(match[1], file.path);
    for (const map of file.text.matchAll(
      /const [A-Z_]+: Readonly<\s*Record<string, MessageKey>\s*> = \{([\s\S]*?)\n\};/g,
    ))
      for (const key of (map[1] ?? "").matchAll(/^\s*"?([a-z0-9_]+)"?:/gm))
        note(key[1] ?? "", file.path);
  }
  return [...seen]
    .filter(
      ([code]) =>
        !WEB_CODES.has(code) && !new RegExp(`\\b${code}\\b`).test(documented),
    )
    .map(
      ([code, where]) =>
        `${[...new Set(where)].join(", ")}: the code "${code}" is named in neither the OpenAPI document nor the protocol schema`,
    );
}

// Negative controls: each fixture must be refused for its stated reason, and the compliant set must pass.
const documented = {
  paths: {
    "/api/v1/things/{id}": { get: {} },
    "/api/v1/things": { post: {} },
    "/api/v1/later": { get: { "x-tqa-planned": "ADR-0039 slice 5" } },
  },
};
const table = (body) => ({
  path: TABLE,
  text: `export const operations = {\n${body}\n} as const;`,
});
const settingsTable = (body) => ({
  path: SETTINGS_TABLE,
  text: `export const settingsOperations = {\n${body}\n} as const;`,
});
const good = table(
  `  thing: { method: "GET", path: "/api/v1/things/{id}" },\n  create: { method: "POST", path: "/api/v1/things" },`,
);
const CONTROLS = [
  [
    "an undocumented route in the table",
    [table(`  secret: { method: "GET", path: "/api/v1/private" },`)],
    /GET \/api\/v1\/private is not in the OpenAPI document/,
  ],
  [
    "an undocumented route in the settings table",
    [
      table(`  thing: { method: "GET", path: "/api/v1/things/{id}" },`),
      settingsTable(`  hidden: { method: "GET", path: "/api/v1/hidden" },`),
    ],
    /settings.operations\.ts:\d+ hidden: GET \/api\/v1\/hidden is not in the OpenAPI document/,
  ],
  [
    "one name in both tables",
    [
      table(`  thing: { method: "GET", path: "/api/v1/things/{id}" },`),
      settingsTable(`  thing: { method: "POST", path: "/api/v1/things" },`),
    ],
    /thing is named in both operations tables/,
  ],
  [
    "a wrong method on a documented path",
    [table(`  thing: { method: "DELETE", path: "/api/v1/things/{id}" },`)],
    /DELETE \/api\/v1\/things\/\{id\} is not in/,
  ],
  [
    "a hand-built URL",
    [
      good,
      {
        path: join("src", "things", "load.ts"),
        text: 'export const url = "/api/v1/private";',
      },
    ],
    /builds a gateway URL outside the operations table/,
  ],
  [
    "a template URL",
    [
      good,
      {
        path: join("src", "things", "stream.ts"),
        text: "export const url = (id: string) => `/api/v1/things/${id}/events`;",
      },
    ],
    /builds a gateway URL/,
  ],
  [
    "a concatenated URL",
    [
      good,
      {
        path: join("src", "things", "join.ts"),
        text: 'export const url = "/api" + "/v1/private";',
      },
    ],
    /builds a gateway URL/,
  ],
  [
    "a download href",
    [
      good,
      {
        path: join("src", "things", "Link.tsx"),
        text: 'export const Link = () => <a href="/api/v1/things/1/download">file</a>;',
      },
    ],
    /builds a gateway URL/,
  ],
  [
    "an image src",
    [
      good,
      {
        path: join("src", "things", "Image.tsx"),
        text: 'export const Image = ({ id }: { id: string }) => <img src={`/api/v1/things/${id}`} alt="" />;',
      },
    ],
    /builds a gateway URL/,
  ],
  [
    "fetch outside the client",
    [
      good,
      {
        path: join("src", "things", "raw.ts"),
        text: "export const get = (url: string) => fetch(url);",
      },
    ],
    /uses fetch outside src\/api/,
  ],
  [
    "an EventSource outside the client",
    [
      good,
      {
        path: join("src", "things", "live.ts"),
        text: "export const open = (url: string) => new EventSource(url);",
      },
    ],
    /uses EventSource outside src\/api/,
  ],
];

function selfTest() {
  let failed = 0;
  for (const [name, files, expected] of CONTROLS) {
    const { problems } = analyse(files, documented);
    if (!problems.some((problem) => expected.test(problem))) {
      console.error(
        `Self-test failed: ${name} was not refused (${problems.join("; ") || "no problems"})`,
      );
      failed++;
    }
  }
  const compliant = analyse(
    [
      table(`  thing: { method: "GET", path: "/api/v1/things/{id}" },`),
      settingsTable(
        `  create: { method: "POST", path: "/api/v1/things" },\n  later: { method: "GET", path: "/api/v1/later" },`,
      ),
      {
        path: join("src", "things", "use.ts"),
        text: 'import { pathFor } from "../api/operations";\nexport const url = (id: string) => pathFor("thing", { id });',
      },
    ],
    documented,
  );
  if (compliant.problems.length) {
    console.error(
      `Self-test failed: the compliant fixture was refused: ${compliant.problems.join("; ")}`,
    );
    failed++;
  }
  // Rule 5: a planned operation's marker must name a slice.
  for (const marker of ["", "soon", 5, true]) {
    const { problems } = analyse([good], {
      paths: {
        ...documented.paths,
        "/api/v1/later": { get: { "x-tqa-planned": marker } },
      },
    });
    if (!problems.some((problem) => /x-tqa-planned must name/.test(problem))) {
      console.error(
        `Self-test failed: the planned marker ${JSON.stringify(marker)} was not refused`,
      );
      failed++;
    }
  }
  const codes = codeProblems(
    [
      {
        path: "a.ts",
        text: 'if (error.code === "ghost") x();\nif (error.code === "known") y();\nif (error.code === "request_failed") z();',
      },
      {
        path: "b.ts",
        text: 'const REASONS: Readonly<Record<string, MessageKey>> = {\n  known: "a.b",\n  phantom: "a.c",\n};',
      },
    ],
    "a known code, and `known_other`",
  );
  if (
    codes.length !== 2 ||
    !codes[0]?.includes('"ghost"') ||
    !codes[1]?.includes('"phantom"')
  ) {
    console.error(
      `Self-test failed: undocumented codes were not refused as expected: ${codes.join("; ")}`,
    );
    failed++;
  }
  if (failed) process.exit(1);
  console.log(
    `API parity self-test passed: ${CONTROLS.length} refused and 1 compliant fixture (a planned route included); 4 malformed planned markers and 2 undocumented codes refused.`,
  );
}

selfTest();
if (!process.argv.includes("--self-test")) {
  const files = sources(ROOT).map((path) => ({
    path: relative(".", path),
    text: readFileSync(path, "utf8"),
  }));
  const document = readFileSync(OPENAPI, "utf8");
  const openapi = JSON.parse(document);
  const { problems, unused, operations } = analyse(files, openapi);
  problems.push(
    ...codeProblems(files, `${document}\n${readFileSync(PROTOCOL, "utf8")}`),
  );
  if (unused.length)
    console.log(
      `Documented operations the web does not use (${unused.length}): ${unused.join(", ")}`,
    );
  if (problems.length) {
    for (const problem of problems) console.error(problem);
    console.error(`API parity failed: ${problems.length} problem(s).`);
    process.exit(1);
  }
  console.log(
    `API parity passed: ${operations} operations, all documented; ${files.length} files send nothing outside the client.`,
  );
}
