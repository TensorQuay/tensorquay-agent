import { signIn } from "./session";
import {
  addSharedKey,
  axe,
  capture,
  fitsColumn,
  rowMenu,
} from "./settings-page";
import { expect, test } from "./test";

// WEB-U-054's browser part, on the fake front: who may use a secret, changed by keyboard in its dialog, at
// the widths UI-GUIDELINES §17.2 asks for in both themes. Each run adds, binds, then deletes its own secret.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-054 secret bindings ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `bind-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys/shared");
      // Used by names the routes that send a key (Keys › Shared, WEB-U-076): the seeded key is local's.
      await expect(
        page.getByRole("row", { name: /^local(?![\w-])/ }),
      ).not.toContainText("Not used");
      await addSharedKey(page, name, "fixture-value");
      await expect(page.getByText(`Added shared key ${name}.`)).toBeVisible();
      const row = page.getByRole("row", {
        name: new RegExp(`^${name}(?![\\w-])`),
      });
      await expect(row).toContainText("Not used");
      await fitsColumn(page, /^More actions for shared key /);
      await rowMenu(
        page,
        `More actions for shared key ${name}`,
        `Change who may use shared key ${name}`,
      );
      const dialog = page.getByRole("dialog", {
        name: `Who may use shared key ${name}`,
      });
      // Grouped by kind, so each box reads its plain name (F8).
      const local = dialog
        .getByRole("group", { name: "Model routes" })
        .getByRole("checkbox", { name: "local" });
      await local.focus();
      await page.keyboard.press("Space");
      await expect(local).toBeChecked();
      await axe(page);
      await capture(page, `secret-bindings-${width}-${colorScheme}`);
      await dialog.getByRole("button", { name: "Save" }).focus();
      await page.keyboard.press("Enter");
      await expect(
        page.getByText(`Saved who may use shared key ${name}.`),
      ).toBeVisible();
      // Allowing a route doesn't make it send the key: the row still reads Not used, and the dialog keeps the choice.
      await expect(row).toContainText("Not used");
      await rowMenu(
        page,
        `More actions for shared key ${name}`,
        `Change who may use shared key ${name}`,
      );
      await expect(
        page
          .getByRole("dialog", { name: `Who may use shared key ${name}` })
          .getByRole("group", { name: "Model routes" })
          .getByRole("checkbox", { name: "local" }),
      ).toBeChecked();
      await page.keyboard.press("Escape");
      await fitsColumn(page, /^More actions for shared key /);
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
