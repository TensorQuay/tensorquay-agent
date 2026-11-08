import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test } from "./test";

const PAGES = [
  "/settings/agent-profiles",
  "/settings/people",
  "/settings/models",
  "/settings/environments",
  "/settings/connectors",
  "/settings/decision-engines",
  "/settings/internet-access",
  "/settings/status",
] as const;
/** The words a passed check says: the only ones in green (UI-GUIDELINES: green, amber and red for a stated meaning). */
const VERIFIED = new Set(["Verified", "Ready", "Available"]);

// WEB-E2E-109 (17.3): "Default" is a muted pill, and green is only for a check that passed; a plain state (on, active,
// in new tasks) reads in the muted tone, its word saying it.
test("WEB-E2E-109 green only for a passed check, Default a muted pill", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  for (const href of PAGES)
    await test.step(href, async () => {
      await page.goto(href);
      await expect(page.locator(".settings-column > h2")).toBeVisible();
      await page.waitForLoadState("networkidle");
      const green = await page
        .locator('.status-pill[data-tone="ok"]')
        .allTextContents();
      for (const words of green)
        expect
          .soft(VERIFIED.has(words.trim()), `${href}: "${words}" in green`)
          .toBe(true);
      await capture(page, `pill-tones-${href.split("/").at(-1)}`);
    });
  await test.step("Default, a muted pill", async () => {
    await page.goto("/settings/environments");
    await expect(
      page
        .locator(".status-pill")
        .filter({ hasText: /^Default$/ })
        .first(),
    ).toHaveAttribute("data-tone", "muted");
  });
});
