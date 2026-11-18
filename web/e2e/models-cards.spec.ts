import { signIn } from "./session";
import { capture } from "./settings-page";
import { expect, test } from "./test";

// WEB-E2E-136 (18.1, the 17.2 review's finding 18, decided on 18 Nov 2026): on a phone the Models list's cards open
// their route on a tap anywhere on the card, with no Edit button of their own; the ⋯ stays for the rest. The card is one
// focus target for opening (its ring drawn around the card), then its ⋯. The table above 600 px keeps its Edit.

test.use({ hasTouch: true });

for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-136 a Models card opens its route on a tap (390 ${colorScheme})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await signIn(page);
    await page.goto("/settings/models");
    const open = page.getByRole("link", { name: "Edit route DeepSeek Flash" });
    await expect(open).toBeAttached();
    const card = page.locator("tr", { has: open });
    await expect(page.locator(".data-table-scroll[data-cards]")).toHaveCount(1);
    // No Edit word on a card: the card is the way in.
    // (Its word is still in the link's name, off the screen: a box of 1 × 1 px at most, as WEB-E2E-135 measures it.)
    const word = await card.getByText("Edit", { exact: true }).boundingBox();
    expect(word === null || (word.width <= 1 && word.height <= 1)).toBe(true);
    await expect(
      card.locator('a[href="/settings/models/deepseek-flash"]'),
    ).toHaveCount(1);
    await capture(page, `models-cards-390-${colorScheme}`, { fullPage: true });

    // ⋯ opens its menu, not the route.
    await card
      .getByRole("button", { name: "More actions for route DeepSeek Flash" })
      .tap();
    await expect(page.getByRole("menuitem", { name: /^Check/ })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(/\/settings\/models$/);

    // A tap on the card's body (its model's name) opens the route.
    // At the screen point, as a finger does: the card's link lies over its text.
    const body = (await card
      .getByText("deepseek-flash", { exact: true })
      .boundingBox())!;
    await page.touchscreen.tap(
      body.x + body.width / 2,
      body.y + body.height / 2,
    );
    await expect(page).toHaveURL(/\/settings\/models\/deepseek-flash$/);
    await page.goBack();

    // The keyboard: the card is one stop to open it, its ring around the card; then its ⋯.
    await expect(open).toBeAttached();
    await open.focus();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Tab");
    await expect(open).toBeFocused();
    const ring = await card.evaluate(
      (row) => getComputedStyle(row).outlineStyle,
    );
    expect(ring).not.toBe("none");
    await page.keyboard.press("Tab");
    await expect(
      card.getByRole("button", {
        name: "More actions for route DeepSeek Flash",
      }),
    ).toBeFocused();
  });

test("WEB-E2E-136 the Models table keeps its Edit button (1280)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  await page.goto("/settings/models");
  const open = page.getByRole("link", { name: "Edit route DeepSeek Flash" });
  await expect(open).toBeVisible();
  await expect(open).toHaveText("Edit");
});
