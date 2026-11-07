import { defineConfig } from "vitest/config";

// With its extension: Vite's native config loader, planned as its default, doesn't resolve one left out.
import { parseScale, parseWorkers, unitCaps } from "./deadline-scale.ts";

/**
 * Coverage floors (ENGINEERING-GUARDRAILS §2.2). Each store, reducer and utility (each non-component module) keeps at
 * least 80 % of its own statements: scripts/check-module-coverage.mjs checks that per file after this run, because
 * Vitest's glob thresholds are aggregate only. Components are held to behaviour tests instead. The whole-source floors are the
 * measured values after the gaps and the component behaviour tests on 2026-09-26 (92.53 / 87.56 / 89.35 / 93.47), rounded down, so
 * coverage cannot regress.
 */
const FLOOR = { statements: 92, branches: 87, functions: 89, lines: 93 };

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
    // Keep concurrent DOM suites within a predictable memory and CPU budget: 2, or fewer on a loaded host
    // (TQA_VITEST_MAX_WORKERS, 1 to 8; WEB-U-313).
    maxWorkers: parseWorkers(process.env.TQA_VITEST_MAX_WORKERS),
    // Every test runs in well under 1.2 s under coverage (7 Oct 2026, 2406 tests). A host stalled by other jobs once ran
    // one 45 times slower, past the 5 s default (WEB-U-074, 10 s), so the cap scales with TQA_TEST_DEADLINE_SCALE, which
    // the check sets to 4 (20 s): a stall passes, and a plain run still fails a test seconds late.
    testTimeout: unitCaps(parseScale(process.env.TQA_TEST_DEADLINE_SCALE))
      .testMs,
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      // Tests, generated code, test helpers and the development-only reference are not product code.
      exclude: [
        "src/**/*.test.{ts,tsx}",
        "src/**/generated/**",
        "src/test/**",
        "src/design/**",
        "src/app/(app)/design/**",
      ],
      reporter: ["text-summary", "json-summary"],
      thresholds: FLOOR,
    },
  },
});
