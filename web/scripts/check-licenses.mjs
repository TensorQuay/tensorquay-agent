import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";

const policy = JSON.parse(
  readFileSync(new URL("../licenses.json", import.meta.url), "utf8"),
);
const allowedLicenses = new Set(policy.allow);

function matchesGlob(value, pattern) {
  const source = pattern
    .replace(/[.+^${}()|[\]\\]/g, "\\$&")
    .replace(/\*/g, ".*")
    .replace(/\?/g, ".");
  return new RegExp(`^${source}$`, "i").test(value);
}

function requiresNotice(expression) {
  const identifiers = expression.match(/[A-Za-z0-9][A-Za-z0-9.-]*/g) ?? [];
  const noticeLicenses = ["MPL-2.0", "CC-BY-4.0"];
  return noticeLicenses.some((license) => identifiers.includes(license));
}

function licenseOf(manifest) {
  const declaration = manifest.license ?? manifest.licenses;
  const typeOf = (value) =>
    typeof value === "string" ? value : (value?.type ?? "<missing>");
  if (Array.isArray(declaration)) {
    return declaration.length
      ? declaration.map((value) => `(${typeOf(value)})`).join(" OR ")
      : "<missing>";
  }
  return typeOf(declaration);
}

function isAllowedLicense(expression) {
  if (typeof expression !== "string") return false;
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  if (
    policy.forbidden.some((pattern) =>
      [...tokens, expression].some((value) => matchesGlob(value, pattern)),
    )
  )
    return false;
  let index = 0;

  function atom() {
    const token = tokens[index++];
    if (token === "(") {
      const value = or();
      if (tokens[index++] !== ")") throw new Error("Unclosed expression");
      return value;
    }
    if (!token || [")", "AND", "OR", "WITH"].includes(token)) {
      throw new Error("Expected a license identifier");
    }
    if (tokens[index] === "WITH") {
      index++;
      const exception = tokens[index++];
      if (!exception || ["(", ")", "AND", "OR", "WITH"].includes(exception)) {
        throw new Error("Expected an exception identifier");
      }
      return allowedLicenses.has(`${token} WITH ${exception}`);
    }
    return allowedLicenses.has(token);
  }

  function and() {
    let value = atom();
    while (tokens[index] === "AND") {
      index++;
      const right = atom();
      value = value && right;
    }
    return value;
  }

  function or() {
    let value = and();
    while (tokens[index] === "OR") {
      index++;
      const right = and();
      value = value || right;
    }
    return value;
  }

  try {
    const value = or();
    return index === tokens.length && value;
  } catch {
    return false;
  }
}

function scanPackages(root) {
  const seen = new Set();
  const issues = [];
  const notices = [];
  let count = 0;

  function inspect(directory) {
    const real = realpathSync(directory);
    if (seen.has(real)) return;
    seen.add(real);
    count++;
    const location = relative(root, directory);
    try {
      const manifest = JSON.parse(
        readFileSync(join(directory, "package.json"), "utf8"),
      );
      const license = licenseOf(manifest);
      const entry = {
        name: manifest.name ?? location,
        version: manifest.version ?? "unknown",
        license,
        location,
      };
      if (!isAllowedLicense(license)) {
        issues.push(entry);
      } else if (requiresNotice(license)) {
        notices.push(entry);
      }
    } catch {
      issues.push({
        name: location,
        version: "unknown",
        license: "<missing or invalid package.json>",
        location,
      });
    }
    const nested = join(directory, "node_modules");
    if (existsSync(nested)) walk(nested);
  }

  function walk(directory) {
    const store = join(directory, ".pnpm");
    if (existsSync(store)) {
      for (const packageDirectory of readdirSync(store)) {
        const nested = join(store, packageDirectory, "node_modules");
        if (existsSync(nested)) walk(nested);
      }
    }
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      if (!entry.isDirectory() && !entry.isSymbolicLink()) continue;
      const path = join(directory, entry.name);
      if (entry.name.startsWith("@")) walk(path);
      else inspect(path);
    }
  }

  walk(root);
  return { count, issues, notices };
}

/**
 * Vendored assets (licenses.json `assets`): each under an allowed licence, each file present and exactly the pinned
 * bytes, so a changed or swapped file can't pass as the one whose licence was checked. Paths are relative to `root`.
 */
export function checkAssets(assets, root) {
  const issues = [];
  for (const { name, version, license, files } of assets) {
    const label = `${name} ${version}`;
    for (const [path, pin] of Object.entries(files)) {
      const file = join(root, path);
      if (!existsSync(file)) issues.push(`${label}: ${path} is missing`);
      else if (
        createHash("sha256").update(readFileSync(file)).digest("hex") !== pin
      )
        issues.push(`${label}: ${path} doesn't match its pinned sha256`);
    }
    if (!isAllowedLicense(license))
      issues.push(`${label}: ${license} is not an allowed licence`);
  }
  return issues;
}

function selfTest() {
  const fixtures = [
    [{ name: "allowed", license: "MIT" }, true],
    [{ name: "disallowed", license: "GPL-3.0-only" }, false],
    [{ name: "missing" }, false],
    [{ license: "(MIT OR LicenseRef-Unlisted)" }, true],
    [{ license: "MIT OR GPL-3.0-only" }, false],
    [{ license: "MIT AND Apache-2.0" }, true],
    [{ license: "MIT AND GPL-3.0-only" }, false],
    [{ license: "MIT AND (Apache-2.0 OR GPL-3.0-only)" }, false],
    [{ license: "MIT OR" }, false],
    [{ license: "MIT OR (GPL-3.0-only" }, false],
    [{ license: "MIT WITH unknown-exception" }, false],
    [{ license: "MIT OR (GPL-2.0-only WITH Classpath-exception-2.0)" }, false],
    [{ licenses: [{ type: "GPL-3.0-only" }, { type: "ISC" }] }, false],
    [{ licenses: ["BSD-3-Clause"] }, true],
    [{ licenses: [] }, false],
    [{ license: "MPL-2.0 AND MIT" }, true],
    [{ license: "Apache-2.0 WITH LLVM-exception" }, true],
    [{ name: "@img/sharp-libvips-test", license: "LGPL-3.0-or-later" }, false],
    [
      { name: "@img/sharp-wasm32", license: "MIT AND LGPL-3.0-or-later" },
      false,
    ],
    [{ name: "unrelated-package", license: "LGPL-3.0-or-later" }, false],
    [{ name: "@img/sharp-wasm32", license: "GPL-3.0-only" }, false],
    [{ license: "MIT OR CC-BY-NC-4.0" }, false],
    [{ license: "UNLICENSED OR MIT" }, false],
    [{ license: "SEE LICENSE IN COPYING" }, false],
  ];

  for (const [manifest, allowed] of fixtures) {
    assert.equal(isAllowedLicense(licenseOf(manifest)), allowed);
  }
  assert.equal(requiresNotice("MPL-2.0"), true);
  assert.equal(requiresNotice("CC-BY-4.0"), true);
  assert.equal(requiresNotice("LGPL-3.0-or-later"), false);
  assert.equal(requiresNotice("MIT"), false);
  testInstalledPackages();
  testAssets();
  console.log(
    `License self-test passed: ${fixtures.length} declarations, 6 installed-package fixtures and 4 asset fixtures.`,
  );
}

function testAssets() {
  const root = mkdtempSync(join(tmpdir(), "web-asset-test-"));
  try {
    writeFileSync(join(root, "font.ttf"), "font bytes");
    const pin = createHash("sha256").update("font bytes").digest("hex");
    const asset = (license, files) => ({
      name: "Fixture",
      version: "1",
      license,
      source: "https://example.invalid/fixture.tar",
      files,
    });
    assert.deepEqual(
      checkAssets([asset("Apache-2.0", { "font.ttf": pin })], root),
      [],
    );
    assert.deepEqual(
      checkAssets(
        [
          asset("Apache-2.0", { "font.ttf": "0".repeat(64) }),
          asset("GPL-2.0-only", { "font.ttf": pin }),
          asset("Apache-2.0", { "gone.ttf": pin }),
        ],
        root,
      ),
      [
        "Fixture 1: font.ttf doesn't match its pinned sha256",
        "Fixture 1: GPL-2.0-only is not an allowed licence",
        "Fixture 1: gone.ttf is missing",
      ],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function testInstalledPackages() {
  const root = mkdtempSync(join(tmpdir(), "web-license-test-"));
  try {
    const packages = {
      allowed: { name: "allowed", license: "MIT" },
      disallowed: { name: "disallowed", license: "GPL-3.0-only" },
      missing: { name: "missing" },
      "@fixture/scoped": { name: "scoped", license: "ISC" },
      "allowed/node_modules/nested": {
        name: "nested",
        license: "GPL-3.0-only",
      },
      ".pnpm/virtual@1/node_modules/virtual": {
        name: "virtual",
        license: "LGPL-3.0-only",
      },
    };
    for (const [directory, manifest] of Object.entries(packages)) {
      const path = join(root, directory);
      mkdirSync(path, { recursive: true });
      writeFileSync(join(path, "package.json"), JSON.stringify(manifest));
    }
    const result = scanPackages(root);
    assert.equal(result.count, 6);
    assert.deepEqual(result.issues.map((issue) => issue.name).sort(), [
      "disallowed",
      "missing",
      "nested",
      "virtual",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

if (process.argv.includes("--self-test")) {
  selfTest();
} else {
  const { count, issues, notices } = scanPackages(resolve("node_modules"));
  for (const issue of checkAssets(policy.assets ?? [], resolve("."))) {
    console.error(issue);
    process.exitCode = 1;
  }
  for (const issue of issues.sort((a, b) =>
    a.location.localeCompare(b.location),
  )) {
    console.error(
      `${issue.name}@${issue.version}: ${issue.license} (${issue.location})`,
    );
  }
  if (issues.length) {
    console.error(
      `License check failed: ${issues.length} of ${count} packages.`,
    );
    process.exitCode = 1;
  } else if (process.argv.includes("--notices")) {
    const cell = (value) =>
      String(value).replace(/\|/g, "\\|").replace(/\s+/g, " ");
    console.log("| Package | Version | License |");
    console.log("| --- | --- | --- |");
    for (const entry of notices.sort((a, b) => a.name.localeCompare(b.name))) {
      console.log(
        `| ${cell(entry.name)} | ${cell(entry.version)} | ${cell(entry.license)} |`,
      );
    }
  } else {
    console.log(`License check passed: ${count} packages.`);
  }
}
