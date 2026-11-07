import { expect, test, type Page } from "@playwright/test";

import { clearTasks } from "./queue-stack";
import { afterStart } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// T22, Keep environment on (WEB-U-204), on the real one-slot admission stack: a task's kept environment stays running
// and counts toward the tasks running at once, so with one slot it isn't made room for. The same person's next task
// waits for a free slot while it's kept, and starts once it isn't. The control first: a task that isn't kept is made
// room for (the gateway retires the owner's own idle sandbox, F26), so the wait is the keeping's alone.
// Each task's first message contains the queue fixture's "Wait for a free slot", whose reply answers at once.

test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned one-slot admission stack",
);
test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const status = (page: Page) => page.getByLabel("Run status");

/** Starts a task from New task and waits for its answer; returns its id. */
async function answeredTask(page: Page, label: string) {
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill(`Wait for a free slot: ${label}.`);
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[^/]+$/);
  await expect(status(page)).toHaveText(/^Done/, {
    timeout: afterStart(60_000),
  });
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

/** Sends a new task's first message and returns its id, once the gateway has said it is queued. */
async function queuedTask(page: Page, label: string) {
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill(`Wait for a free slot: ${label}.`);
  const accepted = page.waitForResponse(
    (response) =>
      /\/conversations\/[^/]+\/messages$/.test(response.url()) &&
      response.request().method() === "POST",
  );
  await input.press("Enter");
  const response = await accepted;
  expect(response.status()).toBe(202);
  expect(response.headers()["x-tqa-execution-status"]).toBe("queued");
  await expect(page).toHaveURL(/\/c\/[^/]+$/);
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

/** Sets Keep environment on or off from the task's ⋯ menu, as a person does, and checks the gateway kept it. */
async function keepEnvironment(page: Page, id: string, on: boolean) {
  await page.goto(`/c/${id}`);
  // The task header's own ⋯: every task in the sidebar has one too, named the same for this task.
  const more = page
    .getByRole("region", { name: "Task header" })
    .getByRole("button", { name: /^More actions for/ });
  await more.click();
  const toggle = page.getByRole("menuitemcheckbox", {
    name: "Keep environment on",
  });
  await expect(toggle).toHaveAttribute("aria-checked", String(!on));
  const saved = page.waitForResponse(
    (response) =>
      response.request().method() === "PATCH" &&
      response.url().endsWith(`/api/v1/conversations/${id}`),
  );
  await toggle.click();
  const answer = await saved;
  expect(answer.status()).toBe(200);
  expect(answer.request().postDataJSON()).toEqual({ pinned: on });
  // The gateway's own record, read back, and the menu after a reload.
  const read = await page.request.get(`/api/v1/conversations/${id}`);
  expect(((await read.json()) as { pinned: boolean }).pinned).toBe(on);
  await page.reload();
  await more.click();
  await expect(toggle).toHaveAttribute("aria-checked", String(on));
  await page.keyboard.press("Escape");
}

test("WEB-E2E-041 Keep environment on: a kept environment keeps its slot until it's let go", async ({
  page,
}) => {
  // Four tasks, each with an environment start, plus the hold below: past the stack's default 60 s.
  test.setTimeout(240_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signIn(page);
  await clearTasks(page);
  try {
    // The control: an idle task that isn't kept is made room for, so the next task starts without a lasting wait.
    await answeredTask(page, "the first task, not kept");
    const made = Date.now();
    await answeredTask(page, "the next task, which gets the slot");
    const startedWithin = Date.now() - made;

    // Kept: the next task waits, and goes on waiting well past the time the control took to start.
    const kept = await answeredTask(page, "a task that keeps its environment");
    await keepEnvironment(page, kept, true);
    const waiting = await queuedTask(
      page,
      "a task that waits for the kept one",
    );
    await expect(status(page)).toHaveText("Waiting for a free slot · next");
    await page.waitForTimeout(Math.max(10_000, 3 * startedWithin));
    await expect(status(page)).toHaveText("Waiting for a free slot · next");
    const read = await page.request.get(`/api/v1/conversations/${waiting}`);
    expect(
      ((await read.json()) as { run_status?: string }).run_status ?? "idle",
    ).toBe("idle");

    // Let go: the kept task is made room for, and the waiting one runs and answers.
    await keepEnvironment(page, kept, false);
    await page.goto(`/c/${waiting}`);
    await expect(status(page)).toHaveText(/^Done/, {
      timeout: afterStart(60_000),
    });
    await expect(
      page.getByText("The queued task ran once the slot was free."),
    ).toHaveCount(1);
  } finally {
    // Nothing kept or waiting is left for the one-slot suite's next spec.
    await clearTasks(page);
  }
});
