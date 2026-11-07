import { signIn } from "./session";
import { axe, capture, rowMenu, type } from "./settings-page";
import { expect, test, type Page } from "./test";

// WEB-E2E-031: the Environment page (the UI standard's Page; WEB-U-097 and WEB-U-098 in the browser) on the fake
// front's image list, at 1280 and 390 in both themes. Each run adds and deletes its own environment; the reset test
// leaves Data science customised again, as it found it.

const form = (page: Page) => page.locator("form.settings-form-page");
/**
 * The pinned Save bar sits flush on the window's bottom edge while the page scrolls under it: nothing of the page
 * shows below it.
 */
const flushSaveBar = async (page: Page) => {
  const bar = page.getByRole("group", { name: "Save or cancel" });
  const box = await bar.boundingBox();
  if (!box) throw new Error("The Save bar must be visible");
  const height = page.viewportSize()?.height ?? 0;
  expect(Math.round(box.y + box.height), "the bar's bottom edge").toBe(height);
};

/** No page-wide sideways scroll at this width. */
const noSideways = async (page: Page) =>
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth,
    ),
  ).toBeLessThanOrEqual(0);

for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-031 adds an environment from a picked image ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const display = "Notebooks";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      // A deep link works on its own.
      await page.goto("/settings/environments/new");
      const page_ = form(page);
      await expect(
        page_.getByRole("heading", { level: 2, name: "Add an environment" }),
      ).toBeVisible();
      const cards = page_.getByRole("radiogroup", {
        name: "Start from a built-in",
      });
      await expect(cards.getByRole("radio")).toHaveCount(3);
      // WEB-U-184: a new environment starts from the default built-in, chosen, its tools said once.
      await expect(cards.getByRole("radio", { checked: true })).toHaveCount(1);
      await expect(
        page_
          .getByRole("region", { name: "Capabilities" })
          .getByRole("checkbox"),
      ).toHaveCount(0);
      await expect(
        page_.getByRole("region", { name: "Capabilities" }),
      ).toContainText("Set by ");
      await expect(
        page_
          .getByRole("group", { name: "Save or cancel" })
          .getByRole("button", {
            name: "Save",
          }),
      ).toBeInViewport();
      await noSideways(page);
      await flushSaveBar(page);
      // Scrolled halfway, still flush.
      await page.locator("#main").evaluate((main) => main.scrollBy(0, 300));
      await flushSaveBar(page);
      await page.locator("#main").evaluate((main) => main.scrollTo(0, 0));
      await axe(page);
      await capture(page, `environment-new-${width}-${colorScheme}`);
      // The picker by keyboard: the host's own images, one line each; the unusable ones say why.
      const picker = page_.getByRole("button", {
        name: "Or pick one of your images: Choose an image",
      });
      await picker.focus();
      await page.keyboard.press("ArrowDown");
      const menu = page.getByRole("menu", {
        name: "Or pick one of your images",
      });
      await expect(menu.getByRole("menuitemradio")).toHaveCount(4);
      await expect(
        menu.getByRole("menuitemradio", { name: /team\/old-tools:2025/ }),
      ).toHaveAttribute("aria-disabled", "true");
      await expect(
        menu.getByRole("menuitemradio", { name: /team\/old-tools:2025/ }),
      ).toContainText("Too old for this version");
      await axe(page);
      await capture(page, `environment-picker-${width}-${colorScheme}`);
      await menu
        .getByRole("menuitemradio", { name: /team\/notebooks:1/ })
        .click();
      await expect(menu).toHaveCount(0);
      await expect(
        page_.getByRole("button", {
          name: "Or pick one of your images: team/notebooks:1",
        }),
      ).toBeFocused();
      await expect(
        page_.getByRole("checkbox", { name: "Browser" }),
      ).toBeChecked();
      await type(page, "Display name", display, page_);
      // The ID follows the display name on a quiet line (WEB-U-097), and the bar stays flush while there are changes.
      await expect(
        page_.getByRole("region", { name: "Environment" }),
      ).toContainText("ID: notebooks · Change");
      await flushSaveBar(page);
      await capture(page, `environment-id-line-${width}-${colorScheme}`);
      // The Save bar holds two status lines: this one, and the count of fields to fix (WEB-U-110).
      await expect(page_.getByText("Unsaved changes")).toHaveRole("status");
      await page_.getByRole("button", { name: "Save" }).click();
      await expect(page).toHaveURL(/\/settings\/environments$/);
      await expect(
        page.getByText(
          `Saved environment ${display}. The gateway checks it next.`,
        ),
      ).toBeVisible();
      // Edit keeps its image: the picker names it, and Advanced stays closed.
      await page
        .getByRole("link", { name: `Edit environment ${display}` })
        .click();
      await expect(
        form(page).getByRole("button", {
          name: "Or pick one of your images: team/notebooks:1",
        }),
      ).toBeVisible();
      await expect(
        form(page).locator("details", { hasText: "Advanced" }).first(),
      ).not.toHaveAttribute("open", "");
      await form(page).getByRole("link", { name: "Environments" }).click();
      await rowMenu(
        page,
        `More actions for environment ${display}`,
        `Delete environment ${display}`,
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete environment" })
        .click();
      await expect(
        page.getByText(`Deleted environment ${display}.`),
      ).toBeVisible();
    });

test("WEB-E2E-031 asks before leaving unsaved changes, and starts from a built-in card", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  await page.goto("/settings/environments");
  await page.getByRole("link", { name: "Edit environment Minimal" }).click();
  await type(page, "Display name", "Minimal edited", form(page));
  // A link elsewhere on the page (the Settings menu) is held while the page asks.
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Models" })
    .click();
  const ask = page.getByRole("dialog", { name: "Leave without saving?" });
  await expect(ask).toBeVisible();
  await axe(page);
  await ask.getByRole("button", { name: "Keep editing" }).click();
  await expect(page).toHaveURL(/\/settings\/environments\/minimal$/);
  await form(page).getByRole("button", { name: "Cancel" }).click();
  await ask.getByRole("button", { name: "Leave" }).click();
  await expect(page).toHaveURL(/\/settings\/environments$/);
  await expect(page.getByRole("row", { name: /^Minimal/ })).toBeVisible();

  // A new environment from the Data science card takes that build and its tools.
  await page.getByRole("link", { name: "Add environment" }).click();
  const cards = form(page).getByRole("radiogroup", {
    name: "Start from a built-in",
  });
  await cards.getByRole("radio", { name: /^Data science/ }).check({
    force: true,
  });
  await expect(
    cards.getByRole("radio", { name: /^Data science/ }),
  ).toBeChecked();
  // WEB-U-184: a built-in's tools are its own, said once in words, not ticked boxes.
  const tools = form(page).getByRole("region", { name: "Capabilities" });
  await expect(tools).toContainText("Set by Data science:");
  await expect(tools).toContainText("Python");
  await expect(tools).not.toContainText("Browser");
  await expect(tools.getByRole("checkbox")).toHaveCount(0);
});

test("WEB-E2E-031 resets a customised built-in, and an edit customises it again", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  await page.goto("/settings/environments");
  const row = page.getByRole("row", { name: /^Data science/ });
  await expect(row).toContainText("Customised: rebuilds no longer update it");
  await capture(page, "environment-list-customised-1280-light");
  await page
    .getByRole("link", { name: "Edit environment Data science" })
    .click();
  await expect(form(page)).toContainText(
    "Customised: rebuilds no longer update it",
  );
  await axe(page);
  await capture(page, "environment-customised-1280-light", { fullPage: true });
  await form(page).getByRole("button", { name: "Reset to built-in" }).click();
  const ask = page.getByRole("dialog", {
    name: "Reset Data science to built-in?",
  });
  await expect(ask).toContainText("Its size, description and Enabled stay.");
  await axe(page);
  await ask.getByRole("button", { name: "Reset to built-in" }).click();
  await expect(
    page.getByText(
      "Reset Data science to built-in. The gateway checks it next.",
    ),
  ).toBeVisible();
  await expect(row).not.toContainText("Customised");
  // Unedited now: its page says rebuilds update it, and an edit makes it the administrator's again.
  await page
    .getByRole("link", { name: "Edit environment Data science" })
    .click();
  await expect(form(page)).toContainText(
    "Built in: new builds of its image update it.",
  );
  await type(page, "Description", "Notebooks, tables and charts.", form(page));
  await form(page).getByRole("button", { name: "Save" }).click();
  await expect(row).toContainText("Customised: rebuilds no longer update it");
});
