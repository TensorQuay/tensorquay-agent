import { dirname, join } from "node:path";

import { defineConfig } from "@playwright/test";

import { budget } from "./e2e/stack-start";

/** The browser a stack suite runs in: Chromium by default, WebKit for Safari (the driver refuses anything else too). */
function stackBrowser() {
  const browser = process.env.TQA_STACK_BROWSER ?? "chromium";
  if (browser !== "chromium" && browser !== "webkit")
    throw new Error("TQA_STACK_BROWSER must be chromium or webkit");
  return browser;
}

/** WEB-E2E-004 owns no page server; the driver supplies a real gateway stack. */
export default defineConfig({
  testDir: "./e2e",
  testMatch:
    process.env.TQA_STACK_SUITE === "desktop"
      ? // WEB-E2E-071..077: the Desktop environment on the desktop suite's stack.
        "desktop-stack.spec.ts"
      : process.env.TQA_STACK_SUITE === "agents"
        ? // One worker runs files in name order: the edge cases (026..030), an ended team (WEB-E2E-062), then 021..025,
          // and 020 last.
          [
            "agents-edges-stack.spec.ts",
            "agents-ended-stack.spec.ts",
            "agents-lead-unread-stack.spec.ts",
            "agents-stack.spec.ts",
          ]
        : process.env.TQA_STACK_SUITE === "ux"
          ? "ux-invariants.spec.ts"
          : process.env.TQA_STACK_SUITE === "environment"
            ? // E2E-ENV-010 and E2E-ENV-012, and E2E-PROFILE-018 on the same scripted model.
              ["environment-stack.spec.ts", "profile-tools-stack.spec.ts"]
            : process.env.TQA_STACK_SUITE === "approvals"
              ? "approval-stack.spec.ts"
              : process.env.TQA_STACK_SUITE === "host-keys"
                ? // WEB-E2E-035, run by crates/tqa-sandbox/tests/egress/web_host_keys.py on the host-keys stack.
                  "host-keys-stack.spec.ts"
                : process.env.TQA_STACK_SUITE === "queue"
                  ? // WEB-E2E-018, and T22's Keep environment on with the same one slot (the c15 stack gaps).
                    [
                      "queue-stack.spec.ts",
                      "queue-keep-environment-stack.spec.ts",
                    ]
                  : process.env.TQA_STACK_SUITE === "egress"
                    ? "egress-stack.spec.ts"
                    : process.env.TQA_STACK_SUITE === "accounts"
                      ? "accounts-stack.spec.ts"
                      : process.env.TQA_STACK_SUITE === "files"
                        ? // The suite's own, then the c15 feature matrix's gap in the Files panel.
                          ["files-stack.spec.ts", "files-c15-stack.spec.ts"]
                        : process.env.TQA_STACK_SUITE === "workspaces"
                          ? "workspaces-stack.spec.ts"
                          : process.env.TQA_STACK_SUITE === "admin"
                            ? "admin-stack.spec.ts"
                            : process.env.TQA_STACK_SUITE === "connector"
                              ? "connector-stack.spec.ts"
                              : process.env.TQA_STACK_SUITE === "profile"
                                ? "profile-stack.spec.ts"
                                : process.env.TQA_STACK_SUITE === "ux-review"
                                  ? "ux-review-stack.spec.ts"
                                  : process.env.TQA_STACK_SUITE === "channels"
                                    ? "channel-stack.spec.ts"
                                    : process.env.TQA_STACK_SUITE ===
                                        "channel-members"
                                      ? "channel-members-stack.spec.ts"
                                      : process.env.TQA_STACK_SUITE ===
                                          "models-people"
                                        ? "models-people-stack.spec.ts"
                                        : process.env.TQA_STACK_SUITE ===
                                            "settings"
                                          ? // Settings: its pages, the c15 gaps (WEB-E2E-057…061), WEB-E2E-064, 067 and 068…070.
                                            [
                                              "settings-c15-stack.spec.ts",
                                              "settings-engines-stack.spec.ts",
                                              "settings-models-checks-stack.spec.ts",
                                              "settings-stack.spec.ts",
                                              "settings-limits-desktop-stack.spec.ts",
                                              "settings-zh-spacing-stack.spec.ts",
                                              "settings-models-server-stack.spec.ts",
                                              "settings-engines-answer-stack.spec.ts",
                                            ]
                                          : process.env.TQA_STACK_SUITE ===
                                              "task-page"
                                            ? // The task page's G6 cases, and the c15 stack gaps: WEB-E2E-050…055 and -042.
                                              [
                                                "task-page-stack.spec.ts",
                                                "task-page-c15-stack.spec.ts",
                                                "task-page-recovery-stack.spec.ts",
                                                "task-page-repeat-stack.spec.ts",
                                                "task-page-retries-stack.spec.ts",
                                              ]
                                            : process.env.TQA_STACK_SUITE ===
                                                "reply-cut"
                                              ? "reply-cut-stack.spec.ts"
                                              : process.env.TQA_STACK_SUITE ===
                                                  "key-use"
                                                ? "key-use-stack.spec.ts"
                                                : process.env
                                                      .TQA_STACK_SUITE ===
                                                    "key-use-connector"
                                                  ? "key-use-connector-stack.spec.ts"
                                                  : process.env
                                                        .TQA_STACK_SUITE ===
                                                      "key-use-push"
                                                    ? "key-use-push-stack.spec.ts"
                                                    : process.env
                                                          .TQA_STACK_SUITE ===
                                                        "previews"
                                                      ? "previews-stack.spec.ts"
                                                      : process.env
                                                            .TQA_STACK_SUITE ===
                                                          "titles"
                                                        ? "titles-stack.spec.ts"
                                                        : process.env
                                                              .TQA_STACK_SUITE ===
                                                            "screen"
                                                          ? "screen-stack.spec.ts"
                                                          : "full-stack.spec.ts",
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  // Baselines are never rewritten by a check run; TQA_STACK_UPDATE_SNAPSHOTS=1 rewrites the ones a suite reads, for a
  // release that changes the screens they show (each new baseline is reviewed before it is committed).
  updateSnapshots:
    process.env.TQA_STACK_UPDATE_SNAPSHOTS === "1" ? "all" : "none",
  workers: 1,
  // A test without its own budget still has room for one start (e2e/stack-start.ts).
  timeout: budget(1, 60_000),
  expect: { timeout: 30_000 },
  // Every test.step in the log, so a run's log shows each part of a suite it reached (WEB-E2E-004, -021).
  reporter: [["list", { printSteps: true }]],
  outputDir: process.env.TQA_WEB_EVIDENCE_DIR
    ? join(process.env.TQA_WEB_EVIDENCE_DIR, "playwright")
    : process.env.TQA_STACK_RESULT
      ? join(dirname(process.env.TQA_STACK_RESULT), "playwright")
      : "test-results/stack",
  snapshotPathTemplate: process.env.TQA_UX_BASELINES
    ? `${process.env.TQA_UX_BASELINES}/{arg}{ext}`
    : "{testDir}/__screenshots__/{platform}/{arg}{ext}",
  use: {
    baseURL: process.env.TQA_STACK_URL,
    viewport: { width: 1440, height: 1000 },
    reducedMotion: "reduce",
    browserName: stackBrowser(),
  },
});
