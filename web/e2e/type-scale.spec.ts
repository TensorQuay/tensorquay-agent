import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { signIn } from "./session";
import { expect, resetFront, test, type Locator, type Page } from "./test";

/** A shot for the reviewers when TQA_WEB_EVIDENCE_DIR names a folder, once the page's finite motion has ended. */
async function shot(page: Page, name: string, subject?: Locator) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter(
            (motion) =>
              motion.effect?.getComputedTiming().iterations !== Infinity,
          )
          .map((motion) => motion.finished.catch(() => undefined)),
      ),
      new Promise((resolve) => setTimeout(resolve, 5_000)),
    ]),
  );
  const path = join(directory, `${name}.png`);
  if (subject) await subject.screenshot({ path });
  else await page.screenshot({ path });
}

/** What the browser draws for an element's text: its size and its line, in px. */
const text = (element: Locator) =>
  element.first().evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      size: parseFloat(style.fontSize),
      line: parseFloat(style.lineHeight),
    };
  });

const height = (element: Locator) =>
  element
    .first()
    .evaluate((node) => Math.round(node.getBoundingClientRect().height));

// WEB-U-451 (UI-GUIDELINES §4 and §5c; decided on 9 Nov 2026): the whole app one step bigger, through its
// tokens. The browser draws body text 16/24, small text 14 px, the navigation 15 px, page headings 22 px, a task's
// title 18 px and labels 13 px; list and menu rows are 36 px (44 px on touch screens). On the task list, a task page,
// Settings and the account menu, at 1280 and on a phone at 390, light and dark; each is captured for the reviewers.
for (const [width, colorScheme] of [
  [1280, "light"],
  [1280, "dark"],
  [390, "light"],
  [390, "dark"],
] as const)
  test(`WEB-U-451 the type scale at ${width} ${colorScheme}`, async ({
    browser,
  }) => {
    test.setTimeout(120_000);
    const phone = width === 390;
    const row = phone ? 44 : 36;
    const context = await browser.newContext({
      viewport: { width, height: phone ? 844 : 800 },
      deviceScaleFactor: 2,
      colorScheme,
      reducedMotion: "reduce",
      ...(phone ? { isMobile: true, hasTouch: true } : {}),
    });
    const page = await context.newPage();
    const name = (what: string) => `${what}-${width}-${colorScheme}`;
    await resetFront(page.request, ["agent-teams"]);
    await signIn(page);

    await test.step("a task page with a finished turn", async () => {
      await page.goto("/");
      await page
        .getByLabel("Message", { exact: true })
        .fill("Summarise the release notes in three lines");
      await page.getByRole("button", { name: "Send", exact: true }).click();
      await expect(page).toHaveURL(/\/c\/[^/]+$/);
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
        timeout: 30_000,
      });
      await shot(page, name("task"));
      const prose = await text(page.locator(".assistant-text p"));
      expect.soft(prose.size).toBe(16);
      expect.soft(prose.line).toBe(24);
      const title = await text(
        page
          .getByRole("region", { name: "Task header" })
          .getByRole("heading", { level: 1 }),
      );
      expect.soft(title.size).toBe(18);
    });

    await test.step("the task list", async () => {
      const list = phone
        ? page.getByRole("dialog", { name: "Navigation" })
        : page.locator(".app-sidebar");
      if (phone)
        await page.getByRole("button", { name: "Open navigation" }).click();
      await expect(list.locator(".conversation-row").first()).toBeVisible();
      await shot(page, name("list"));
      if (!phone) await shot(page, name("sidebar"), list);
      const link = list.locator(".conversation-link");
      expect.soft((await text(link)).size).toBe(15);
      expect.soft(await height(list.locator(".conversation-row"))).toBe(row);
      expect.soft((await text(list.locator(".sidebar-label"))).size).toBe(13);
    });

    await test.step("the account menu", async () => {
      const holder = phone
        ? page.getByRole("dialog", { name: "Navigation" })
        : page.locator(".app-sidebar");
      await holder
        .getByRole("button", { name: "Account", exact: true })
        .click();
      const menu = page.getByRole("menu", { name: "Account" });
      await expect(menu).toBeVisible();
      await shot(page, name("account-menu"));
      const item = menu.getByRole("menuitem").first();
      expect.soft((await text(item)).size).toBe(15);
      expect.soft(await height(item)).toBeGreaterThanOrEqual(row);
      await page.keyboard.press("Escape");
      if (phone) await page.keyboard.press("Escape");
    });

    await test.step("Settings", async () => {
      await page.goto("/settings/models");
      const heading = page.locator(".settings-title h1");
      await expect(heading).toBeVisible();
      await shot(page, name("settings"));
      expect.soft((await text(heading)).size).toBe(22);
      expect.soft((await text(heading)).line).toBe(30);
      const muted = page
        .locator(".settings-section-intro, .type-small")
        .first();
      expect.soft((await text(muted)).size).toBe(14);
    });
    await context.close();
  });

// WEB-U-451 at 200 % zoom (WCAG 1.4.4): a 1280 × 800 window zoomed to 200 % lays the app out at 640 × 400 CSS px at a
// device scale of 2. With the bigger type, no page scrolls sideways: a task page, its list in the drawer, Settings.
test("WEB-U-451 the type scale at 200 % zoom", async ({ browser }) => {
  test.setTimeout(120_000);
  const context = await browser.newContext({
    viewport: { width: 640, height: 400 },
    deviceScaleFactor: 2,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await resetFront(page.request, ["agent-teams"]);
  await signIn(page);
  const sideways = () =>
    page.evaluate(() => {
      const root = document.scrollingElement;
      return root ? root.scrollWidth - root.clientWidth : 0;
    });
  await page.goto("/");
  await page
    .getByLabel("Message", { exact: true })
    .fill("Summarise the release notes in three lines");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
  await shot(page, "zoom-200-task");
  expect(await sideways()).toBe(0);
  await page.getByRole("button", { name: "Open navigation" }).click();
  const drawer = page.getByRole("dialog", { name: "Navigation" });
  await expect(drawer.locator(".conversation-row").first()).toBeVisible();
  await shot(page, "zoom-200-list");
  expect(await sideways()).toBe(0);
  await page.keyboard.press("Escape");
  await page.goto("/settings/models");
  await expect(page.locator(".settings-title h1")).toBeVisible();
  await shot(page, "zoom-200-settings");
  expect(await sideways()).toBe(0);
  await context.close();
});
