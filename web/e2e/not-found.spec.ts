import { axeViolations } from "./a11y";
import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-E2E-088 (a pass over the pages before 17.0): an address with no page shows the app's own 404, in the
// reader's language and theme, with a way to the tasks (before: Next.js's default, always light and English, a dead
// end); a channel that doesn't exist says so under the page's bar and leads back to the channels.

for (const width of [1280, 390])
  test(`WEB-E2E-088 an unknown address in Chinese and dark, at ${width}, leads to the tasks`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
    // The language and theme menus' stored choices, as a person leaves them.
    await page.addInitScript(() => {
      localStorage.setItem("tensorquay-language", "zh-Hans");
      localStorage.setItem("tensorquay-theme", "dark");
    });
    await signIn(page);
    const answer = await page.goto("/no-such-page");
    expect(answer?.status()).toBe(404);
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-Hans");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(
      page.getByRole("heading", { level: 1, name: "找不到页面" }),
    ).toBeVisible();
    await expect(page).toHaveTitle("找不到页面 · TensorQuay Agent");
    // The dark theme's paper, not the default page's white.
    const background = await page.evaluate(
      () => getComputedStyle(document.body).backgroundColor,
    );
    expect(background).toBe("rgb(18, 18, 17)");
    // No horizontal scroll at 390.
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    expect(await axeViolations(page)).toEqual([]);
    const home = page.getByRole("link", { name: "前往你的任务" });
    await home.focus();
    await expect(home).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/$/);
  });

test("WEB-E2E-088 a channel that doesn't exist says so and leads back to the channels", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await signIn(page);
  await page.goto("/channels/0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b");
  await expect(
    page.getByRole("heading", { level: 1, name: "Channels" }),
  ).toBeVisible();
  await expect(
    page.getByRole("alert").filter({
      hasText: "This channel doesn't exist, or you can't see it.",
    }),
  ).toBeVisible();
  expect(await axeViolations(page)).toEqual([]);
  await page.getByRole("link", { name: "Go to channels" }).click();
  await expect(page).toHaveURL(/\/channels$/);
});
