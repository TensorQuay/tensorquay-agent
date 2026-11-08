import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { settled } from "./motion";
import { signIn } from "./session";
import { expect, resetFront, test, type Locator } from "./test";

const box = async (locator: Locator) => {
  const rect = await locator.boundingBox();
  if (!rect) throw new Error("Not on screen");
  return rect;
};

// WEB-E2E-098: a picker whose list is longer than the window (the environment page's own images) keeps its menu inside
// the window, capped by the room on its side of the trigger less 16 px; its list scrolls inside under the Find box,
// which stays at the top, and its last choice can be reached (UI-GUIDELINES §7 Popover).
for (const [width, height] of [
  [1280, 900],
  [390, 844],
] as const)
  test(`WEB-E2E-098 a long picker's menu ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await signIn(page);
    await resetFront(page.request, ["many-images"]);
    await page.goto("/settings/environments/new");
    const trigger = page.getByRole("button", {
      name: "Or pick one of your images: Choose an image",
    });
    await trigger.click();
    const menu = page.getByRole("menu", { name: "Or pick one of your images" });
    await expect(menu.getByRole("menuitemradio")).toHaveCount(28);
    const find = page.getByRole("searchbox");
    await expect(find).toBeVisible();
    await settled(page);
    const surface = await box(menu.locator("xpath=.."));
    const button = await box(trigger);
    // Inside the window, and no taller than the room on its side of the trigger, less 16 px.
    expect(surface.y).toBeGreaterThanOrEqual(0);
    expect(surface.y + surface.height).toBeLessThanOrEqual(height);
    const below = surface.y >= button.y + button.height;
    const room = below
      ? height - (button.y + button.height) - 16
      : button.y - 16;
    expect(surface.height).toBeLessThanOrEqual(room + 0.5);
    // The list scrolls inside the menu, under the Find box; at its end the last choice shows and the box is still at
    // the top.
    const scroller = menu.locator("xpath=..");
    expect(
      await scroller.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
    ).toBe(true);
    const top = (await box(find)).y;
    await scroller.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(menu.getByRole("menuitemradio").last()).toBeInViewport();
    expect((await box(find)).y).toBe(top);
    expect(top).toBeLessThan(surface.y + 16);
    await capture(page, `menu-height-end-${width}`);
    // The accessibility check; the polish audit's control rules read the whole page, whose settings checkboxes are
    // 13 px native boxes on every settings form, a finding of their own and not this menu's.
    expect(await axeViolations(page)).toEqual([]);
  });
