import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-092 on the page: renaming a task turns the title itself into the field,
// at its font, size and place, reaching to the header's controls, all of it selected, with no label, no buttons and
// one quiet underline for focus; the status and the pickers don't move. Enter or a click elsewhere saves, Esc puts it
// back, and an empty title keeps the old one. The sidebar row's Rename works the same way.
async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  // A shot of a surface still arriving reads as washed out; wait until nothing moves.
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== "running"),
  );
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}
const box = async (locator: Locator) => {
  const found = await locator.boundingBox();
  if (!found) throw new Error("The part must be visible");
  return found;
};
/** The type the field and the title are drawn in. */
const type = (locator: Locator) =>
  locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return [
      style.fontFamily,
      style.fontSize,
      style.fontWeight,
      style.lineHeight,
    ].join(" ");
  });

for (const width of [1280, 390])
  for (const theme of ["light", "dark"] as const)
    test(`WEB-U-092 the task title is renamed in place ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await signIn(page);
      await page.goto(await createConversation(page));
      const bar = page.getByRole("region", { name: "Task header" });
      const heading = bar.getByRole("heading", { level: 1 });
      await expect(heading).toHaveText("Workspace review");
      const status = bar.getByLabel("Run status");
      // What follows the title on its line: the Files icon (on a phone the pickers have their own line, WEB-U-154).
      const pickers = bar.getByRole("button", { name: "Files" });
      const before = {
        title: await box(heading),
        status: (await status.count()) ? await box(status) : null,
        pickers: await box(pickers),
        titleType: await type(heading),
      };

      // The UI standard's header, at rest and with its ⋯ open (the review's shots).
      await capture(page, `header-${width}-${theme}`);
      await bar
        .getByRole("button", { name: "More actions for Workspace review" })
        .click();
      await expect(
        page.getByRole("menu", { name: "More actions for Workspace review" }),
      ).toBeVisible();
      await capture(page, `header-menu-${width}-${theme}`);
      await page.keyboard.press("Escape");

      await heading.click();
      const field = bar.getByRole("textbox", { name: "Task title" });
      await expect(field).toBeFocused();
      await expect(field).toHaveValue("Workspace review");
      expect(
        await field.evaluate(
          (input: HTMLInputElement) =>
            input.selectionStart === 0 &&
            input.selectionEnd === input.value.length,
        ),
      ).toBe(true);
      // The same type in the same place, reaching to the controls; no label, no buttons, one quiet underline.
      expect(await type(field)).toBe(before.titleType);
      const placed = await box(field);
      expect(Math.abs(placed.x - before.title.x)).toBeLessThanOrEqual(1);
      expect(
        Math.abs(
          placed.y +
            placed.height / 2 -
            (before.title.y + before.title.height / 2),
        ),
      ).toBeLessThanOrEqual(1);
      const barBox = await box(bar);
      const reach =
        before.pickers.y < before.title.y + before.title.height
          ? before.pickers.x
          : barBox.x + barBox.width;
      expect(placed.x + placed.width).toBeLessThanOrEqual(reach);
      expect(placed.x + placed.width).toBeGreaterThan(
        before.title.x + before.title.width - 1,
      );
      await expect(bar.locator("label")).toHaveCount(0);
      await expect(
        bar.getByRole("button", { name: /^(Save title|Cancel)$/ }),
      ).toHaveCount(0);
      expect(
        await field.evaluate((input) => getComputedStyle(input).outlineStyle),
      ).toBe("none");
      if (before.status) expect(await box(status)).toEqual(before.status);
      expect(await box(pickers)).toEqual(before.pickers);
      await capture(page, `rename-title-${width}-${theme}`);

      // Esc puts it back; an empty title keeps the old one.
      await field.fill("Something else");
      await field.press("Escape");
      await expect(heading).toHaveText("Workspace review");
      // Back to the ⋯, the keyboard's way to rename; its Rename opens the field again.
      const more = bar.getByRole("button", {
        name: "More actions for Workspace review",
      });
      await expect(more).toBeFocused();
      await more.click();
      await page.getByRole("menuitem", { name: "Rename task" }).click();
      await field.fill("   ");
      await field.press("Enter");
      await expect(heading).toHaveText("Workspace review");

      // Enter saves.
      await heading.click();
      await field.fill("Quarterly sales cleanup");
      await field.press("Enter");
      await expect(heading).toHaveText("Quarterly sales cleanup");
      await expect(page).toHaveTitle(
        "Quarterly sales cleanup · TensorQuay Agent",
      );

      // A click elsewhere saves too.
      await heading.click();
      await field.fill("Sales cleanup");
      await page.getByRole("textbox", { name: "Message", exact: true }).click();
      await expect(heading).toHaveText("Sales cleanup");
      await expect(field).toHaveCount(0);
    });

for (const width of [1280, 390])
  test(`WEB-U-092 a sidebar row is renamed in place ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await signIn(page);
    await page.goto(await createConversation(page));
    if (width < 768) {
      await page.getByRole("button", { name: "Open navigation" }).click();
    }
    const list = page.getByRole("region", { name: "Tasks" }).last();
    const row = list.getByRole("link", { name: /^Workspace review/ }).first();
    await expect(row).toBeVisible();
    const rowType = await type(row.locator(".task-title"));
    // The row's menu (WEB-U-086): from a right-click on the row with a mouse, from its ⋯ on a phone.
    if (width < 768) {
      await row.hover();
      await list
        .getByRole("button", { name: "More actions for Workspace review" })
        .first()
        .click();
    } else await row.click({ button: "right" });
    const menu = page.getByRole("menu", {
      name: "More actions for Workspace review",
    });
    await expect(menu.getByRole("menuitem")).toHaveText([
      "Rename task",
      "Delete task",
    ]);
    await expect(
      menu.getByRole("menuitem").first().locator("svg"),
    ).toBeVisible();
    await menu.getByRole("menuitem", { name: "Rename task" }).click();
    await expect(page.getByRole("dialog", { name: "Rename task" })).toHaveCount(
      0,
    );
    const field = list.getByRole("textbox", { name: "Task title" });
    await expect(field).toBeFocused();
    expect(await type(field)).toBe(rowType);
    await capture(page, `rename-row-${width}`);
    await field.fill("Board summary");
    await field.press("Enter");
    await expect(
      list.getByRole("link", { name: /^Board summary/ }).first(),
    ).toBeVisible();
  });
