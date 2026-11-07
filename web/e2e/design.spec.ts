import { fileURLToPath } from "node:url";

import AxeBuilder from "@axe-core/playwright";

import { signIn } from "./session";
import { expect, test } from "./test";

test.beforeEach(async ({ page }) => {
  await signIn(page);
});

test("WEB-U-007 product navigation omits the development reference", async ({
  page,
}) => {
  await page.goto("/");
  await expect(
    page.getByRole("navigation", { name: "Main navigation" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Design reference" }),
  ).toHaveCount(0);
});

// WEB-U-007: the same reference is checked for contrast, accessibility and appearance.
for (const theme of ["light", "dark"] as const) {
  test(`design screenshot ${theme}`, async ({ page }) => {
    await page.goto("/design");
    await page.evaluate(async () => {
      await document.fonts.ready;
    });
    const reference = page.getByTestId(`design-${theme}`);
    await expect(reference).toHaveAttribute("data-ready", "true");
    // The defined state is waited for above (fonts, the panel's tokens read). What's left is the capture itself: two
    // identical frames of a 554 x 4495 px panel, then its raster. Measured with the CPU throttled, it took 0.15 s at 1x
    // and 3.7 s at 40x, so a loaded host ran out the default 5 s (TEST-PLAN §19a, 1 Oct 2026). Its budget is the
    // work's, not a wait for the page.
    await expect(reference).toHaveScreenshot(`design-${theme}.png`, {
      timeout: 30_000,
      animations: "disabled",
      // Capture the complete reference panel; conversation tests check the fixed viewport separately.
      stylePath: fileURLToPath(
        new URL("./reference-capture.css", import.meta.url),
      ),
    });
  });
  for (const path of ["/", "/login", "/design"]) {
    test(`accessibility ${theme} ${path}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto(path);
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      const results = await new AxeBuilder({ page }).analyze();
      expect(
        results.violations.filter(
          ({ impact }) => impact === "serious" || impact === "critical",
        ),
      ).toEqual([]);
    });
  }
}

for (const width of [360, 768, 1024, 1440]) {
  test(`no horizontal overflow at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    for (const path of ["/", "/login", "/design"]) {
      await page.goto(path);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);
    }
  });
}

test("dialog traps keyboard focus and returns it to the trigger", async ({
  page,
}) => {
  await page.goto("/design");
  const trigger = page
    .getByTestId("design-light")
    .getByRole("button", { name: "Open dialog", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Example dialog" });
  await expect(dialog).toBeVisible();
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press("Tab");
    expect(
      await dialog.evaluate((element) =>
        element.contains(document.activeElement),
      ),
    ).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(trigger).toBeFocused();
});

test("loading buttons retain their width", async ({ page }) => {
  await page.goto("/design");
  const reference = page.getByTestId("design-light");
  // Measured once drawn: the reference page is long, and a box read before it is drawn is null.
  await expect(reference.getByTestId("primary-loading")).toBeVisible();
  const ready = await reference.getByTestId("primary-default").boundingBox();
  const loading = await reference.getByTestId("primary-loading").boundingBox();
  expect(ready?.width).toBe(loading?.width);
});

test("theme choice survives navigation and overrides the system preference", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/");
  await expect(
    page.getByRole("textbox", { name: "Message", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "Account", exact: true }).click();
  const theme = page.getByRole("menuitemradio", {
    name: "System",
    exact: true,
  });
  await theme.focus();
  await expect(theme).toBeFocused();
  await page.keyboard.press("l");
  await page.keyboard.press("Enter");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.keyboard.press("Escape");
  await page.reload();
  await page.getByRole("button", { name: "Account", exact: true }).click();
  await expect(
    page.getByRole("menuitemradio", { name: "Light", exact: true }),
  ).toHaveAttribute("aria-checked", "true");
  expect(
    await page
      .locator("html")
      .evaluate((element) => getComputedStyle(element).colorScheme),
  ).toBe("light");
});
