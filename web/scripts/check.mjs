import { spawnSync } from "node:child_process";

const steps = [
  "protocol:check",
  "api:check",
  "format:check",
  "lint",
  "typecheck",
  // Every browser spec loads, so a full Playwright run can't fail before its first test unnoticed.
  "e2e:list",
  "tokens",
  "copy",
  "i18n:check",
  // The guides in docs/ quote current English copy.
  "guides",
  // The unit tests, with the coverage floors of vitest.config.ts (ENGINEERING-GUARDRAILS §2.2).
  "test:coverage",
  "licenses",
  // TEST: SEC-008
  // Known vulnerabilities in the lockfile's packages, failing on high or critical (SEC-008). The image's own
  // packages, Node's bundled npm included, are checked by the release gate (REL-U-003).
  "audit",
  "build",
  // Each route's first-load JavaScript within its budget, read from the build (WEB-U-069).
  "budgets",
  // What the web server can send is build output: the connector stack test relies on it (WEB-E2E-010).
  "boundary",
  "test:production",
];
const corepack = process.platform === "win32" ? "corepack.cmd" : "corepack";

for (const step of steps) {
  console.log(`[check] ${step}`);
  const result = spawnSync(corepack, ["pnpm", "run", step], {
    stdio: "inherit",
    // The unit tests' waits scale with the harness's load scale, 4 in the gates as in tools/check.sh (WEB-U-302).
    env:
      step === "test:coverage"
        ? {
            ...process.env,
            TQA_TEST_DEADLINE_SCALE: process.env.TQA_TEST_DEADLINE_SCALE ?? "4",
          }
        : process.env,
  });
  if (result.error || result.status !== 0) {
    console.error(`[check] ${step} failed`);
    if (result.error) console.error(result.error.message);
    process.exit(result.status ?? 1);
  }
}

console.log("[check] All web gates passed.");
