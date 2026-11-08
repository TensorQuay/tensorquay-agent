import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-094 on the page: a failed push says what happened and what to do under its step, in plain words, never its
// code (the fake front's "Fail the save with <code>", e2e/front/stream.mjs). key_use_unrecorded is GW-I-149's.
for (const [code, words] of [
  [
    "key_use_unrecorded",
    "The changes weren't saved: the key's use couldn't be recorded. Try again in a moment.",
  ],
  ["git_push_rejected", /^The project didn't accept the changes/],
] as const)
  test(`WEB-U-094 a push that failed with ${code} says so plainly`, async ({
    page,
  }) => {
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByLabel("Message", { exact: true });
    await input.fill(`Fail the save with ${code}`);
    await input.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText(/^(Done|Failed)/);
    const group = page.getByRole("button", { name: /Worked for/ }).first();
    if ((await group.getAttribute("aria-expanded")) === "false")
      await group.click();
    const hint = page.locator(".activity-next").first();
    await expect(hint).toHaveText(words);
    expect(await page.locator("main").innerText()).not.toContain(`${code}:`);
  });
