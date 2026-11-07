import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { chooseModel } from "./model";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";
import { followUp, followingBottom } from "./ux/follow-up";
import { observations, observe } from "./ux/observer";

async function capture(page: Page, name: string) {
  const folder = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!folder) return;
  await mkdir(folder, { recursive: true });
  await page.screenshot({ path: join(folder, `${name}.png`) });
}

test("WEB-E2E-018 follows a second message after reload, rename and model selection", async ({
  page,
}) => {
  await observe(page);
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await expect(input).toBeEnabled();
  await input.fill("Stream Markdown before replay");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 20_000,
  });
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  await followingBottom(page);
  await page.locator(".conversation-bar h1").click();
  const title = page.getByRole("textbox", { name: "Task title" });
  await title.fill("Continued review");
  await title.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Continued review" }),
  ).toBeVisible();
  await chooseModel(page, "second");

  let release: (() => void) | undefined;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/messages", async (route) => {
    await held;
    await route.continue();
  });
  try {
    await followUp(page, "Stream Markdown after replay");
    await expect(
      page.getByRole("button", { name: "Sending", exact: true }),
    ).toBeDisabled();
    await followingBottom(page);
    await capture(page, "follow-up-pending");
  } finally {
    release?.();
  }
  await expect(page.getByLabel("Run status")).toHaveText(/^Working/);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 20_000,
  });
  await followingBottom(page);
  await observations(page);
  await capture(page, "follow-up-completed");
});
