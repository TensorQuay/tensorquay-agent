import { randomUUID } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";

import { afterStart, budget, scaled, START_MS } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-E2E-121 (backs WEB-U-468): on the real stack behind a proxy that buffers the event stream's tail (the catch-up
// suite's overlay: the first 2 KiB pass, then 40 s of silence), the page catches up from the task's record and the
// paged read: the status reads Done and Send is back long before the proxy lets the tail go.
test.skip(
  process.env.TQA_STACK_SUITE !== "catch-up",
  "runs on the catch-up stack suite only",
);

const HOLD_MS = 40_000;

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

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

test("WEB-U-468 on the real stack: behind a buffering proxy the page catches up and gives Send back", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(1, HOLD_MS + 60_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await signIn(page);
  await task(page, baseURL, "Catch up");
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  const started = Date.now();
  await input.fill("Create the small project.");
  await input.press("Enter");
  // The stream opened and delivered the run's start before the proxy began holding it.
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/, {
    timeout: START_MS,
  });
  // The run ends behind the held stream; the record and the paged read bring the page there before the hold ends.
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(HOLD_MS - scaled(10_000)),
  });
  expect(Date.now() - started).toBeLessThan(START_MS + HOLD_MS);
  // Send is back: Stop is gone, the box takes a message and the (one) button sends it; it stays unavailable only
  // while the box is empty, as always.
  await expect(page.getByRole("button", { name: "Stop" })).toHaveCount(0);
  await expect(input).toBeEnabled();
  await input.fill("Thanks.");
  await expect(page.getByRole("button", { name: "Send" })).toBeEnabled();
  // What the paged read brought: the run's answer and its folded steps, not only its status.
  await expect(
    page.getByText(
      "Created hello/README.md with a title and a project description.",
    ),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /created 1 file, ran 1 command/ }),
  ).toBeVisible();
});
