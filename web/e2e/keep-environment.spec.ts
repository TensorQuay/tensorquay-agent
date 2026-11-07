import { createConversation, signIn } from "./session";
import { axe } from "./settings-page";
import { expect, test } from "./test";

// WEB-U-204 on a phone, from the keyboard: the task's ⋯ holds Keep environment on, a checked toggle with what it costs
// under it, inside the screen; chosen with Enter, it pins the task, and the menu reads it on again.
test("WEB-U-204 Keep environment on from the task's ⋯, at 390 by keyboard", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await page.goto(await createConversation(page));
  const more = page.getByRole("button", { name: /^More actions for/ });
  await more.focus();
  await page.keyboard.press("Enter");
  const toggle = page.getByRole("menuitemcheckbox", {
    name: "Keep environment on",
  });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(
    page.getByText(
      "It stays running and counts toward your tasks running at once.",
    ),
  ).toBeVisible();
  const box = await toggle.boundingBox();
  expect(box && box.x >= 0 && box.x + box.width <= 390).toBe(true);
  await axe(page);
  await page.keyboard.press("ArrowDown");
  await expect(toggle).toBeFocused();
  const saved = page.waitForResponse(
    (response) =>
      response.request().method() === "PATCH" &&
      /\/api\/v1\/conversations\/[^/]+$/.test(response.url()),
  );
  await page.keyboard.press("Enter");
  const answer = await saved;
  expect(answer.request().postDataJSON()).toEqual({ pinned: true });
  await expect(more).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await page.reload();
  await page.getByRole("button", { name: /^More actions for/ }).click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
});
