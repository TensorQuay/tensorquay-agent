import { HOLD } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// GW-I-129: a message sent while the agent works is held by the gateway, listed in the composer, and sent
// as the next turn when the run ends; a stopped run drops it, to send again or dismiss.
async function working(page: Page, text: string) {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill(text);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  return input;
}
const waiting = (page: Page) =>
  page.getByRole("list", { name: "Messages waiting for the agent" });

test("WEB-U-079 a message sent while the agent works goes out when the run ends", async ({
  page,
}) => {
  const input = await working(page, HOLD);
  // The fake run pauses after its narration; a resume before that pause would be lost (as in conversation.spec).
  await expect(page.getByText("I’ll check the project files.")).toBeVisible();
  await input.fill("Also list the tests.");
  await page.getByRole("button", { name: "Send after this" }).click();
  await expect(waiting(page)).toHaveText(
    /Sends when the agent finishes: Also list the tests\./,
  );
  await expect(input).toHaveValue("");
  const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  expect(
    (
      await page.request.post(`/api/v1/conversations/${task}/fixture-resume`)
    ).status(),
  ).toBe(204);
  await expect(
    page.getByRole("article", { name: "Your message" }).last(),
  ).toHaveText("Also list the tests.");
  await expect(waiting(page)).toHaveCount(0);
});

test("WEB-U-079 a stopped run drops a held message, which can be dismissed", async ({
  page,
}) => {
  const input = await working(page, "Wait for cancellation");
  await input.fill("Then update the README.");
  await input.press("Enter");
  await expect(waiting(page)).toBeVisible();
  // Stop acts at once (c9): the work so far is kept, so there is nothing to confirm.
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(waiting(page)).toHaveText(
    /Not sent, because the run was stopped: Then update the README\./,
  );
  await expect(
    waiting(page).getByRole("button", { name: "Send again" }),
  ).toBeVisible();
  await waiting(page).getByRole("button", { name: "Dismiss" }).click();
  await expect(waiting(page)).toHaveCount(0);
});

test("WEB-U-079 a held message can be cancelled before the run ends", async ({
  page,
}) => {
  const input = await working(page, "Wait for cancellation");
  await input.fill("Never mind this one.");
  await page.getByRole("button", { name: "Send after this" }).click();
  await waiting(page).getByRole("button", { name: "Cancel" }).click();
  await expect(waiting(page)).toHaveCount(0);
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
});
