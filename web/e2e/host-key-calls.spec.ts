import { createConversation, signIn } from "./session";
import { axe } from "./settings-page";
import { expect, test } from "./test";

// A task's keyed calls in words (ADR-0037 §8): the gateway's live notices land on the step that ran them, each refusal
// a sentence naming the key and the address, never its code; a key the task used that isn't set up offers the Add page
// with its address.
test("WEB-U-148 a task's keyed calls on its step, in words", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Call keyed services");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  const main = page.getByRole("main");
  // A finished run's steps are folded; the lines are on the step that ran the command.
  await main.locator(".activity-heading").last().click();
  for (const words of [
    "Called api.github.com with GITHUB_WORK",
    "GITHUB_WORK wasn't sent to api.github.com: the run had already ended.",
    "GITHUB_WORK wasn't sent to api.github.com: the key store was unavailable. Try again in a moment.",
    "The task used a key for git.acme.lan:8443 that isn't set up.",
  ])
    await expect(main.getByText(words, { exact: true })).toBeVisible();
  // Never a code: "unavailable" is a word of its own sentence, so only the others are looked for.
  await expect(main).not.toContainText(/\b(expired|absent|host_key)\b/);
  const add = main.getByRole("link", {
    name: "Add a key for git.acme.lan:8443",
  });
  await expect(add).toHaveAttribute(
    "href",
    `/settings/keys/new?address=${encodeURIComponent("git.acme.lan:8443")}`,
  );
  await axe(page);
  await add.click();
  await expect(page.getByLabel("Web address", { exact: true })).toHaveValue(
    "git.acme.lan:8443",
  );
});

// ADR-0037: a page opened later reads the task's key uses, as the gateway stored them; a key that wasn't set up made
// none, and a deleted key's rows go with it. Each line once, on its step or under "During this run", wherever the
// clocks allow.
test("WEB-U-193 a task's keyed calls after a reload, once each", async ({
  page,
}) => {
  await signIn(page);
  // The key the task's calls use, as a person would have set it up.
  const added = await page.request.post("/api/v1/host-keys", {
    data: {
      name: "GITHUB_WORK",
      address: "https://api.github.com",
      scheme: "bearer",
      value: "fixture-not-a-secret",
    },
  });
  expect(added.status()).toBe(201);
  const { id, version } = (await added.json()) as {
    id: string;
    version: number;
  };
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Call keyed services");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  const main = page.getByRole("main");
  await expect(
    main.getByText(
      "The task used a key for git.acme.lan:8443 that isn't set up.",
      { exact: true },
    ),
  ).toBeAttached();
  const lines = [
    "Called api.github.com with GITHUB_WORK",
    "GITHUB_WORK wasn't sent to api.github.com: the run had already ended.",
    "GITHUB_WORK wasn't sent to api.github.com: the key store was unavailable. Try again in a moment.",
  ];
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await main.locator(".activity-heading").last().click();
  for (const words of lines)
    await expect(main.getByText(words, { exact: true })).toHaveCount(1);
  await expect(main.getByText(/isn't set up/)).toHaveCount(0);
  await axe(page);
  // Deleted, the key takes its uses with it.
  expect(
    (
      await page.request.delete(`/api/v1/host-keys/${id}?version=${version}`)
    ).status(),
  ).toBe(204);
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await main.locator(".activity-heading").last().click();
  for (const words of lines)
    await expect(main.getByText(words, { exact: true })).toHaveCount(0);
});
