import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-093 on the page: a task's decision engine that stops answering never fails the run; the agent goes on deciding
// by itself, and the page says so once for the run, in plain words, with no code. The fake model's engine fails twice
// ("Decide without the engine", e2e/front/stream.mjs).
test("WEB-U-093 the decision engine's fallback is said once per run", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Decide without the engine");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  const line = page.locator(".transcript-notice", {
    hasText: "deciding on its own",
  });
  await expect(line).toHaveText(
    "The decision engine couldn't be reached, so the agent is deciding on its own for this run.",
  );
  await expect(line).toHaveCount(1);
  const shown = await page.locator("main").innerText();
  expect(shown).not.toMatch(/unreachable|timeout|decision\.fallback/);
});
