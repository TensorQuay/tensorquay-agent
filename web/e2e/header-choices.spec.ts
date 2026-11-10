import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-113 on the page (the UI standard's step 2 B, corrected on 30 Sep 2026): a task's model and environment are two
// small pickers in its header, right of the title and status, before the Files icon and the ⋯; each opens below and
// applies at once. The composer is "+", the text and Send. The header never overflows (WEB-U-112).
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

async function box(locator: Locator) {
  const found = await locator.boundingBox();
  if (!found) throw new Error("The part must be visible");
  return found;
}

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-113 the task's choices sit in its header ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const header = page.getByRole("region", { name: "Task header" });
      await expect(
        header.getByRole("button", { name: "Change model or environment" }),
      ).toHaveCount(0);
      const choices = header.getByRole("group", {
        name: "Model and environment",
      });
      const model = choices.getByRole("button", { name: /^Model: / });
      const environment = choices.getByRole("button", {
        name: /^Environment: /,
      });
      await expect(model).toBeVisible();
      await expect(environment).toBeVisible();
      // The composer holds "+", the text and Send only.
      const composer = page.getByRole("form", { name: "Message composer" });
      await expect(
        composer.getByRole("group", { name: "Model and environment" }),
      ).toHaveCount(0);
      await expect(
        composer.getByRole("button", { name: /^Model:/ }),
      ).toHaveCount(0);

      // One line, in order: the status, the pickers, the Files icon, the ⋯; all inside the window. On a phone the pickers
      // take a line of their own under the title (WEB-U-154): the first line is the status, Files and ⋯.
      const status = header.getByRole("group", { name: "Run status" });
      // The status keeps its word at every width ("Ready").
      await expect(status).toHaveText(/\S/);
      expect((await box(status)).width).toBeGreaterThan(30);
      const files = header.getByRole("button", { name: "Files" });
      const more = header.getByRole("button", { name: /^More actions for / });
      const phone = width < 768;
      const lines = phone
        ? [
            [status, files, more],
            [model, environment],
          ]
        : [[status, model, environment, files, more]];
      const boxes = [];
      for (const line of lines) {
        const placed = [];
        for (const part of line) placed.push(await box(part));
        for (let index = 1; index < placed.length; index += 1) {
          const [before, after] = [placed[index - 1], placed[index]];
          if (!before || !after) throw new Error("Each part has a box");
          expect(before.x + before.width).toBeLessThanOrEqual(after.x + 1);
          expect(
            Math.abs(
              before.y + before.height / 2 - (after.y + after.height / 2),
            ),
          ).toBeLessThanOrEqual(2);
        }
        boxes.push(...placed);
      }
      if (phone) {
        const [statusBox, modelBox] = [await box(status), await box(model)];
        expect(modelBox.y).toBeGreaterThanOrEqual(
          statusBox.y + statusBox.height - 1,
        );
      }
      const last = boxes.at(-1);
      if (!last) throw new Error("The ⋯ has a box");
      expect(last.x + last.width).toBeLessThanOrEqual(width);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      await capture(page, `header-choices-${width}-${theme}`);

      // It opens below, and choosing applies at once.
      await model.click();
      const menu = page.getByRole("menu");
      await expect(menu).toBeVisible();
      const menuBox = await box(menu);
      const modelBox = await box(model);
      expect(menuBox.y).toBeGreaterThanOrEqual(modelBox.y + modelBox.height);
      expect(menuBox.x).toBeGreaterThanOrEqual(0);
      expect(menuBox.x + menuBox.width).toBeLessThanOrEqual(width);
      await capture(page, `header-choices-model-${width}-${theme}`);
      // Each row has its muted second line (WEB-U-103); the button is named after the chosen model.
      const other = menu.getByRole("menuitemradio", { name: /^Second model/ });
      await expect(other).toContainText("Thinks before answering");
      await other.click();
      await expect(menu).toHaveCount(0);
      await expect(model).toHaveAccessibleName("Model: Second model");
    });
