import { pick, picker } from "./choices";
import { signIn } from "./session";
import {
  addSharedKey,
  axe,
  capture,
  fitsColumn,
  rowMenu,
} from "./settings-page";
import { expect, test } from "./test";

// WEB-U-052's browser part, on the fake front: when each secret last changed, a due badge that blocks
// nothing, and a reminder set and cleared by keyboard, at the widths UI-GUIDELINES §17.2 asks for in both themes. Each
// run adds, then deletes, its own secret.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-052 rotation reminder ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `reminder-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys/shared");
      const due = page.getByRole("row", { name: /^local(?![\w-])/ });
      await expect(due).toContainText("Rotation due");
      await expect(due).toContainText("Reminder every 90 days");
      await addSharedKey(page, name, "fixture-value");
      await expect(page.getByText(`Added shared key ${name}.`)).toBeVisible();
      const row = page.getByRole("row", {
        name: new RegExp(`^${name}(?![\\w-])`),
      });
      await expect(row).toContainText("No reminder");
      await fitsColumn(page, /^More actions for shared key /);
      await axe(page);

      const open = async () => {
        await rowMenu(
          page,
          `More actions for shared key ${name}`,
          `Set the reminder for shared key ${name}`,
        );
        return page.getByRole("dialog", {
          name: `Rotation reminder for shared key ${name}`,
        });
      };
      let dialog = await open();
      // The standard picker by keyboard (WEB-U-134): focused, opened with ArrowDown, a choice with Enter.
      await picker(page, "Remind me", dialog).focus();
      await page.keyboard.press("ArrowDown");
      await page.getByRole("menuitemradio", { name: /^Other/ }).focus();
      await page.keyboard.press("Enter");
      const days = dialog.getByLabel("Days", { exact: true });
      await days.focus();
      await page.keyboard.type("4000");
      await page.keyboard.press("Enter");
      await expect(
        dialog.getByText("Enter a whole number of days from 1 to 3650."),
      ).toBeVisible();
      await axe(page);
      await capture(page, `secrets-reminder-${width}-${colorScheme}`);
      await pick(page, "Remind me", "Reminder every 90 days", dialog);
      await dialog.getByRole("button", { name: "Save reminder" }).focus();
      await page.keyboard.press("Enter");
      await expect(
        page.getByText(`Saved the reminder for shared key ${name}.`),
      ).toBeVisible();
      await expect(row).toContainText("Reminder every 90 days");
      // Just added, so not due.
      await expect(row).not.toContainText("Rotation due");

      dialog = await open();
      await expect(picker(page, "Remind me", dialog)).toHaveAccessibleName(
        "Remind me: Reminder every 90 days",
      );
      await pick(page, "Remind me", "No reminder", dialog);
      await dialog.getByRole("button", { name: "Save reminder" }).focus();
      await page.keyboard.press("Enter");
      await expect(row).toContainText("No reminder");

      await rowMenu(
        page,
        `More actions for shared key ${name}`,
        `Delete shared key ${name}`,
      );
      await page
        .getByRole("dialog", { name: `Delete shared key ${name}?` })
        .getByRole("button", { name: "Delete key" })
        .click();
      await expect(page.getByText(`Deleted shared key ${name}.`)).toBeVisible();
    });
