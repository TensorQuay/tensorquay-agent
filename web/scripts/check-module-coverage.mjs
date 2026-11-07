// ENGINEERING-GUARDRAILS §2.2: every store, reducer and utility keeps at least 80 % of its statements. Vitest's glob
// thresholds are aggregate only, so this check reads the coverage summary `test:coverage` writes and holds each
// non-component module (a .ts file of the product source) to the floor on its own. Components (.tsx) are held to
// behaviour tests instead. Run after `vitest run --coverage`:
//   node scripts/check-module-coverage.mjs              check coverage/coverage-summary.json
//   node scripts/check-module-coverage.mjs --self-test  check the rule on fixtures
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { relative, resolve } from "node:path";

/** The guardrail's floor for one module's statements, in percent (ENGINEERING-GUARDRAILS §2.2). */
export const MODULE_STATEMENTS = 80;
const SUMMARY = resolve("coverage", "coverage-summary.json");

/**
 * The modules below the floor, as "path pct%".
 * @param {Record<string, {statements: {pct: number}}>} summary vitest's json-summary (absolute paths, plus "total")
 * @param {string} root the web directory, for readable paths
 */
export function belowFloor(summary, root) {
  return Object.entries(summary)
    .filter(([path]) => path !== "total" && path.endsWith(".ts"))
    .filter(([, figures]) => figures.statements.pct < MODULE_STATEMENTS)
    .map(
      ([path, figures]) => `${relative(root, path)} ${figures.statements.pct}%`,
    )
    .sort();
}

function selfTest() {
  const root = "/web";
  const file = (pct) => ({ statements: { pct } });
  const summary = {
    total: file(50),
    "/web/src/api/transport.ts": file(79.99),
    "/web/src/api/client.ts": file(80),
    "/web/src/ui/Button.tsx": file(10),
  };
  assert.deepEqual(belowFloor(summary, root), ["src/api/transport.ts 79.99%"]);
  assert.deepEqual(belowFloor({ total: file(10) }, root), []);
  console.log(
    "Module coverage self-test passed: the floor, the boundary, components and the total.",
  );
}

if (process.argv.includes("--self-test")) selfTest();
else {
  const low = belowFloor(
    JSON.parse(readFileSync(SUMMARY, "utf8")),
    process.cwd(),
  );
  if (low.length) {
    console.error(
      `Modules below ${MODULE_STATEMENTS} % statements (ENGINEERING-GUARDRAILS §2.2):\n  ${low.join("\n  ")}`,
    );
    process.exitCode = 1;
  } else
    console.log(
      `Every module covers at least ${MODULE_STATEMENTS} % of its statements.`,
    );
}
