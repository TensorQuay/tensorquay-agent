// UI-GUIDELINES §11b: every user-visible string comes from a catalogue. This check fails on hard-coded copy in the
// application source: visible JSX text, user-facing attribute values and sentence-like string literals.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import ts from "typescript";

// Attributes whose string values reach the reader or assistive technology.
const COPY_ATTRIBUTES = new Set([
  "aria-label",
  "aria-description",
  "aria-valuetext",
  "title",
  "placeholder",
  "alt",
  "label",
  "message",
]);
// Attributes and properties that hold identifiers, styles or routes, never copy.
const CODE_ATTRIBUTES = new Set([
  "className",
  "id",
  "key",
  "href",
  "role",
  "type",
  "variant",
  "name",
  "autoComplete",
  "inputMode",
  "rel",
  "target",
  "method",
  "lang",
]);
// A literal reads as copy when it starts with a capitalised word followed by lowercase letters or contains a sentence.
const SENTENCE =
  /^[\p{Lu}][\p{Ll}]+(?:[\s,.:;!?…]|$)|[\p{L}]{2,}[\s][\p{L}]{2,}[.?!…]$/u;
// Words beside a space in a template (" lines ", "created ", "Worked for ") are prose, not identifiers.
const WORDS = /\p{L}{2,}\s|\s\p{L}{2,}/u;
const ALLOW = "copy-lint: not user copy";
// Source that never reaches a product page: tests, generated code, test helpers and the development-only reference.
const SKIPPED = [
  /\.test\.tsx?$/,
  /\/generated\//,
  /^src\/test\//,
  /^src\/design\//,
  /^src\/app\/\(app\)\/design\//,
];

function allowed(source, node) {
  const line = source.getLineAndCharacterOfPosition(node.getStart()).line;
  const text = source.text.split("\n");
  return [text[line], text[line - 1]].some((value) => value?.includes(ALLOW));
}

function attributeName(node) {
  let current = node.parent;
  while (current && ts.isJsxExpression(current)) current = current.parent;
  while (
    current &&
    (ts.isConditionalExpression(current) || ts.isBinaryExpression(current))
  )
    current = current.parent;
  if (current && ts.isJsxExpression(current)) current = current.parent;
  return current && ts.isJsxAttribute(current) ? current.name.getText() : null;
}

function propertyName(node) {
  const parent = node.parent;
  if (parent && ts.isPropertyAssignment(parent) && parent.initializer === node)
    return parent.name.getText().replace(/["']/g, "");
  return null;
}

export function violations(text, fileName = "component.tsx") {
  const source = ts.createSourceFile(
    fileName,
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const found = [];
  const report = (node, value) => {
    if (allowed(source, node)) return;
    const { line } = source.getLineAndCharacterOfPosition(node.getStart());
    found.push({ line: line + 1, value: value.trim().slice(0, 60) });
  };
  function visit(node) {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) return;
    if (ts.isJsxText(node) && /\p{L}/u.test(node.text)) report(node, node.text);
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      const attribute = attributeName(node);
      const property = propertyName(node);
      const parent = node.parent;
      const code =
        (attribute &&
          (CODE_ATTRIBUTES.has(attribute) || attribute.startsWith("data-"))) ||
        (property && CODE_ATTRIBUTES.has(property)) ||
        (parent && ts.isExpressionStatement(parent)) ||
        (parent && ts.isElementAccessExpression(parent)) ||
        (parent &&
          ts.isCallExpression(parent) &&
          /^(?:t|require|import|parseApi|parseFrame|validator|matchMedia)$/.test(
            parent.expression.getText(),
          )) ||
        (parent &&
          ts.isBinaryExpression(parent) &&
          [
            ts.SyntaxKind.EqualsEqualsEqualsToken,
            ts.SyntaxKind.ExclamationEqualsEqualsToken,
          ].includes(parent.operatorToken.kind)) ||
        (parent && ts.isCaseClause(parent)) ||
        (parent && ts.isLiteralTypeNode(parent));
      if (
        !code &&
        /\p{L}/u.test(node.text) &&
        ((attribute && COPY_ATTRIBUTES.has(attribute)) ||
          SENTENCE.test(node.text))
      )
        report(node, node.text);
    }
    if (ts.isTemplateExpression(node)) {
      const spans = [
        node.head.text,
        ...node.templateSpans.map((span) => span.literal.text),
      ];
      const attribute = attributeName(node);
      const code =
        attribute &&
        (CODE_ATTRIBUTES.has(attribute) || attribute.startsWith("data-"));
      if (
        !code &&
        !allowed(source, node) &&
        spans.some((text) => SENTENCE.test(text) || WORDS.test(text))
      )
        report(node, spans.join("…"));
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  return found;
}

function scan(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return scan(path);
    if (
      !/\.tsx?$/.test(entry.name) ||
      SKIPPED.some((pattern) => pattern.test(path))
    )
      return [];
    return violations(readFileSync(path, "utf8"), path).map(
      ({ line, value }) => `${path}:${line}: hard-coded copy "${value}"`,
    );
  });
}

function selfTest() {
  const compliant = [
    'export const A = () => <p className="type-small muted">{t("sidebar.tasks")}</p>;',
    'const b = <Button variant="ghost" aria-label={t("task.rename")} data-state="Open" />;',
    'const c = { role: "Menu", type: "Button" }; throw new Error("x"); // copy-lint: not user copy',
    'if (event.key === "Escape") close(); const v = parseApi(value, "Conversation");',
    'switch (kind) { case "Created": break; } type Label = "Sending…";',
    '"use client";',
    'import { Button } from "../ui/Button";',
  ];
  const forbidden = [
    "const a = <p>Tasks</p>;",
    'const b = <Button aria-label="Rename task" />;',
    'const c = <input placeholder="Reply…" />;',
    'const d = sending ? "Sending…" : label;',
    'const e = <p title={busy ? "Please wait" : name} />;',
    "const f = `Expires in ${hours} h`;",
    "const g = `created ${count} files`;",
    "const h = `${label} choices`;",
    'return { label: "Waiting for your approval", tone: "warn" };',
  ];
  for (const text of compliant) assert.deepEqual(violations(text), [], text);
  for (const text of forbidden) assert.ok(violations(text).length > 0, text);
  console.log(
    `Copy self-test passed: ${compliant.length} compliant and ${forbidden.length} forbidden fixtures.`,
  );
}

if (process.argv.includes("--self-test")) selfTest();
else {
  const failures = scan("src");
  for (const failure of failures) console.error(failure);
  if (failures.length) {
    console.error(
      `${failures.length} hard-coded strings; move them to src/i18n/locales/en.json.`,
    );
    process.exitCode = 1;
  } else
    console.log(
      "Copy check passed: user-visible text comes from the catalogues.",
    );
}
