import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { afterStart, budget, START_MS } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

/** A named step in the run's log (the list reporter prints steps): the log shows every part the test reached. */
const step = (title: string) => test.step(title, () => undefined);

// The feature matrix (G6, and G1's copy and follow): the task page on the real stack, with the scripted model in
// deploy/tests/fixtures/task-page.json. One reply is held until Stop (wait_ms), so the run stays active while a
// message waits; another streams a long answer slowly, so the page can be scrolled away from it and brought back.
test.skip(
  process.env.TQA_STACK_SUITE !== "task-page",
  "Requires the task-page stack suite",
);
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

/** A new task, opened; the fixture picks its replies by a phrase in its first message. */
async function task(page: Page, baseURL: string, title: string) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title, model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  return id;
}
const waiting = (page: Page) =>
  page.getByRole("list", { name: "Messages waiting for the agent" });

const results: Record<string, unknown> = {};
test.afterAll(async () => {
  if (process.env.TQA_STACK_RESULT)
    await writeFile(process.env.TQA_STACK_RESULT, JSON.stringify(results), {
      mode: 0o600,
    });
});

test("WEB-U-079 on the real stack: Stop ends a run, a message sent meanwhile waits, is cancelled or dropped", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(1, 90_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await signIn(page);
  const id = await task(page, baseURL, "Held reply");
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Hold this reply until I stop it.");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/, {
    timeout: START_MS,
  });

  // T5: a message sent while it works waits, and can be taken back.
  await step("G6 T5: a message sent while it works waits, and is cancelled");
  await input.fill("Never mind this one.");
  await page.getByRole("button", { name: "Send after this" }).click();
  await expect(waiting(page)).toHaveText(
    /Sends when the agent finishes: Never mind this one\./,
  );
  await waiting(page).getByRole("button", { name: "Cancel" }).click();
  await expect(waiting(page)).toHaveCount(0);
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);

  // T3: Stop ends the held run; the message still waiting is dropped, to send again or dismiss.
  await step("G6 T3: Stop ends the held run and drops the waiting message");
  await input.fill("Then summarise it.");
  await input.press("Enter");
  await expect(waiting(page)).toHaveText(
    /Sends when the agent finishes: Then summarise it\./,
  );
  // Stop acts at once (c9): the work so far is kept, so there is nothing to confirm.
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(waiting(page)).toHaveText(
    /Not sent, because the run was stopped: Then summarise it\./,
  );
  await expect(
    waiting(page).getByRole("button", { name: "Send again" }),
  ).toBeVisible();
  await waiting(page).getByRole("button", { name: "Dismiss" }).click();
  await expect(waiting(page)).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeVisible();

  // The gateway agrees: the run was cancelled (run_status is on the task list's items only), and only the first
  // message was ever delivered.
  await step("G6: the gateway reads the run as cancelled");
  const listed = await page.request.get("/api/v1/conversations");
  const { items } = (await listed.json()) as {
    items: { id: string; run_status?: string }[];
  };
  const conversation = items.find((item) => item.id === id);
  expect(conversation?.run_status).toBe("cancelled");
  await page.reload();
  await expect(page.getByRole("article", { name: "Your message" })).toHaveText([
    "Hold this reply until I stop it.",
  ]);
  results.held = { id, run_status: conversation?.run_status };
});

test("WEB-U-014/015 on the real stack: a long answer streams, its code copies, and Jump to latest brings it back", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(1, 90_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await signIn(page);
  const id = await task(page, baseURL, "Long answer");
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Write a long answer with code.");
  await input.press("Enter");

  // T10: scrolled away while it streams, the page stops following: it offers Jump to latest and stays where the
  // reader is when the answer ends. A third of the way in, the answer is taller than the transcript, so there is
  // somewhere to scroll to. (How many new lines the button counts depends on how the stack batches the stream; the
  // fake front checks the count.)
  await step(
    "G1 T10: scrolled away, the page stops following and offers Jump to latest",
  );
  await expect(
    page.getByRole("heading", { name: "Section 8", exact: true }),
  ).toBeAttached({ timeout: afterStart(30_000) });
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  await region.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByRole("button", { name: /Jump to latest/ }),
  ).toBeVisible();
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: 60_000,
  });
  expect(await region.evaluate((element) => element.scrollTop)).toBe(0);

  // T9: the code block copies exactly, over plain HTTP.
  await step("G1 T9: the code block copies exactly");
  const code = page.locator(".markdown-code").first();
  await code.scrollIntoViewIfNeeded();
  await code.getByRole("button", { name: "Copy code" }).click();
  await expect(code.getByRole("button", { name: "Copied" })).toBeVisible();
  await input.focus();
  await page.keyboard.press("Control+V");
  await expect(input).toHaveValue(
    (await code.locator("pre").textContent()) ?? "",
  );
  await input.fill("");

  await page.getByRole("button", { name: /Jump to latest/ }).press("Enter");
  await expect(
    page.getByText("End of the long answer.", { exact: true }),
  ).toBeInViewport();
  results.long = { id };
});
