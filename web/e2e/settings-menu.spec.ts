import { signIn } from "./session";
import { expect, test } from "./test";

/**
 * Where the menu's current item sits against the part of the menu the window shows (the menu's box clipped to the
 * window): "in view", or the numbers that say why not.
 */
function placeOfCurrent(menu: Element): string {
  const item = menu.querySelector('[aria-current="page"]');
  if (!item) return "no current item";
  const box = menu.getBoundingClientRect();
  const top = Math.max(box.top, 0);
  const bottom = Math.min(box.bottom, window.innerHeight);
  const place = item.getBoundingClientRect();
  return place.top >= top && place.bottom <= bottom
    ? "in view"
    : `out of view: ${Math.round(place.top)}-${Math.round(place.bottom)} in ${Math.round(top)}-${Math.round(bottom)}`;
}

// WEB-U-460 (17.4 fix): in a window shorter than the Settings menu, the last sections open with their item in the
// menu's view, the administrator's sections included (their items arrive with the session, after the first render),
// and only the menu scrolls to show it: the page stays at its top. A move from inside the app keeps it there too.
test("WEB-U-460 a short window shows the current section in the menu and leaves the page at its top", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 560 });
  await signIn(page);
  const menu = page.getByRole("navigation", { name: "Settings sections" });
  const pageTop = () =>
    page.evaluate(
      () =>
        (document.querySelector(".app-content")?.scrollTop ?? 0) +
        window.scrollY,
    );
  for (const [href, name] of [
    ["/settings/usage", "Usage"],
    ["/settings/status", "Status"],
  ] as const)
    await test.step(`${name}, loaded cold`, async () => {
      await page.goto(href);
      await expect(menu.getByRole("link", { name })).toHaveAttribute(
        "aria-current",
        "page",
      );
      await expect.poll(() => menu.evaluate(placeOfCurrent)).toBe("in view");
      expect(await pageTop()).toBe(0);
    });
  await test.step("Limits, from inside the app", async () => {
    await page.goto("/settings/account");
    await expect(menu.getByRole("link", { name: "Account" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    // A script's click scrolls nothing itself, so whatever moves after it is the page's own doing.
    await menu
      .getByRole("link", { name: "Limits" })
      .evaluate((link) => (link as HTMLElement).click());
    await expect(menu.getByRole("link", { name: "Limits" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect.poll(() => menu.evaluate(placeOfCurrent)).toBe("in view");
    expect(await pageTop()).toBe(0);
  });
});
