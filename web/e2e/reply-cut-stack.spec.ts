import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget as startBudget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-028: failures a person can hit when a model's reply is cut, on the real stack with the fake model's
// reply-cut scenario (crates/tqa-testkit/scenarios/reply-cut.json). A cut text, a cut thought and a cut tool call
// each end the run with plain words and no raw code; the cut call never runs; a new message then gets an answer. A
// spent output budget and an input too long for the model each show their own plain words. Every value in the result
// file is a read; the driver (deploy/tests/demo.py, suite "reply-cut") checks it.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned reply-cut stack");

const RUN = Date.now().toString(36);
const CUT_WORDS =
  "The model's reply was cut off at its output limit. Raise Max output on the route, or the model server's context or max tokens.";
/** A reasoning-only cut: the model spent its whole limit thinking, which has its own words. */
const THINKING_WORDS =
  "The model spent its whole output limit thinking and wrote no answer. Send the message again: a cut like this is often a one-off. If it keeps happening, an administrator can raise Max output on the route above its current limit, or set a thinking budget in the route's Extra body if its model server supports one.";
const BUDGET_WORDS =
  "The reply was cut off at the task's length limit. Ask for a shorter answer, or send a message to continue.";
const TOO_LONG_NEXT = "Start a new task, or ask for a short summary first.";
/** Raw words a person must never see on the page. */
const RAW = [
  "reply_cut",
  "budget.output_tokens",
  "finish: length",
  "512 tokens",
  "provider_error",
];
const result: Record<string, unknown> = { rawShown: false };

type Frame = { event?: { type: string; code?: string; reason?: string } };

async function write(
  page: Page,
  method: "POST" | "PUT",
  path: string,
  data: unknown,
) {
  const response = await page.request.fetch(path, {
    method,
    data,
    headers: {
      Origin: new URL(page.url()).origin,
      "Idempotency-Key": randomUUID(),
    },
  });
  expect(
    response.ok(),
    `${method} ${path}: ${response.status()} ${await response.text()}`,
  ).toBe(true);
  return (await response.json()) as Record<string, unknown>;
}

/** The stack's route, set so the numbers in a cut's reason are exact: usage streamed, Max output 2048. */
async function setRoute(page: Page, model: string) {
  const routes = (await (
    await page.request.get("/api/v1/admin/routes")
  ).json()) as {
    items: Record<string, unknown>[];
  };
  const local = routes.items.find((item) => item.name === "local") ?? {};
  const keep = [
    "protocol",
    "base_url",
    "model",
    "auth",
    "key_ref",
    "context_window",
    "concurrency",
    "requests_per_minute",
    "native_tools",
    "parallel_tools",
    "vision",
    "reasoning",
    "cache_hint",
    "extra_headers",
    "extra_body",
    "quirks",
    "strict",
    "schema_keywords",
    "instructions_role",
    "fallbacks",
    "display_name",
    "description",
    "version",
  ];
  const body = Object.fromEntries(
    Object.entries(local).filter(([key]) => keep.includes(key)),
  );
  await write(page, "PUT", "/api/v1/admin/routes/local", {
    ...body,
    model,
    stream_usage: true,
    max_output: 2048,
  });
}

/** The task's durable events. */
async function events(page: Page, task: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${task}/events`,
  );
  expect(response.ok()).toBe(true);
  return ((await response.json()) as { items: Frame[] }).items.flatMap(
    (frame) => (frame.event ? [frame.event] : []),
  );
}

async function send(page: Page, text: string) {
  const message = page.getByLabel("Message", { exact: true });
  await message.fill(text);
  await message.press("Enter");
}

async function failed(page: Page) {
  await expect(page.getByLabel("Run status")).toHaveText(/^Failed/, {
    timeout: afterStart(30_000),
  });
  for (const raw of RAW)
    if ((await page.content()).includes(raw)) result.rawShown = true;
}

/** Starts a task whose first message names a scenario phrase; returns its id once the run has failed. */
async function cutTask(page: Page, phrase: string, words = CUT_WORDS) {
  await page.goto("/");
  await send(page, `${phrase}, please.`);
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  await failed(page);
  await expect(page.locator(".turn-outcome").last()).toContainText(words);
  return task;
}

/** A cut case: its failure's code and reason, and whether a new message then completed. */
async function afterCut(
  page: Page,
  task: string,
  extra: Record<string, unknown> = {},
) {
  const failure = (await events(page, task)).find(
    (event) => event.type === "run.failed",
  );
  await send(page, "Try again, shorter.");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 120_000,
  });
  const completed = (await events(page, task)).some(
    (event) => event.type === "run.completed",
  );
  return {
    code: failure?.code,
    reason: failure?.reason,
    ...extra,
    again: completed ? "completed" : "not completed",
  };
}

test("WEB-E2E-028 a cut reply, a spent budget and a too-long input end with plain words", async ({
  page,
}) => {
  test.setTimeout(startBudget(3, 210_000));
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  await setRoute(page, "fixture");

  // A text cut short of the budget.
  let task = await cutTask(page, "Cut the text");
  await axe(page);
  result.text = await afterCut(page, task);

  // A thinking model spends its whole limit on reasoning.
  task = await cutTask(page, "Cut the thinking", THINKING_WORDS);
  result.thinking = await afterCut(page, task);

  // A cut tool call is never run: no call is recorded and no step line shows for it.
  task = await cutTask(page, "Cut the call");
  await expect(
    page.getByRole("button", { name: /^Ran a command/ }),
  ).toHaveCount(0);
  const ran = (await events(page, task)).some(
    (event) => event.type === "tool.call" || event.type === "tool.start",
  );
  result.call = await afterCut(page, task, { toolRan: ran });

  // The task's own output budget really spent: a profile allowing 300 tokens, and a reply that uses them.
  const profile = await write(page, "POST", "/api/v1/profiles", {
    name: `short-${RUN}`,
    display_name: "Short answers",
    model_route: "local",
    limits: { max_output_tokens: 300 },
  });
  const created = await write(page, "POST", "/api/v1/conversations", {
    title: "Budget",
    profile: profile.id,
  });
  await page.goto(`/c/${String(created.id)}`);
  await send(page, "Spend the budget, please.");
  await failed(page);
  await expect(page.locator(".turn-outcome").last()).toContainText(
    BUDGET_WORDS,
  );
  const budget = (await events(page, String(created.id))).find(
    (event) => event.type === "run.failed",
  );
  result.budget = { code: budget?.code, reason: budget?.reason };

  // The model server refuses an input longer than its context (the fake's too-long model answers 400 with its words).
  await setRoute(page, "too-long");
  await page.goto("/");
  await send(page, "Summarise this very long document.");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  await expect(page.getByLabel("Run status")).toHaveText(
    "Failed: The conversation is too long for the model",
    {
      timeout: afterStart(30_000),
    },
  );
  await expect(page.locator(".turn-outcome").last()).toContainText(
    TOO_LONG_NEXT,
  );
  await axe(page);
  const tooLong = (await events(page, task)).find(
    (event) => event.type === "run.failed",
  );
  result.tooLong = { code: tooLong?.code };
  // provider_error keeps its own muted code line; nothing else raw is shown.
  for (const raw of RAW.filter((word) => word !== "provider_error"))
    if ((await page.content()).includes(raw)) result.rawShown = true;

  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify(result));
});
