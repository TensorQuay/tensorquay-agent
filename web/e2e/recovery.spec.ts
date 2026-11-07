import { createConversation, signIn } from "./session";
import { axe } from "./settings-page";
import { expect, test, type Page } from "./test";

// A sandbox that keeps stopping (WEB-U-068): the fake front sends session.ready with SandboxSummary.recovery, as the
// gateway's worker does on each loss. The run status line counts down to each replacement, says when there will be
// none, and a message brings the task back; each state is announced once and passes axe in both themes.

/** Only the fake front exposes this: a loss as the worker reports it. */
async function lose(
  page: Page,
  conversation: string,
  state: "destroyed" | "creating" | "failed",
  losses: number,
  retry: number | null,
) {
  const response = await page.request.post(
    `/api/v1/conversations/${conversation}/_fixture/recovery`,
    { data: { state, losses, retry_in_secs: retry } },
  );
  expect(response.status()).toBe(200);
}

for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-U-068 sandbox recovery on the task page 1024 ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    const path = await createConversation(page);
    const conversation = path.split("/").at(-1) ?? "";
    await page.goto(path);
    const status = page.getByLabel("Run status");
    const line = page.locator(".environment-status");
    const announced = status.getByRole("status");
    await expect(page.getByLabel("Message", { exact: true })).toBeVisible();

    // The first loss: a short countdown, announced once as the notice gave it.
    await lose(page, conversation, "destroyed", 1, 3);
    // It opens at the notice's 3 s and counts down (the exact second depends on when the page is read).
    await expect(line).toHaveText(
      /^Environment stopped once; restarting in [1-3] s$/,
    );
    await expect(announced).toContainText(
      "Environment stopped once; restarting in 3 s",
    );
    await expect(line).toHaveText("Environment stopped once; restarting now");
    await expect(announced).toContainText("restarting in 3 s");
    await axe(page);

    // The second loss, with the replacement starting.
    await lose(page, conversation, "creating", 2, 15);
    await expect(status).toContainText("Starting…");
    // It opens at the notice's 15 s and counts down; a page slow to show the loss may first read 14 s or less.
    await expect(line).toHaveText(
      /^Environment stopped 2 times; restarting in (?:[1-9]|1[0-5]) s$/,
    );

    // The fifth: the gateway stops replacing it until the owner acts.
    await lose(page, conversation, "failed", 5, null);
    const stopped =
      "Environment stopped 5 times; it won't restart by itself. Send a message to try again.";
    await expect(line).toHaveText(stopped);
    await expect(announced).toContainText(stopped);
    await axe(page);

    // A message brings the task back, and the recovery goes.
    await page.getByLabel("Message", { exact: true }).fill("Carry on");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(status).toHaveText(/^Done/, { timeout: 30_000 });
    await expect(page.getByText(/Environment stopped/)).toHaveCount(0);
  });
