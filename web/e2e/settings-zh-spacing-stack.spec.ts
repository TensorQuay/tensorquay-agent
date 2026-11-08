import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { signedIn } from "./ux/flow";

// WEB-E2E-064 (backs up WEB-U-305 and WEB-U-185): Simplified Chinese spaces its numbers on the real stack. With the
// language set to 简体中文 from the Account menu, Settings › Limits shows its durations as "N 秒", "N 分钟" or "N 小时",
// never attached, and a real task's summary attaches no time to its number either. The language lives in this
// browser's storage, so the next spec's fresh browser starts in English whatever happens here.

test.skip(
  process.env.TQA_STACK_SUITE !== "settings",
  "Requires the settings stack suite",
);
test.describe.configure({ mode: "serial" });

/** A number attached to a time unit: what the zh rule (UI-GUIDELINES §11b) forbids. */
const ATTACHED = /\d(秒|分钟|小时|毫秒)/;
/** A number spaced from its unit, as the rule asks. */
const SPACED = /\d (秒|分钟|小时)/;

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await signedIn(page);
  } finally {
    await page.close();
  }
});

async function chooseLanguage(page: Page, current: string, name: string) {
  await page.getByRole("button", { name: /^(Account|账户)$/ }).click();
  await page
    .getByRole("menuitem", { name: new RegExp(`· ${current}$`) })
    .click();
  await page.getByRole("menuitemradio", { name }).click();
  await page.keyboard.press("Escape");
}

test("WEB-E2E-064 Simplified Chinese spaces numbers on Limits and in a task", async ({
  page,
}) => {
  // One task's start, plus the pages and the run; grows with the harness's load scale as the other stack specs do.
  test.setTimeout(budget(1, 150_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await signedIn(page);
  await page.goto("/");
  await chooseLanguage(page, "English", "简体中文");
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");

  // Settings › Limits: the gateway's real limits, their durations spaced from their numbers.
  await page.goto("/settings/limits");
  const main = page.getByRole("main");
  await expect(main).toContainText(SPACED);
  await expect(main).not.toContainText(ATTACHED);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

  // A real task on the stack's scripted model: its status and its steps' summary attach no time to a number.
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "消息" });
  await input.fill("Summarize the workspace.");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[^/]+$/);
  const status = page.getByLabel("运行状态");
  // The run finishes (完成): its summary is then always there, so the spacing check below can't vanish.
  await expect(status).toHaveText(/^完成/, {
    timeout: afterStart(90_000),
  });
  await expect(status).not.toContainText(ATTACHED);
  const summary = page.getByRole("button", { name: /工作了/ }).first();
  await expect(summary).toBeVisible();
  await expect(summary).not.toContainText(ATTACHED);
  await summary.click();
  await expect(page.getByRole("main")).not.toContainText(ATTACHED);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

  // The choice holds over a reload, and English comes back on request.
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");
  await chooseLanguage(page, "简体中文", "English");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
});
