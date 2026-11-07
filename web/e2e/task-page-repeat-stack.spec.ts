import { randomUUID } from "node:crypto";

import { expect, test } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-065: a step that keeps failing stops the run with "several times" (backs WEB-U-016's repeated_failure case;
// CORE-E-041's guard), on the task-page suite's stack. Under the phrase "Fail the same step again and again" in
// deploy/tests/fixtures/task-page.json the scripted model calls `shell` with `exit 3` at every step (the last reply
// repeats), so the same call fails three times with no change between and the runtime stops the run.
test.skip(
  process.env.TQA_STACK_SUITE !== "task-page",
  "Requires the task-page stack suite",
);

const RUN = randomUUID().slice(0, 8);

test("WEB-E2E-065 the same step failing again and again stops the run, said as several times", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  await bootstrap(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: `Repeated ${RUN}`, model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const box = page.getByRole("textbox", { name: "Message", exact: true });
  await box.fill("Fail the same step again and again.");
  await box.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Failed/, {
    timeout: afterStart(180_000),
  });

  // The journal: three results of the one call, all failed, and the run's code (the words key on it, not its reason).
  const response = await page.request.get(
    `/api/v1/conversations/${id}/events?limit=1000`,
  );
  expect(response.ok()).toBe(true);
  const { items } = (await response.json()) as {
    items: { event?: { type: string; status?: string; code?: string } }[];
  };
  const events = items.flatMap((item) => (item.event ? [item.event] : []));
  expect(
    events.filter((e) => e.type === "tool.result" && e.status === "error"),
  ).toHaveLength(3);
  expect(events.find((e) => e.type === "run.failed")?.code).toBe(
    "repeated_failure",
  );

  // The page says it in words, with what to do next; never the runtime's own reason text.
  await expect(
    page
      .getByText(
        "The agent tried the same step several times and it kept failing",
      )
      .first(),
  ).toBeVisible();
  await expect(
    page.getByText("Send a message to try another way.").first(),
  ).toBeVisible();
  await expect(page.locator("body")).not.toContainText(
    "the same tool call failed three times",
  );
  await expect(page.locator("body")).not.toContainText("three times");
});
