import { randomUUID } from "node:crypto";

import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

/** Enough workspaces for the page's search, filter and sort (eight or more, UI-GUIDELINES §7 List tools). */
async function manyWorkspaces(page: Page) {
  for (let index = 1; index <= 8; index += 1)
    expect(
      (
        await page.request.post("/api/v1/workspaces", {
          headers: { "Idempotency-Key": randomUUID() },
          data: { name: `Quarter ${index}` },
        })
      ).status(),
    ).toBe(201);
}

// WEB-E2E-110 (17.3): the Workspaces filter is the light segmented control where there is room, and the standard
// picker at phone width, never a row of labels to scroll sideways.
for (const [width, shown] of [
  [1280, "segmented"],
  [390, "picker"],
] as const)
  test(`WEB-E2E-110 the workspace filter at ${width}: the ${shown}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await signIn(page);
    await manyWorkspaces(page);
    await page.goto("/workspaces");
    const group = page.getByRole("group", { name: "Show" });
    const picker = page.getByRole("button", { name: /^Show: / });
    if (shown === "segmented") {
      await expect(group).toBeVisible();
      await expect(picker).toBeHidden();
    } else {
      await expect(picker).toBeVisible();
      await expect(group).toBeHidden();
      await expect(picker).toHaveAccessibleName("Show: All");
      await picker.click();
      await page
        .getByRole("menuitemradio", { name: /^Shared with me/ })
        .click();
      await expect(page).toHaveURL(/filter=shared_with_me/);
      await expect(picker).toHaveAccessibleName("Show: Shared with me");
    }
    // Nothing on the page scrolls sideways.
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await capture(page, `workspace-filter-${width}`);
  });
