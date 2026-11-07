import { expect, test } from "./test";

test("root page has the application title", async ({ page }) => {
  await page.goto("/");

  await expect(page).toHaveTitle(/TensorQuay Agent/);
});
