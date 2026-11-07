import { HOLD } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-038 (GW-I-229, WEB-U-292; first-hour F7's follow-up): Settings › Status counts the tasks working on a step, so
// an administrator restarts in a gap. Two tasks held mid-step read "2 tasks are working on a step"; once both runs have
// ended, Check again reads the quiet line. A count only: no task's name reaches the section.

/** Starts a run that holds after its narration, mid-step, on a new task; its id. */
async function holdMidStep(page: Page): Promise<string> {
  const path = await createConversation(page);
  await page.goto(path);
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill(HOLD);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  await expect(page.getByText("I’ll check the project files.")).toBeVisible();
  return path.split("/").at(-1) ?? "";
}

test("WEB-E2E-038 Status says how many tasks are working on a step, and when none is", async ({
  page,
}) => {
  await signIn(page);
  const tasks = [await holdMidStep(page), await holdMidStep(page)];

  await page.goto("/settings/status");
  const before = page.getByRole("region", { name: "Before a restart" });
  await expect(before).toContainText(
    "2 tasks are working on a step. Restarting now would start their steps over: wait until none is, or tell people first.",
  );
  await expect(before).not.toContainText("Workspace review");

  for (const task of tasks)
    expect(
      (
        await page.request.post(`/api/v1/conversations/${task}/fixture-resume`)
      ).status(),
    ).toBe(204);
  // Each run ends a few seconds after it resumes; Check again reads the count anew until both have.
  await expect(async () => {
    await page.getByRole("button", { name: "Check again" }).click();
    await expect(before).toContainText(
      "No task is working on a step, so restarting now wouldn't start any step over.",
      { timeout: 1000 },
    );
  }).toPass({ timeout: 30_000 });
});
