import { signIn } from "./session";
import { axe, capture } from "./settings-page";
import { expect, test, type Page } from "./test";

// WEB-U-190 in the browser: where a settings table's own column is narrow (here a phone), its rows are cards rather
// than a table to scroll sideways: the row's name with its ⋯ beside it, each other cell under its column's name. On a
// laptop the same tables are tables. Every settings page with a table on the fake front, in both themes.
const PAGES = [
  "/settings/people",
  "/settings/keys",
  "/settings/keys/shared",
  "/settings/internet-access",
  "/settings/models",
  "/settings/environments",
  "/settings/connectors",
  "/settings/agent-profiles",
  "/settings/decision-engines",
] as const;

/** Waits for the page's tables to fill, and returns how many rows they hold. */
async function filled(page: Page) {
  const rows = page.locator(".data-table tbody tr");
  await expect(rows.first()).toBeVisible();
  await expect(page.locator(".skeleton")).toHaveCount(0);
  return rows.count();
}

for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-U-190 settings tables are cards on a phone 390 ${colorScheme}`, async ({
    page,
  }) => {
    test.setTimeout(180_000);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    for (const path of PAGES) {
      await page.goto(path);
      const rows = await filled(page);
      expect(rows, path).toBeGreaterThan(0);
      // Nothing scrolls sideways: not the page, and not a table in its own box.
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
        path,
      ).toBe(true);
      for (const box of await page.locator(".data-table-scroll").all()) {
        await expect(box, path).not.toHaveAttribute("tabindex");
        expect(
          await box.evaluate(
            (element) => element.scrollWidth <= element.clientWidth,
          ),
          path,
        ).toBe(true);
      }
      for (const row of await page.locator(".data-table tbody tr").all()) {
        // A card, and every cell but the name and the actions under its column's name.
        expect(
          await row.evaluate((element) => getComputedStyle(element).display),
          path,
        ).toBe("grid");
        const labels = await row
          .locator("td")
          .evaluateAll((cells) =>
            cells.map((cell) => cell.getAttribute("data-label")),
          );
        expect(labels[0], path).toBeNull();
        // The row's actions, when it has any, are in view as they are: the ⋯ beside the name.
        const more = row.getByRole("button", { name: /^More actions/ });
        if ((await more.count()) > 0) {
          await more.first().scrollIntoViewIfNeeded();
          await expect(more.first(), path).toBeInViewport({ ratio: 1 });
        }
      }
      await expect(page.getByRole("table").first(), path).toBeVisible();
      await axe(page);
      await capture(
        page,
        `narrow-tables${path.replaceAll("/", "-")}-390-${colorScheme}`,
        { fullPage: true },
      );
    }
  });

test("WEB-U-190 the same tables are tables on a laptop", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  for (const path of PAGES) {
    await page.goto(path);
    await filled(page);
    expect(
      await page
        .locator(".data-table tbody tr")
        .first()
        .evaluate((element) => getComputedStyle(element).display),
      path,
    ).toBe("table-row");
    await expect(page.locator(".data-table thead").first()).toBeVisible();
  }
});
