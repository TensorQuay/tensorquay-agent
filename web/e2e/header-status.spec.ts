import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-U-112 on the page (found before 17.0): a long failure in the task header truncates, whole in its tooltip, and
// never pushes out the model and environment pickers (WEB-U-113), the Files icon or the ⋯; under 768 px it reads
// "Failed", its reason being the turn's outcome.
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

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-112 a long failure leaves the header's controls whole ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const input = page.getByLabel("Message", { exact: true });
      await input.fill("Fail with repeated_failure");
      await input.press("Enter");
      const status = page.getByLabel("Run status");
      await expect(status).toHaveText(/^Failed: .{20,}/);
      await expect(page.locator(".turn-outcome")).toBeVisible();

      const header = page.getByRole("region", { name: "Task header" });
      const files = header.getByRole("button", { name: "Files" });
      const more = header.getByRole("button", { name: /^More actions for / });
      const choices = header.getByRole("group", {
        name: "Model and environment",
      });
      const statusBox = await status.boundingBox();
      if (!statusBox) throw new Error("The status must be visible");
      for (const control of [
        choices.getByRole("button", { name: /^Model:/ }),
        choices.getByRole("button", { name: /^Environment:/ }),
        files,
        more,
      ]) {
        await expect(control).toBeVisible();
        const box = await control.boundingBox();
        if (!box) throw new Error("The control must be visible");
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(width);
        // Nothing of the status runs under a control on its line (on a phone the pickers have a line of their own
        // under the title, WEB-U-154).
        if (box.y < statusBox.y + statusBox.height - 1)
          expect(statusBox.x + statusBox.width).toBeLessThanOrEqual(box.x);
      }
      // The whole status is in its tooltip.
      await expect(status).toHaveAttribute(
        "title",
        (await status.textContent()) ?? "",
      );
      // At every width only its word shows (WEB-U-155); the reason is the turn's outcome below.
      const long = status.getByRole("status");
      const shown = await status.evaluate(
        (element) => getComputedStyle(element, "::before").content,
      );
      expect(shown).toContain("Failed");
      // And it shows: the word has its room.
      expect((await status.boundingBox())?.width ?? 0).toBeGreaterThan(30);
      expect(
        await long.evaluate((element) => element.getBoundingClientRect().width),
      ).toBeLessThanOrEqual(1);
      await capture(page, `header-failed-${width}-${theme}`);
    });
