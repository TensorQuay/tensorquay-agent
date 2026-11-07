import { signIn } from "./session";
import { expect, resetFront, test } from "./test";

// WEB-U-321/322 on the fake front (15c's known issue): two routes an administrator named "Qwen", the first by name
// failing its check. New task starts on the one that passed; each reads apart in the menu and the button; only the
// failing one's row says its check didn't pass, and a person may still pick it.
test.beforeEach(async ({ request }) => {
  await resetFront(request, ["same-name-routes"]);
});

test("WEB-U-321 New task tells same-name routes apart and starts on the one that passed its check", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const model = page.getByRole("button", { name: "Model: Qwen · Qwen3 8b" });
  await expect(model).toBeVisible();
  await expect(model).not.toContainText("check");
  await model.click();
  const failing = page.getByRole("menuitemradio", { name: /^Qwen · Qwen3 4b/ });
  const passing = page.getByRole("menuitemradio", { name: /^Qwen · Qwen3 8b/ });
  await expect(failing).toContainText("Its last check didn't pass");
  await expect(passing).not.toContainText("Its last check didn't pass");
  await expect(passing).toHaveAttribute("aria-checked", "true");
  await failing.click();
  await expect(
    page.getByRole("button", { name: "Model: Qwen · Qwen3 4b" }),
  ).toBeVisible();
});
