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
      await expect(header.getByText("Budget 38 % used today")).toBeVisible();
      const log = page.getByRole("list", { name: "Channel log" });
      await expect(
        log.getByText("The Q3 numbers are in.", { exact: false }),
      ).toBeVisible();
      await expect(
        log.getByText("Pro churn in the EU was 1.8 %", { exact: false }),
      ).toBeVisible();
      const pdf = log.getByRole("link", { name: /q3-revenue-by-region\.pdf/ });
      await expect(pdf).toBeVisible();
      // A post's file downloads under its own name (the review of 20 Nov 2026).
      const [download] = await Promise.all([
        page.waitForEvent("download"),
        pdf.click(),
      ]);
      expect(download.suggestedFilename()).toBe("q3-revenue-by-region.pdf");
      await expect(
        log.getByRole("button", { name: "Show the whole report · 14 lines" }),
      ).toBeVisible();
      await expect(
        log.getByRole("button", { name: "2 replies" }),
      ).toBeVisible();
      // An agent's answer is a reply, and it reads in the log without a click (C-10, the review of 20 Nov 2026).
      await expect(
        log.getByText("Agreed: churn stays under the line at €24."),
      ).toBeVisible();
      await expect(
        log
          .getByRole("list", { name: /^Replies to #/ })
          .first()
          .getByText("Pricing analyst"),
      ).toBeVisible();
      if (width === 1440) {
        // A post's time and Reply wait for the pointer, or for the keyboard (the review of 20 Nov 2026): hidden at
        // rest, shown on hover, and shown while Tab is on its Reply.
        const post = log
          .getByRole("article")
          .filter({ hasText: "The Q3 numbers are in." })
          .first();
        const meta = post.locator(".channel-post-meta").first();
        const opacity = () =>
          meta.evaluate((element) => getComputedStyle(element).opacity);
        // At rest: no pointer over it and no focus in it (the download above left the focus on its file).
        await page.mouse.move(0, 0);
        await page.evaluate(() => {
          (document.activeElement as HTMLElement | null)?.blur();
        });
        expect(await opacity()).toBe("0");
        await post.hover();
        expect(await opacity()).toBe("1");
        await page.mouse.move(0, 0);
        const reply = post.getByRole("button", { name: /^Reply/ });
        await reply.evaluate((element) => {
          (element.previousElementSibling as HTMLElement | null)?.focus();
        });
        await page.keyboard.press("Tab");
        await expect(reply).toBeFocused();
        expect(await opacity()).toBe("1");
      }
      const composer = page.getByRole("form", { name: "Post to the channel" });
      await expect(
        composer.getByRole("button", { name: "Send" }),
      ).toBeInViewport();
      // The task's composer (the review of 20 Nov 2026): one card, + / the text / a round Send in one row, under the
      // post's kind; at rest no taller than 140 px (it was 265 at 1440).
      const shape = await composer.evaluate((form) => {
        const row = form.querySelector(".composer-row");
        const centre = (element: Element | null | undefined) => {
          const box = element?.getBoundingClientRect();
          return box ? Math.round(box.top + box.height / 2) : -1;
        };
        return {
          card: form.classList.contains("composer-card"),
          height: Math.round(form.getBoundingClientRect().height),
          row: [
            centre(row?.querySelector(".composer-attach")),
            centre(row?.querySelector("textarea")),
            centre(row?.querySelector(".composer-submit")),
          ],
        };
      });
      expect(shape.card).toBe(true);
      expect(shape.height).toBeLessThanOrEqual(140);
      expect(new Set(shape.row).size, shape.row.join(",")).toBe(1);
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
      // Room for whole posts (the review of 20 Nov 2026): each author's name and "Agent" on one line, replies flush
      // with the root's text, and a reply's text at least 240 px wide.
      const fit = await thread.evaluate((aside) => {
        const lines = [...aside.querySelectorAll(".channel-post-who")].map(
          (who) => {
            const box = who.getBoundingClientRect();
            const line = Number.parseFloat(getComputedStyle(who).lineHeight);
            return Math.round(box.height / line);
          },
        );
        const root = aside.querySelector(".channel-post p");
        const reply = aside.querySelector(".channel-replies .channel-post p");
        return {
          lines,
          flush:
            Math.round(root?.getBoundingClientRect().left ?? 0) ===
            Math.round(reply?.getBoundingClientRect().left ?? -1),
          text: Math.round(reply?.getBoundingClientRect().width ?? 0),
        };
      });
      expect({
        lines: fit.lines.filter((count) => count !== 1),
        flush: fit.flush,
        roomy: fit.text >= 240,
      }).toEqual({ lines: [], flush: true, roomy: true });
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

// WEB-E2E-150 (18.2.1; the lead's live check of 23 Nov 2026): a channel opens at its newest message. At 390 px the
// pricing channel's log is several screens tall; on 18.2 the page opened at its first post.
test("WEB-E2E-150 a channel opens at its newest message", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await signIn(page);
  await page.goto(PRICING);
  const log = page.getByRole("list", { name: "Channel log" });
  await expect(
    log.getByText(
      "Woken agents have used 80 % of this channel's budget for today.",
    ),
  ).toBeInViewport();
  await expect(
    log.getByText("The Q3 numbers are in.", { exact: false }),
  ).not.toBeInViewport();
});
