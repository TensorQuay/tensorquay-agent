import { readFile, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import { afterStart } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-E2E-042 on the real stack (WEB-U-068 backs it up on the fake front): the environment recovers after a
// failure. The driver (deploy/tests/demo_task_page_recovery.py) kills the task's own sandbox container mid-run when
// asked, twice. The gateway replaces it at once the first time and after 5 s the second (manager/losses.rs), the page
// says so and counts down, and the replacement's runtime resumes the run, which answers once. The scripted reply
// (fixtures/task-page.json, "Recover from a lost environment") takes 10 s per call, so each kill lands mid-call.

test.skip(!process.env.TQA_STACK_URL, "Requires the owned task-page stack");
test.describe.configure({ mode: "serial" });

const ANSWER = "The run finished after its environment was replaced twice.";

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

/** The driver's markers directory; the spec fails, not skips, without it. */
function markers() {
  const directory = process.env.TQA_TASK_PAGE_RECOVERY_MARKERS;
  if (!directory)
    throw new Error(
      "The driver must start the sandbox killer (TQA_TASK_PAGE_RECOVERY_MARKERS)",
    );
  return directory;
}

/** Waits for the driver's marker `name`, or fails naming the driver's own failure. */
async function marker(name: string) {
  const directory = markers();
  let text: string | null = null;
  await expect
    .poll(
      async () => {
        const failed = await readFile(
          join(directory, "kill-failed"),
          "utf8",
        ).catch(() => null);
        if (failed) throw new Error(`The sandbox killer failed: ${failed}`);
        text = await readFile(join(directory, name), "utf8").catch(() => null);
        return text !== null;
      },
      { timeout: afterStart(120_000) },
    )
    .toBe(true);
  return text ?? "";
}

/** Asks the driver to kill the task's sandbox; returns the killed container once it's gone. */
async function killSandbox(task: string) {
  const directory = markers();
  await rm(join(directory, "killed"), { force: true });
  await rm(join(directory, "replaced"), { force: true });
  // Whole or not at all: the driver reads the request once it exists.
  await writeFile(join(directory, "kill-wanted.partial"), task);
  await rename(
    join(directory, "kill-wanted.partial"),
    join(directory, "kill-wanted"),
  );
  return marker("killed");
}

/** Waits until the gateway's replacement for the killed container is running. */
async function replaced(killed: string) {
  expect(await marker("replaced")).not.toBe(killed);
}

const status = (page: Page) => page.getByLabel("Run status");
const line = (page: Page) => page.locator(".environment-status");

test("WEB-E2E-042 the environment recovers after a failure, twice mid-run, and the run answers once", async ({
  page,
}) => {
  // Three 10 s calls, two replacements (one after 5 s) and the environment starts: past the stack's default 60 s.
  test.setTimeout(240_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signIn(page);
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Recover from a lost environment, then answer.");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[^/]+$/);
  const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  await expect(status(page)).toHaveText(/^Working…/, {
    timeout: afterStart(60_000),
  });

  // The first loss: replaced at once (no wait), and the run goes on.
  await replaced(await killSandbox(task));
  await expect(status(page)).toHaveText(/^Working…/, {
    timeout: afterStart(60_000),
  });

  // The second within 10 minutes: replaced after 5 s, counted down on the page, the run still unfinished.
  const second = await killSandbox(task);
  await expect(line(page)).toHaveText(
    /^Environment stopped 2 times; restarting in [1-5] s$/,
  );
  await expect(page.getByText(ANSWER)).toHaveCount(0);
  await replaced(second);

  // The replacement's runtime resumes the run: it answers once, and the environment is no longer news.
  await expect(status(page)).toHaveText(/^Done/, {
    timeout: afterStart(60_000),
  });
  await expect(page.getByText(ANSWER)).toHaveCount(1);
  await expect(line(page)).toHaveCount(0);

  // Opened again, it replays the same: one answer, done, nothing about the losses.
  await page.reload();
  await expect(status(page)).toHaveText(/^Done/);
  await expect(page.getByText(ANSWER)).toHaveCount(1);
  await expect(line(page)).toHaveCount(0);
});
