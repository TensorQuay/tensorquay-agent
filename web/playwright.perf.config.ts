import { defineConfig } from "@playwright/test";

import base from "./playwright.config";

// PERF-040 / WEB-E-009: the long-transcript render harness, on the same production build and fake front as the web
// e2e suite, run on its own (`pnpm run perf:transcript`, through the heavy-run wrapper) so the regular suite stays fast.
export default defineConfig({
  ...base,
  testDir: "./e2e/perf",
  testIgnore: [],
  testMatch: "**/*.perf.ts",
  timeout: 20 * 60_000,
});
