import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-U-102 on the page, as the owner corrected it (WEB-U-113): New task's model and environment are two small pickers
// in its top bar; the composer's "+" offers Attach files, Workspace… and Agent profile…; a workspace or profile shows
// as a chip whose picker opens at once, and its × goes back to the default. No quiet line, no Change panel.
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
    test(`WEB-U-102 New task's composer ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto("/");
      const composer = page.getByRole("form", { name: "Message composer" });
      const bar = page.getByRole("region", { name: "Task header" });
      const choices = bar.getByRole("group", {
        name: "Model and environment",
      });
      await expect(
        composer.getByRole("group", { name: "Model and environment" }),
      ).toHaveCount(0);
      await expect(
        choices.getByRole("button", { name: /^Model: / }),
      ).toBeVisible();
      await expect(
        choices.getByRole("button", { name: /^Environment: / }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Change what this task uses" }),
      ).toHaveCount(0);
      await capture(page, `new-task-${width}-${theme}`);

      // "+" offers what else a new task can use.
      await composer.getByRole("button", { name: "Add" }).click();
      const plus = page.getByRole("menu", { name: "Add" });
      await expect(plus.getByRole("menuitem").first()).toHaveText(
        "Attach files",
      );
      await expect(
        plus.getByRole("menuitem", { name: "Workspace…" }),
      ).toBeVisible();
      await capture(page, `new-task-plus-${width}-${theme}`);
      await plus.getByRole("menuitem", { name: "Workspace…" }).click();
      // The chip's picker opens at once; choosing a workspace keeps the chip.
      await page.getByRole("menuitemradio", { name: /^Team handbook/ }).click();
      const chip = composer.getByRole("button", {
        name: "Workspace: Team handbook",
      });
      await expect(chip).toBeVisible();
      await capture(page, `new-task-chip-${width}-${theme}`);
      // Its × goes back to a new workspace, and the chip leaves.
      await composer
        .getByRole("button", { name: "Remove workspace Team handbook" })
        .click();
      await expect(chip).toHaveCount(0);

      // Closing the chip's picker without a choice leaves no chip behind.
      await composer.getByRole("button", { name: "Add" }).click();
      await page
        .getByRole("menu", { name: "Add" })
        .getByRole("menuitem", { name: "Workspace…" })
        .click();
      await expect(page.getByRole("menuitemradio").first()).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(
        composer.getByRole("button", { name: /^Workspace:/ }),
      ).toHaveCount(0);
    });
