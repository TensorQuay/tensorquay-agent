import { signIn } from "./session";
import { axe, capture, fitsColumn, rowMenu, type } from "./settings-page";
import { expect, test } from "./test";

// A person's own connector keys (ADR-0031; WEB-U-044 browser part), keyboard first, at the widths
// UI-GUIDELINES §17.2 asks for in both themes. The value is typed once and never appears in the page again.
const VALUE = "tok-e2e-5f4e3d2c1b";

for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-044 connector keys ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys");
      await expect(page).toHaveTitle("Keys · TensorQuay Agent");
      const tracker = page.getByRole("row", { name: /Issue tracker/ });
      await expect(tracker).toContainText("Needed");
      await expect(page.getByRole("row", { name: /wiki/ })).toHaveCount(0);
      await fitsColumn(page, /^(Add|Replace) your key for /);
      await axe(page);
      await capture(page, `connector-keys-${width}-${colorScheme}`);
      await page
        .getByRole("button", { name: "Add your key for Issue tracker" })
        .focus();
      await page.keyboard.press("Enter");
      await expect(
        page.getByRole("dialog", {
          name: "Add your key for Issue tracker",
        }),
      ).toBeVisible();
      await type(page, "Key", VALUE);
      await axe(page);
      await page.keyboard.press("Enter");
      await expect(
        page.getByText("Saved your key for Issue tracker."),
      ).toBeVisible();
      await expect(tracker).toContainText("Added");
      expect(await page.content()).not.toContain(VALUE);
      await rowMenu(
        page,
        "More actions for your Issue tracker key",
        "Remove your key for Issue tracker",
      );
      await page
        .getByRole("dialog", {
          name: "Remove your key for Issue tracker?",
        })
        .getByRole("button", { name: "Remove key" })
        .click();
      await expect(
        page.getByText("Removed your key for Issue tracker."),
      ).toBeVisible();
      await expect(tracker).toContainText("Needed");
    });
