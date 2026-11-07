import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { THINK } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-123 and WEB-U-122 on the page (candidate12 F13, F5): a model thinking at length shows it is at work. The live
// line counts on, and opens what it is thinking; the header's "Working… · N s" counts on too.
async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== "running"),
  );
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

/** The seconds a live count shows ("… · 12 s"), as the reader sees it. */
async function seconds(locator: Locator) {
  const text = await locator.innerText();
  const found = /· (\d+) s/.exec(text);
  if (!found) throw new Error(`No count in "${text}"`);
  return Number(found[1]);
}

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-123 a long think counts on and opens its thinking ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      // The header at its full width: the Files panel closed.
      const files = page
        .getByRole("region", { name: "Task header" })
        .getByRole("button", { name: "Files" });
      if ((await files.getAttribute("aria-expanded")) === "true")
        await files.click();
      const input = page.getByLabel("Message", { exact: true });
      await input.fill(THINK);
      await input.press("Enter");
      const group = page.getByRole("group", { name: "Activity" });
      const live = group.getByRole("button", { name: /^Thinking… · \d+ s$/ });
      await expect(live).toBeVisible();
      const status = page.getByLabel("Run status");
      await expect(status).toHaveText(/^Working…/);
      const first = await seconds(live);
      await page.waitForTimeout(3000);
      expect(await seconds(live)).toBeGreaterThanOrEqual(first + 2);
      // The header counts on too, where it has room for the time.
      // The header's status is one word; its count is in its tooltip (WEB-U-155).
      if (width >= 768) {
        const title = (await status.getAttribute("title")) ?? "";
        const counted = /· (\d+) s/.exec(title);
        if (!counted) throw new Error(`No count in "${title}"`);
        expect(Number(counted[1])).toBeGreaterThanOrEqual(3);
      }

      // The line opens what the model is thinking, once.
      const thought =
        "The totals come from three sheets; I'll compare them first.";
      await expect(page.getByText(thought)).toBeHidden();
      await live.click();
      await expect(live).toHaveAttribute("aria-expanded", "true");
      await expect(page.getByText(thought)).toBeVisible();
      await expect(page.getByText(thought)).toHaveCount(1);
      await capture(page, `thinking-live-${width}-${theme}`);
    });
