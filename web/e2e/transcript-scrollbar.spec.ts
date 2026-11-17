import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-132 (18.1 polish): a long transcript shows no scrollbar at rest and takes no width for one, so its column
// and the composer never move; scrolling shows a thin bar over its edge, which goes about a second after. At 1280 and
// 390, light and dark, with a desktop's classic scrollbars (Playwright hides them unless told not to).

test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({
    path: join(directory, `transcript-scrollbar-${name}.png`),
  });
}

/** What shows of a scrollbar on the transcript: the browser's own (its width) and the overlay's opacity. */
async function bars(page: Page) {
  return page.evaluate(() => {
    const region = document.querySelector<HTMLElement>(".transcript-scroll");
    if (!region) throw new Error("No transcript");
    const overlay = document.querySelector<HTMLElement>(".overlay-scrollbar");
    return {
      native: region.offsetWidth - region.clientWidth - region.clientLeft * 2,
      overlay: overlay ? Number(getComputedStyle(overlay).opacity) : null,
      scrolls: region.scrollHeight > region.clientHeight,
    };
  });
}

for (const [width, height] of [
  [1280, 800],
  [390, 844],
] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-132 the transcript's scrollbar (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      await page
        .getByLabel("Message", { exact: true })
        .fill(`Stream Markdown ${"unbroken".repeat(400)}`);
      await page.getByRole("button", { name: "Send" }).click();
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
        timeout: 20_000,
      });
      const region = page.getByRole("region", {
        name: "Transcript",
        exact: true,
      });
      // At rest, a second after the last scroll: no bar of either kind, and none of its width.
      await page.waitForTimeout(1500);
      await settled(page);
      const rest = await bars(page);
      expect(rest.scrolls).toBe(true);
      expect(rest.native).toBe(0);
      expect(rest.overlay).toBe(0);
      const column = await region.boundingBox();
      await capture(page, `${width}-${colorScheme}-rest`);
      // Scrolling shows the overlay; the column keeps its place and width.
      await region.hover();
      await page.mouse.wheel(0, -600);
      await expect.poll(async () => (await bars(page)).overlay).toBe(1);
      expect((await bars(page)).native).toBe(0);
      const shown = await region.boundingBox();
      expect([shown?.x, shown?.width]).toEqual([column?.x, column?.width]);
      const jump = page.getByRole("button", { name: /Jump to latest/ });
      await expect(jump).toBeVisible();
      const places = async () => ({
        composer: await page
          .getByLabel("Message", { exact: true })
          .boundingBox(),
        transcript: await region.boundingBox(),
        jump: await jump.boundingBox(),
      });
      const whileShown = await places();
      await capture(page, `${width}-${colorScheme}-scrolling`);
      // About a second after the last scroll it goes, and nothing moves as it does.
      await expect
        .poll(async () => (await bars(page)).overlay, { timeout: 3000 })
        .toBe(0);
      expect(await places()).toEqual(whileShown);
    });
