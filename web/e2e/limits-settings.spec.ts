import { picker } from "./choices";
import { signIn } from "./session";
import { axe, capture, fitsColumn } from "./settings-page";
import { expect, test, type Page } from "./test";

// The Limits page on the admin limits API (slice 5, ADR-0028; WEB-U-041 browser part), keyboard only, at the widths
// UI-GUIDELINES §17.2 asks for in both themes. Each run sets one limit and resets it, so the shared fake store is
// back where it started.

async function setIdle(page: Page, minutes: number) {
  await page.getByRole("button", { name: "Edit Sleep after idle" }).focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Edit Sleep after idle" });
  // Edited in the unit the table shows (minutes here), with the range in that unit.
  await expect(picker(page, "Unit", dialog)).toHaveAccessibleName(
    "Unit: Minutes",
  );
  await expect(dialog.getByText("From 1 to 1440.")).toBeVisible();
  const field = dialog.getByRole("spinbutton", { name: "Value" });
  await field.focus();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("0.5");
  await page.keyboard.press("Enter");
  await expect(
    dialog.getByText("Enter a number from 1 to 1440."),
  ).toBeVisible();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type(String(minutes));
  await axe(page);
  await page.keyboard.press("Enter");
  await expect(
    page.getByText(`Saved Sleep after idle: ${minutes} min.`),
  ).toBeVisible();
}

async function resetIdle(page: Page) {
  await page.getByRole("button", { name: "Edit Sleep after idle" }).click();
  await page
    .getByRole("dialog", { name: "Edit Sleep after idle" })
    .getByRole("button", { name: "Reset to the configuration file's value" })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Reset Sleep after idle to 15 min?",
  });
  await dialog.getByRole("button", { name: "Reset limit" }).click();
  await expect(
    page.getByText("Reset Sleep after idle to 15 min."),
  ).toBeVisible();
  // Back at the configuration file's value, nothing is set here to reset (WEB-U-041).
  await page.getByRole("button", { name: "Edit Sleep after idle" }).click();
  const edit = page.getByRole("dialog", { name: "Edit Sleep after idle" });
  await expect(edit.getByRole("button", { name: /^Reset/ })).toHaveCount(0);
  await edit.getByRole("button", { name: "Cancel" }).click();
}

for (const [index, width] of [1024, 1280].entries())
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-041 limits for administrators ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/limits");
      await expect(page).toHaveTitle("Limits · TensorQuay Agent");
      const slots = page.getByRole("row", { name: /Tasks running at once/ });
      // WEB-U-041: what differs from the configuration file, in a sentence.
      await expect(
        slots.getByText(
          "Lowered from 12 tasks when the configuration file changed; 8 tasks is the most allowed.",
        ),
      ).toBeVisible();
      // Sizes are each environment's now; the page shows only the configuration file's ceiling for them.
      await expect(
        page.getByRole("row", { name: /per environment/ }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("region", { name: "Largest environment" }),
      ).toContainText("Environments can use up to 8 CPUs, 16 GB of memory");
      // ADR-0035: nothing is parked, so there is no limit for it.
      await expect(page.getByRole("row", { name: /^Park / })).toHaveCount(0);
      // Rows hold only Edit; a reset is in the edit dialog.
      await expect(page.getByRole("button", { name: /^Reset / })).toHaveCount(
        0,
      );
      await fitsColumn(page, /^Edit /);
      await axe(page);
      await capture(page, `limits-list-${width}-${colorScheme}`);
      await setIdle(page, 20 + index * 2 + (colorScheme === "dark" ? 1 : 0));
      await capture(page, `limits-saved-${width}-${colorScheme}`);
      await resetIdle(page);
      await axe(page);
    });

test("WEB-U-041 members see no Limits section", async ({ page }) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await page.goto("/settings/limits");
  await expect(
    page.getByText("Only administrators can manage limits."),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Limits" }),
  ).toHaveCount(0);
});
