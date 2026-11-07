import { defineConfig } from "@playwright/test";

const baseURL = "http://localhost:3100";

export default defineConfig({
  testDir: "./e2e",
  // The fake front's responses keep the gateway's contract; a drift fails the run (e2e/front/contract.mjs).
  globalTeardown: "./e2e/contract-teardown.ts",
  // e2e:list (pnpm check) loads every spec, the stack ones too, so a spec that can't load fails the check.
  testIgnore: process.env.TQA_E2E_LIST_ALL
    ? []
    : [
        "full-stack.spec.ts",
        "ux-invariants.spec.ts",
        "environment-stack.spec.ts",
        "approval-stack.spec.ts",
        "queue-stack.spec.ts",
        "egress-stack.spec.ts",
        "admin-stack.spec.ts",
        "connector-stack.spec.ts",
        // Captures the UX review's screenshots; it does not gate (UI-GUIDELINES §17).
        "ux-review-stack.spec.ts",
        "profile-stack.spec.ts",
        "channel-stack.spec.ts",
        "channel-members-stack.spec.ts",
        "host-keys-stack.spec.ts",
        // ADR-0039 on the real stack, run by crates/tqa-sandbox/tests/egress/desktop_live.py.
        "screen-stack.spec.ts",
      ],
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: 1,
  reporter: "list",
  snapshotPathTemplate: "{testDir}/__screenshots__/{platform}/{arg}{ext}",
  use: {
    baseURL,
    viewport: { width: 1440, height: 1000 },
    reducedMotion: "reduce",
    // A failed test keeps its trace and a screenshot (in test-results/, git-ignored), so a flake that only happens
    // under load shows what the page did (WEB-E2E-018). The fake front's credentials are synthetic; nothing real is
    // recorded.
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
  webServer: [
    {
      command:
        "TQA_WEB_TEST_BUILD=1 corepack pnpm run build && HOSTNAME=localhost PORT=3101 corepack pnpm run start",
      url: "http://localhost:3101/login",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command: "node e2e/front/server.mjs",
      url: `${baseURL}/healthz`,
      reuseExistingServer: false,
      timeout: 30_000,
    },
  ],
});
