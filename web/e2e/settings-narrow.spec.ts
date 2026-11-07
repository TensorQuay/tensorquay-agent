import { signIn } from "./session";
import { expect, resetFront, test } from "./test";

// WEB-U-356 (the 16f page pass, E3): below 768 px a settings section's intro and its main button stack, the intro over
// the button, so the intro keeps the column's width instead of a word or two a line beside the button.
for (const [path, intro] of [
  ["/settings/keys", "For a connector your administrator set up"],
  ["/settings/keys/shared", "Keys the gateway uses to reach model providers"],
] as const)
  test(`WEB-U-356 ${path}: the intro over its button at 390`, async ({
    page,
    request,
  }) => {
    await resetFront(request, ["shared-host-keys"]);
    await signIn(page);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(path);
    const text = page.getByText(intro, { exact: false }).first();
    await expect(text).toBeVisible();
    const bar = text.locator(
      "xpath=ancestor::div[contains(@class, 'settings-bar')][1]",
    );
    const button = bar.getByRole("button").first();
    const [paragraph, action, column] = await Promise.all([
      text.boundingBox(),
      button.boundingBox(),
      bar.boundingBox(),
    ]);
    if (!paragraph || !action || !column)
      throw new Error("The section's bar must be visible");
    // The intro takes the column's width, and the button comes under it.
    expect(paragraph.width).toBeGreaterThan(column.width * 0.9);
    expect(action.y).toBeGreaterThanOrEqual(paragraph.y + paragraph.height - 1);
  });
