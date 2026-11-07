import { writeFile } from "node:fs/promises";

import { expect } from "@playwright/test";

import { chooseEnvironment, confirmEnvironment } from "./environment";
import {
  activities,
  fixedShell,
  markdown,
  pauseAndResume,
  snapshot,
} from "./ux/assertions";
import { humanChrome } from "./ux/chrome";
import { test } from "./ux/evidence";
import {
  bootstrap,
  completed,
  newConversation,
  scripted,
  STACK_START_MS,
  send,
  signIn,
} from "./ux/flow";
import { followUp, followingBottom } from "./ux/follow-up";
import { observations, observe } from "./ux/observer";
import { finishedVisuals, liveVisual, visual } from "./ux/visuals";

// WEB-E2E-018 and WEB-E2E-019 share assertions; only the latter uses an on-demand private model route.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned UX demo stack");
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The UX driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});
const conversations: string[] = [];

for (const width of [1024, 1440, 1920])
  for (const theme of ["light", "dark"] as const) {
    test(`WEB-E2E-018 UX ${width} ${theme}`, async ({ page, log }) => {
      // Two tasks, an environment switch and a follow-up, each start allowed STACK_START_MS on a loaded host.
      test.setTimeout(480_000);
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme });
      // The reviewed baselines record the transcript at its full column width, so the files panel (open by default
      // from 1280 px, task 068b) starts closed here; the files suites cover the panel itself.
      await page.addInitScript(() => {
        localStorage.setItem("tensorquay-files-panel", "closed");
      });
      await observe(page);
      await signIn(page);
      if (width === 1440)
        await visual(page, page.locator("main"), `ux-start-${theme}`);
      const first = await send(page);
      log.conversations.push(first);
      await pauseAndResume(page);
      if (width === 1440) {
        if (scripted) {
          await expect(page.locator(".activity-line")).toHaveCount(12);
          await expect(
            page
              .locator(".activity-mark")
              .filter({ hasText: /Running|Prepared/ }),
          ).toHaveCount(0);
        }
        await liveVisual(page, theme);
      }
      await completed(page);
      await expect(page.getByText(/^Switched to /)).toHaveCount(0);
      await humanChrome(page);
      log.samples.push(await observations(page));
      await markdown(page);
      await fixedShell(page);
      if (width === 1440) await finishedVisuals(page, theme);
      await activities(page, scripted);
      const before = await snapshot(page);
      await page.reload();
      await completed(page);
      await humanChrome(page);
      await expect
        .poll(() => snapshot(page), {
          message: "replay: preserve the complete normalized transcript",
        })
        .toBe(before);
      await chooseEnvironment(page, "Minimal");
      await confirmEnvironment(page, "Minimal");
      await expect(
        page.getByText("Switched to Minimal", { exact: true }),
      ).toHaveCount(0);
      await followUp(
        page,
        "Continue with the retained workspace in the new environment.",
      );
      // The notice comes once the new environment is up, which is a start on a loaded host: the same allowance as the
      // run's own start below (the 13g integration at 1440 light sat at Starting… for 30 s).
      await expect(
        page.getByText("Switched to Minimal", { exact: true }),
      ).toHaveCount(1, { timeout: scripted ? STACK_START_MS : 180_000 });
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
        timeout: scripted ? STACK_START_MS : 180_000,
      });
      await followingBottom(page);
      log.samples.push(await observations(page));
      await page.reload();
      await expect(
        page.getByText("Switched to Minimal", { exact: true }),
      ).toHaveCount(1);
      const second = await newConversation(page, first);
      log.conversations.push(second);
      await pauseAndResume(page);
      await completed(page);
      log.samples.push(await observations(page));
      await markdown(page);
      await activities(page, scripted);
      expect
        .soft(log.consoleFailures, "console: no application warnings or errors")
        .toEqual([]);
      conversations.push(...log.conversations);
      if (process.env.TQA_STACK_RESULT)
        await writeFile(
          process.env.TQA_STACK_RESULT,
          JSON.stringify({ conversations }),
          { mode: 0o600 },
        );
    });
  }
