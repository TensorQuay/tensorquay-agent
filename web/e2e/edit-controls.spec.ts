import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

/** The Settings lists with an Edit on each row: some open a page (a link), some a dialog (a button). */
const LISTS = [
  "/settings/agent-profiles",
  "/settings/models",
  "/settings/environments",
  "/settings/connectors",
  "/settings/decision-engines",
  "/settings/internet-access",
  "/settings/limits",
] as const;

/** Every row's Edit on the page, a link or a button. */
const edits = (page: Page) =>
  page
    .locator(".data-table")
    .locator("a, button")
    .filter({ hasText: /^Edit$/ });

// WEB-E2E-108 (17.3, UI-GUIDELINES §7): one Edit control in lists, a small ghost button, never an underlined link, the
// same whether it opens a page or a dialog; in a table cell 32 px high, so a one-line row stays 40 px (44 on touch).
test("WEB-E2E-108 every list's Edit is the same ghost button, never an underlined link", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  for (const href of LISTS)
    await test.step(href, async () => {
      await page.goto(href);
      await expect(edits(page).first()).toBeVisible();
      for (const edit of await edits(page).all()) {
        await expect.soft(edit).toHaveAttribute("data-variant", "ghost");
        await expect.soft(edit).toHaveCSS("text-decoration-line", "none");
        await expect.soft(edit).toHaveCSS("height", "32px");
        await expect.soft(edit).toHaveCSS("font-weight", "700");
      }
      await capture(page, `edit-controls-${href.split("/").at(-1)}`);
    });
});

// On a touch screen the same Edit is the row's 44 px tap target (UI-GUIDELINES §5c).
test.describe("on a touch screen", () => {
  test.use({ hasTouch: true, isMobile: true });
  test("WEB-E2E-108 every list's Edit is 44 px high on a touch screen", async ({
    page,
  }) => {
    test.setTimeout(90_000);
    await page.setViewportSize({ width: 1280, height: 900 });
    await signIn(page);
    for (const href of LISTS)
      await test.step(href, async () => {
        await page.goto(href);
        await expect(edits(page).first()).toBeVisible();
        for (const edit of await edits(page).all())
          await expect.soft(edit).toHaveCSS("height", "44px");
      });
  });
});
