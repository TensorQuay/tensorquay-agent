import { join } from "node:path";

import { PRICING } from "./channel-page";
import { signIn } from "./session";
import { test } from "./test";

/** Where the gallery's pictures go; unset, the gallery is skipped (it gates nothing). */
const GALLERY = process.env.TQA_CHANNEL_GALLERY_DIR;

// WEB-E2E-146 (18.2 slice 3): the channel page as the Product Owner sees it before the release, 1440 and 390 px, light
// and dark: the log, its thread, its to-dos and members. Non-gating: run with TQA_CHANNEL_GALLERY_DIR set.
for (const width of [1440, 390] as const)
  for (const theme of ["light", "dark"] as const)
    test(`WEB-E2E-146 the channel page's gallery at ${width} px, ${theme}`, async ({
      browser,
    }) => {
      test.skip(!GALLERY, "set TQA_CHANNEL_GALLERY_DIR to take the gallery");
      // 390 is a phone: touch, so the quiet lines show as they would in a hand.
      const phone = width === 390;
      const context = await browser.newContext({
        viewport: { width, height: phone ? 844 : 900 },
        deviceScaleFactor: 2,
        isMobile: phone,
        hasTouch: phone,
        colorScheme: theme,
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      const shot = async (name: string) =>
        page.screenshot({
          path: join(GALLERY ?? "", `channel-${width}-${theme}-${name}.png`),
          fullPage: width === 390,
        });
      await signIn(page);
      await page.goto(PRICING);
      const log = page.getByRole("list", { name: "Channel log" });
      await log.getByRole("button", { name: "2 replies" }).waitFor();
      // The posts' Markdown renders lazily: each picture waits for the words.
      await log.getByText("The Q3 numbers are in.", { exact: false }).waitFor();
      await page.evaluate(() => document.fonts.ready);
      await shot("log");
      await log.getByRole("button", { name: "2 replies" }).click();
      await page.getByRole("complementary", { name: "Thread" }).waitFor();
      await shot("thread");
      await page.getByRole("button", { name: "Close Thread" }).click();
      const header = page.getByRole("region", { name: "Channel header" });
      await header.getByRole("button", { name: /^To-dos/ }).click();
      await shot("todos");
      // On a phone the sheet covers the header: closed first, as a person would.
      await page.getByRole("button", { name: "Close To-dos" }).click();
      await header.getByRole("button", { name: /^Members/ }).click();
      await shot("members");
      await context.close();
    });
