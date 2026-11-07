import { randomUUID } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";

import { allowKey } from "./route-keys";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-063: a done step's retry reasons are a line in the step, read with its toggle (backs WEB-U-304), on the
// task-page suite's stack. A route on the text protocol (native_tools off) gets, under the phrase "Read a reply on the
// second try" in deploy/tests/fixtures/task-page.json, a tqa_call block that isn't JSON once, then a plain answer: the
// runtime retries once for "fallback_parse: malformed" and the step completes; the retry's event names the problem
// (fallback_problem, ADR-0007: not JSON, at its line and column). WEB-E2E-092: a block that is JSON without `name`
// ("Read a reply that names no tool") is retried the same way, its event naming `no_name` (backs CORE-U-088 on the
// stack). The page words the retry by its reason alone, so the problem is read from the journal.
test.skip(
  process.env.TQA_STACK_SUITE !== "task-page",
  "Requires the task-page stack suite",
);

const RUN = randomUUID().slice(0, 8);
const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

/** A copy of the stack's `local` route on the text protocol, checked; returns a best-effort removal. */
async function textRoute(page: Page, baseURL: string, name: string) {
  const routes = (await (
    await page.request.get("/api/v1/admin/routes")
  ).json()) as { items: Record<string, unknown>[] };
  const local = routes.items.find((item) => item.name === "local");
  expect(local, "the stack's local route").toBeDefined();
  const keep = ["protocol", "base_url", "auth", "key_ref", "model", "strict"];
  const body = Object.fromEntries(
    Object.entries(local ?? {}).filter(([key]) => keep.includes(key)),
  );
  // A copy keeps the local route's shared key, which must be allowed for the new route first (409 secret_not_allowed).
  if (typeof body.key_ref === "string")
    await allowKey(page, body.key_ref, name, baseURL);
  const put = await page.request.put(`/api/v1/admin/routes/${name}`, {
    headers: writing(baseURL),
    data: { ...body, native_tools: false, display_name: `Text ${RUN}` },
  });
  expect(put.ok(), `create ${name}: ${put.status()}`).toBe(true);
  const { version } = (await put.json()) as { version: number };
  await page.request.post(`/api/v1/admin/routes/${name}/probe`, {
    headers: writing(baseURL),
    timeout: afterStart(60_000),
  });
  return async () => {
    await page.request.delete(
      `/api/v1/admin/routes/${name}?version=${version}`,
      { headers: writing(baseURL) },
    );
  };
}

type Retry = {
  type: string;
  reason?: string | undefined;
  fallback_problem?: Record<string, unknown> | undefined;
};

/**
 * The conversation's provider.retry events, from its journal: each as its type, reason and problem only (the event
 * also carries its attempt and delay, which these tests don't pin).
 */
async function retries(page: Page, id: string): Promise<Retry[]> {
  const response = await page.request.get(
    `/api/v1/conversations/${id}/events?limit=1000`,
  );
  expect(response.ok()).toBe(true);
  const { items } = (await response.json()) as { items: { event?: Retry }[] };
  return items.flatMap(({ event }) =>
    event?.type === "provider.retry"
      ? [
          {
            type: event.type,
            reason: event.reason,
            fallback_problem: event.fallback_problem,
          },
        ]
      : [],
  );
}

test("WEB-E2E-063 a done step's retry reasons are a line in the step, its toggle's description", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  await bootstrap(page);
  const name = `text-${RUN}`;
  const remove = await textRoute(page, baseURL, name);
  try {
    const created = await page.request.post("/api/v1/conversations", {
      headers: writing(baseURL),
      data: { title: `Retried ${RUN}`, model_route: name },
    });
    expect(created.status()).toBe(201);
    const { id } = (await created.json()) as { id: string };
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const box = page.getByRole("textbox", { name: "Message", exact: true });
    await box.fill("Read a reply on the second try.");
    await box.press("Enter");
    await expect(page.getByText("Read on the second try.")).toBeVisible({
      timeout: afterStart(120_000),
    });
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
      timeout: afterStart(60_000),
    });

    // The done step folds; its toggle counts the retry by its reason and is described by the codes, as the summary
    // already says the reason (Z27).
    const words = "Details: fallback_parse: malformed";
    const step = page.getByRole("group", { name: "Activity" }).last();
    const toggle = step.getByRole("button", { name: /Worked for/ });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(toggle).toContainText(
      /the model's reply couldn't be read, retried once$/,
    );
    await expect(toggle).toHaveAccessibleDescription(words);
    // Opened by keyboard, the same words are the step's own line, once.
    await toggle.focus();
    await page.keyboard.press("Enter");
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(step.getByText(words, { exact: true })).toBeVisible();
    await expect(page.getByText(words, { exact: true })).toHaveCount(1);
    await expect(step.getByText(/couldn't be read/)).toHaveCount(1);

    // The journal names what was wrong, without the reply's content: `}` stood where `,` belonged, line 1, column 18.
    expect(await retries(page, id)).toEqual([
      {
        type: "provider.retry",
        reason: "fallback_parse: malformed",
        fallback_problem: {
          kind: "not_json",
          cut_off: false,
          line: 1,
          column: 18,
        },
      },
    ]);
  } finally {
    await remove();
  }
});

test("WEB-E2E-092 a call block without a name is retried naming it, and the step completes", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  await bootstrap(page);
  const name = `noname-${RUN}`;
  const remove = await textRoute(page, baseURL, name);
  try {
    const created = await page.request.post("/api/v1/conversations", {
      headers: writing(baseURL),
      data: { title: `No name ${RUN}`, model_route: name },
    });
    expect(created.status()).toBe(201);
    const { id } = (await created.json()) as { id: string };
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const box = page.getByRole("textbox", { name: "Message", exact: true });
    await box.fill("Read a reply that names no tool.");
    await box.press("Enter");
    await expect(page.getByText("Read after the tool was named.")).toBeVisible({
      timeout: afterStart(120_000),
    });
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
      timeout: afterStart(60_000),
    });
    const toggle = page
      .getByRole("group", { name: "Activity" })
      .last()
      .getByRole("button", { name: /Worked for/ });
    await expect(toggle).toContainText(
      /the model's reply couldn't be read, retried once$/,
    );
    await expect(page.locator("body")).not.toContainText(
      "Invalid protocol event",
    );

    // One retry, its stable reason unchanged, its problem the missing name.
    expect(await retries(page, id)).toEqual([
      {
        type: "provider.retry",
        reason: "fallback_parse: malformed",
        fallback_problem: { kind: "no_name" },
      },
    ]);
  } finally {
    await remove();
  }
});
