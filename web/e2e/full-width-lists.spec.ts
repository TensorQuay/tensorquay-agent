import { signIn } from "./session";
import { axe, capture } from "./settings-page";
import { expect, test, type Locator, type Page } from "./test";

// WEB-U-131 (UI-GUIDELINES "Long lists use the full width"): a list of choices spans the whole content width, never one
// narrow column beside an empty label column; a profile's Tools are a grid of group cards, about three to a row at 1280
// and one on a phone, with All above. The page is one column: each field's label above it, fields at most about 640 px
// wide. At 1280 and 390 in both themes.

async function box(locator: Locator) {
  const found = await locator.boundingBox();
  if (!found) throw new Error("The element must be visible");
  return found;
}

/** The list starts at the section's left edge (its heading's), not in the fields column. */
async function fromTheLeft(section: Locator, item: Locator) {
  const heading = await box(section.getByRole("heading"));
  expect(
    Math.abs((await box(item)).x - heading.x),
    "the list starts at the section's edge",
  ).toBeLessThan(2);
}

/** How many of these sit on the first row: about three at 1280, one on a phone. */
async function firstRow(items: Locator) {
  const tops = await Promise.all(
    (await items.all()).map(async (item) => (await box(item)).y),
  );
  return tops.filter((top) => Math.abs(top - (tops[0] ?? 0)) < 2).length;
}

const form = (page: Page) => page.locator("form.settings-form-page");

/**
 * One column, as ChatGPT's and Claude's settings: a field's label above it at the section's left edge, never an empty
 * label column beside it; a field is at most about 640 px wide, a phone's whole width.
 */
async function oneColumn(page: Page, width: number) {
  const label = await box(
    form(page).getByText("Display name", { exact: true }),
  );
  const field = await box(form(page).getByLabel("Display name"));
  const heading = await box(
    form(page).getByRole("region").first().getByRole("heading"),
  );
  expect(Math.abs(label.x - heading.x), "the label at the edge").toBeLessThan(
    2,
  );
  expect(Math.abs(field.x - label.x), "the field under its label").toBeLessThan(
    2,
  );
  expect(field.y).toBeGreaterThan(label.y);
  if (width === 1280) expect(field.width).toBeLessThanOrEqual(641);
}

/** No page-wide sideways scroll at this width. */
async function noSideways(page: Page) {
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth,
    ),
  ).toBeLessThanOrEqual(0);
}

for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const) {
    test(`WEB-U-131 a profile's long lists use the full width ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/agent-profiles/new");
      const tools = page.getByRole("region", { name: "Tools" });
      const cards = tools.getByRole("group");
      await expect(cards).toHaveCount(7);
      const first = await box(cards.first());
      await fromTheLeft(tools, cards.first());
      // About three to a row at 1280, one on a phone.
      expect(await firstRow(cards)).toBe(width === 1280 ? 3 : 1);
      // The row reaches the section's right edge: the whole content width.
      const section = await box(tools);
      const row = await box(cards.nth(width === 1280 ? 2 : 0));
      expect(
        Math.abs(row.x + row.width - (section.x + section.width)),
      ).toBeLessThan(2);
      // All sits above the grid, at its left edge.
      const all = await box(
        tools.getByRole("checkbox", { name: "All", exact: true }).locator(".."),
      );
      expect(all.y + all.height).toBeLessThanOrEqual(first.y);
      expect(Math.abs(all.x - first.x)).toBeLessThan(2);
      // Each tool is one line, its description a muted line under it. One line of a check row is its control's 28 px
      // target (the controls standard, WEB-E2E-101), so the row is measured, at most that high.
      const shell = tools.getByRole("checkbox", { name: "Run commands" });
      const name = await box(shell.locator("xpath=ancestor::label[1]"));
      expect(name.height).toBeLessThanOrEqual(28);
      const hint = await box(
        tools.getByText("Run commands in the task's computer"),
      );
      expect(hint.y).toBeGreaterThanOrEqual(name.y + name.height - 1);
      await oneColumn(page, width);
      await fromTheLeft(
        page.getByRole("region", { name: "Connectors" }),
        page
          .getByRole("region", { name: "Connectors" })
          .locator("label, p")
          .first(),
      );
      await noSideways(page);
      await axe(page);
      await capture(page, `full-width-profile-${width}-${colorScheme}`);
      // The settings column scrolls on its own: the tools, then the connectors, brought into view for the review.
      await tools.scrollIntoViewIfNeeded();
      await capture(page, `full-width-profile-tools-${width}-${colorScheme}`);
      await page
        .getByRole("region", { name: "Connectors" })
        .scrollIntoViewIfNeeded();
      await capture(
        page,
        `full-width-profile-connectors-${width}-${colorScheme}`,
      );
    });

    test(`WEB-U-131 an environment's capabilities use the full width ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/environments/new");
      const capabilities = page.getByRole("region", { name: "Capabilities" });
      await oneColumn(page, width);
      const image = page.getByRole("region", { name: "Image" });
      await fromTheLeft(
        image,
        image.getByRole("radiogroup", { name: "Start from a built-in" }),
      );
      // The boxes are your own image's choice (WEB-U-184): a built-in's tools are one line.
      await expect(capabilities).toContainText("Set by ");
      await image
        .getByRole("button", { name: /^Or pick one of your images: / })
        .click();
      await page
        .getByRole("menuitemradio", { name: /team\/notebooks:1/ })
        .click();
      const items = capabilities.getByRole("checkbox").locator("..");
      await fromTheLeft(capabilities, items.first());
      expect(await firstRow(items)).toBe(width === 1280 ? 3 : 1);
      await noSideways(page);
      await axe(page);
      await capture(page, `full-width-environment-${width}-${colorScheme}`);
      await capabilities.scrollIntoViewIfNeeded();
      await capture(
        page,
        `full-width-environment-capabilities-${width}-${colorScheme}`,
      );
    });

    test(`WEB-U-131 a model route's page is one column ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/models/new");
      await expect(
        page.getByRole("heading", { level: 2, name: "Add a route" }),
      ).toBeVisible();
      await oneColumn(page, width);
      // Where the model runs: its options in the normal field rhythm, one control's height apart, 8 px under the
      // question, not a field's gap between each.
      const where = page.getByRole("radiogroup", {
        name: "Where does the model run?",
      });
      const options = await Promise.all(
        (await where.getByRole("radio").all()).map((radio) =>
          box(radio.locator("..")),
        ),
      );
      expect(options).toHaveLength(3);
      for (const [index, option] of options.entries())
        if (index > 0)
          expect(
            option.y - (options[index - 1]?.y ?? 0),
            "one control's height apart",
          ).toBeLessThanOrEqual(29);
      const question = await box(where.getByText("Where does the model run?"));
      expect(
        (options[0]?.y ?? 0) - (question.y + question.height),
      ).toBeLessThanOrEqual(9);
      await noSideways(page);
      await axe(page);
      await capture(page, `full-width-route-${width}-${colorScheme}`);
    });
  }
