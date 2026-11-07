import { expect, test, type Page } from "./test";

// WEB-U-383: the password's show/hide button sits inside the password box whatever is said under the field: the
// server's refusal (WEB-U-380) and the form's own "Enter your password." both put a line below the input, and the
// button must not follow it (the 16h retake, P01 wrong password: it dropped onto the error line).
async function expectEyeInBox(page: Page) {
  const input = await page
    .getByLabel("Password", { exact: true })
    .boundingBox();
  const eye = await page
    .getByRole("button", { name: "Show password" })
    .boundingBox();
  expect(input && eye).toBeTruthy();
  if (!input || !eye) return;
  expect(eye.y).toBeGreaterThanOrEqual(input.y - 0.5);
  expect(eye.y + eye.height).toBeLessThanOrEqual(input.y + input.height + 0.5);
  expect(eye.x).toBeGreaterThanOrEqual(input.x);
  expect(eye.x + eye.width).toBeLessThanOrEqual(input.x + input.width + 0.5);
}

for (const [width, height] of [
  [1280, 900],
  [390, 844],
] as const) {
  test(`WEB-U-383 the password's eye stays in its box under a field error at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await page.goto("/login");
    await page
      .getByLabel("Email", { exact: true })
      .fill("member@example.invalid");
    await expectEyeInBox(page);
    // The server's refusal, said on the password.
    await page.getByLabel("Password", { exact: true }).fill("wrong");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByRole("main").getByRole("alert")).toContainText(
      "Email or password is incorrect",
    );
    await expectEyeInBox(page);
    // The form's own check, before anything is sent.
    await page.getByLabel("Password", { exact: true }).fill("");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByRole("main").getByRole("alert")).toHaveText(
      "Enter your password.",
    );
    await expectEyeInBox(page);
  });
}
