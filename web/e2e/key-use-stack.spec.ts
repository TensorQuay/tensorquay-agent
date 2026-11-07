import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-029 (model call): on the real stack, with the gateway's read-audit table made unwritable by the driver
// (deploy/tests/demo.py, suite "key-use"), the route's key is never used: the relay refuses the model call as its own
// `key_use_unrecorded`, and the page says so in plain words, never as the provider's error. Every value in the result
// file is a read; the driver checks it, and that the fake model received no request.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned key-use stack");

const OUTCOME =
  "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.";
const STATUS =
  "Failed: The key could not be used because its use could not be recorded";
/** Words a person must never see for the gateway's own refusal. */
const RAW = ["provider said", "503", "key_use_unrecorded"];

type Frame = {
  event?: {
    type: string;
    code?: string;
    reason?: string;
    provider_message?: string;
  };
};

async function events(page: Page, task: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${task}/events`,
  );
  expect(response.ok()).toBe(true);
  return ((await response.json()) as { items: Frame[] }).items.flatMap(
    (frame) => (frame.event ? [frame.event] : []),
  );
}

test("WEB-E2E-029 a model call whose key use can't be recorded fails with plain words", async ({
  page,
}) => {
  test.setTimeout(budget(1, 210_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  await page.goto("/");
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Say hello, please.");
  await message.press("Enter");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  // The runtime retries a retryable refusal before it gives up.
  await expect(page.getByLabel("Run status")).toHaveText(STATUS, {
    timeout: afterStart(150_000),
  });
  await expect(page.locator(".turn-outcome").last()).toContainText(OUTCOME);
  await axe(page);
  // What a person reads (WEB-U-016, as failures.spec.ts reads it): the page's visible text, where a code may show only
  // in a failure's muted Code line. A step's retries keep their codes in a tooltip, after "Details:", which is no
  // visible text: a title attribute holding a raw code is allowed only there.
  const codeLines = page.locator(".turn-code");
  for (const line of await codeLines.all()) {
    await expect(line).toHaveClass(/muted/);
    await expect(line).toHaveText(/^Code: /);
  }
  let shown = await page.locator("body").innerText();
  for (const line of await codeLines.allInnerTexts())
    shown = shown.replace(line, "");
  const titles = await page
    .locator("[title]")
    .evaluateAll((elements) =>
      elements.map((element) => element.getAttribute("title") ?? ""),
    );
  const titledRaw = titles.filter((title) =>
    RAW.some(
      (raw) =>
        title.includes(raw) &&
        (!title.includes("Details:") ||
          title.indexOf(raw) < title.indexOf("Details:")),
    ),
  );
  const failure = (await events(page, task)).find(
    (event) => event.type === "run.failed",
  );
  const result = {
    code: failure?.code,
    reason: failure?.reason,
    providerMessage: failure?.provider_message ?? null,
    rawShown: [
      ...RAW.filter((raw) => shown.includes(raw)),
      ...titledRaw.map((title) => `title: ${title}`),
    ],
  };
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify(result));
});
