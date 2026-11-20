import { PRICING, pageProblems } from "./channel-page";
import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-E2E-145 (18.2 slice 3, the mock of 17 Nov 2026 and its defaults decided on 20 Nov 2026): the chat page at 1440 and
// 390 px, light and dark. The header (who is working, the budget, To-dos and Members), the grouped log with the PDF, the
// folded report, the open proposal with "2 replies", and the pinned composer; the thread beside the log at 1440 and a
// sheet over it at 390. No sideways scroll, nothing off the tokens, no serious axe finding.
for (const width of [1440, 390] as const)
  for (const theme of ["light", "dark"] as const)
    test(`WEB-E2E-145 the chat page at ${width} px, ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(PRICING);
      const header = page.getByRole("region", { name: "Channel header" });
      await expect(
        header.getByRole("heading", { level: 1, name: "Q4 pricing review" }),
      ).toBeVisible();
      await expect(header.getByText("2 working")).toBeVisible();
      await expect(header.getByText("Budget 38 % used")).toBeVisible();
      const log = page.getByRole("list", { name: "Channel log" });
      await expect(
        log.getByText("The Q3 numbers are in.", { exact: false }),
      ).toBeVisible();
      await expect(
        log.getByText("Pro churn in the EU was 1.8 %", { exact: false }),
      ).toBeVisible();
      await expect(
        log.getByRole("link", { name: /q3-revenue-by-region\.pdf/ }),
      ).toBeVisible();
      await expect(
        log.getByRole("button", { name: "Show the whole report · 14 lines" }),
      ).toBeVisible();
      await expect(
        log.getByRole("button", { name: "2 replies" }),
      ).toBeVisible();
      const composer = page.getByRole("form", { name: "Post to the channel" });
      await expect(
        composer.getByRole("button", { name: "Send" }),
      ).toBeInViewport();
      expect(await pageProblems(page, `the log at ${width}, ${theme}`)).toEqual(
        [],
      );
      await log.getByRole("button", { name: "2 replies" }).click();
      const thread = page.getByRole("complementary", { name: "Thread" });
      await expect(
        thread.getByText("I'll recheck VAT before I vote."),
      ).toBeVisible();
      const box = await thread.boundingBox();
      if (width === 390) expect(box?.width).toBe(390);
      else expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(1440);
      expect(
        await pageProblems(page, `the thread at ${width}, ${theme}`),
      ).toEqual([]);
      await thread.getByRole("button", { name: "Close Thread" }).click();
      await header.getByRole("button", { name: /^To-dos/ }).click();
      await expect(
        page.getByRole("complementary", { name: "To-dos" }),
      ).toBeVisible();
      expect(
        await pageProblems(page, `the to-dos at ${width}, ${theme}`),
      ).toEqual([]);
    });
