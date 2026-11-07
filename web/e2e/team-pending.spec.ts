import { randomUUID } from "node:crypto";

import type { Locator, Page } from "@playwright/test";

import { HOLD, HOLD_START } from "./front/stream.mjs";
import { recordTexts } from "./header-texts";
import { signIn } from "./session";
import { expect, resetFront, test } from "./test";

// WEB-U-406 (16t int, E2E-AGENTS-032's third failure): a task the page has any sign of a team for (its record's team, or
// a team frame on its stream before the record has its place) never reads Done or Failed until the team layer has
// spoken; then the layer's words. The three orders the stack has shown, on the fake front, sampled every 50 ms.

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["agent-teams"]);
});

const WAITING = "Waiting for the lead to finish the team";

/** The header's words every 50 ms for `ms`, or until `stop` reads true. */
async function sample(status: Locator, ms: number, stop?: () => boolean) {
  const seen: string[] = [];
  const until = Date.now() + ms;
  while (Date.now() < until && !stop?.()) {
    const words = ((await status.textContent().catch(() => "")) ?? "").trim();
    if (seen.at(-1) !== words) seen.push(words);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  return seen;
}

const ended = (seen: string[]) =>
  seen.filter(
    (words) => words.startsWith("Done") || words.startsWith("Failed"),
  );

/** The seeded lead task's id, from the task list the page reads. */
async function seededLead(page: Page) {
  const listed = await page.request.get("/api/v1/conversations?scope=top");
  const { items } = (await listed.json()) as {
    items: { id: string; title?: string | null }[];
  };
  return items.find((item) => item.title === "Report on Q3")?.id ?? "";
}

/** Holds back every request `matches` picks by `ms` while `held.on`. */
async function holdBack(
  page: Page,
  matches: (url: URL) => boolean,
  ms: number,
) {
  const held = { on: true, count: 0 };
  await page.route(
    (url) => matches(url),
    async (route) => {
      if (held.on) {
        held.count += 1;
        await new Promise((resolve) => setTimeout(resolve, ms));
      }
      await route.continue();
    },
  );
  return held;
}

// E2E-AGENTS-032's order on 16t: the page opens on a task in no team; its run starts the team (the stream's team frame);
// the run ends; the task's read again comes back late. Until it does, the record has no team, only the frame says so.
test("WEB-U-406 a page opened before its team: the team's frame, the run's end, the task's read late, never Done", async ({
  page,
}) => {
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: "Plan the offsite", model_route: "local" },
  });
  expect(created.ok()).toBe(true);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  const status = page.getByLabel("Run status");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(HOLD);
  await input.press("Enter");
  await expect(status).toHaveText(/^Working…/);
  // Held after its narration, so the team starts while it works and the run ends on resume.
  await expect(page.getByText("I’ll check the project files.")).toBeVisible();
  // The task's own record, read again on the team's frame: held back 8 s.
  const record = `/api/v1/conversations/${id}`;
  const held = await holdBack(page, (url) => url.pathname === record, 8_000);
  expect(
    (
      await page.request.post(`/api/v1/conversations/${id}/team/_fixture/start`)
    ).status(),
  ).toBe(204);
  expect(
    (
      await page.request.post(`/api/v1/conversations/${id}/fixture-resume`)
    ).status(),
  ).toBe(204);
  const seen = await sample(status, 12_000, () => false);
  test.info().annotations.push({
    type: "header words",
    description: seen.join(" | "),
  });
  // The order happened: the task's read was held while the run ended and the header waited.
  expect(held.count).toBeGreaterThan(0);
  expect(seen).toContain("Waiting…");
  expect(ended(seen)).toEqual([]);
  held.on = false;
  await expect(status).toHaveText(WAITING);
});

// WEB-U-405's order: a lead's page opened after its turn ended, the team read answering before the stream replays.
test("WEB-U-406 a lead's page opened after its turn: the team read before the stream's replay, never Done", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const id = await seededLead(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Hello");
  await input.press("Enter");
  const status = page.getByLabel("Run status");
  await expect(status).toHaveText(WAITING);
  const events = `/api/v1/conversations/${id}/events`;
  await holdBack(page, (url) => url.pathname === events, 2_000);
  await page.reload();
  const seen = await sample(status, 3_500);
  test.info().annotations.push({
    type: "header words",
    description: seen.join(" | "),
  });
  expect(ended(seen)).toEqual([]);
  await expect(status).toHaveText(WAITING);
});

// WEB-U-404's order: a lead's page opened after its turn ended, the team read late.
test("WEB-U-406 a lead's page opened after its turn: the team read late, never Done", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const id = await seededLead(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Hello");
  await input.press("Enter");
  const status = page.getByLabel("Run status");
  await expect(status).toHaveText(WAITING);
  const team = `/api/v1/conversations/${id}/team`;
  await holdBack(page, (url) => url.pathname === team, 2_000);
  await page.reload();
  const seen = await sample(status, 3_500);
  test.info().annotations.push({
    type: "header words",
    description: seen.join(" | "),
  });
  expect(ended(seen)).toEqual([]);
  expect(seen).toContain("Waiting…");
  await expect(status).toHaveText(WAITING);
});

// WEB-U-415 (16v int, E2E-AGENTS-032's fourth failure): with the team layer known, the lead's run ends. The header and
// the layer's words must change in the same render: on 16v the layer's words followed one effect later, so for one
// render the header read Done (or Failed) at every run's end while the team ran.
for (const { end, message, after } of [
  { end: "completed", message: "Hello", after: WAITING },
  { end: "failed", message: "Fail with io", after: "Waiting for its helpers" },
] as const)
  test(`WEB-U-415 a lead's run ${end} with its team layer known: no render reads ${end === "completed" ? "Done" : "Failed"}`, async ({
    page,
  }) => {
    await signIn(page);
    await page.goto("/");
    const id = await seededLead(page);
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const status = page.getByLabel("Run status");
    // The layer is known: its strip names the seeded researcher.
    await expect(
      page.getByRole("region", { name: "Helpers" }).getByRole("link").first(),
    ).toBeVisible();
    const texts = await recordTexts(page);
    const input = page.getByLabel("Message", { exact: true });
    await input.fill(message);
    await input.press("Enter");
    // The run's end, in the layer's words.
    await expect(status).toHaveText(new RegExp(`^${after}`));
    const seen = await texts();
    expect(seen.some((w) => w.startsWith("Working"))).toBe(true);
    test.info().annotations.push({
      type: "header texts",
      description: seen.join(" | "),
    });
    expect(ended(seen)).toEqual([]);
  });

// WEB-U-416 (16w int, E2E-AGENTS-032's fifth failure): the lead's first run starts its team and ends 0.18 s later, before
// the gateway's once-a-second flush sends the team's first team.changed. Until then the page had no sign of the team
// (no team in the record, no frame) and read Done. Its own transcript has one at once: start_member's ok result.
test("WEB-U-416 a lead's first run starts its team and ends before the team's frame: no render reads Done", async ({
  page,
}) => {
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: "Plan the offsite", model_route: "local" },
  });
  expect(created.ok()).toBe(true);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  const status = page.getByLabel("Run status");
  await expect(status).toBeVisible();
  const texts = await recordTexts(page);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(HOLD_START);
  await input.press("Enter");
  await expect(page.getByText("I’ll check the project files.")).toBeVisible();
  // The team starts while the run is held, as the gateway's start_member does before its result; its frame waits 1.5 s.
  expect(
    (
      await page.request.post(
        `/api/v1/conversations/${id}/team/_fixture/start?announce_after_ms=1500`,
      )
    ).status(),
  ).toBe(204);
  expect(
    (
      await page.request.post(`/api/v1/conversations/${id}/fixture-resume`)
    ).status(),
  ).toBe(204);
  await expect(page.getByText("Started a helper: researcher")).toBeVisible();
  await expect(status).toHaveText(WAITING);
  const seen = await texts();
  test.info().annotations.push({
    type: "header texts",
    description: seen.join(" | "),
  });
  expect(seen.some((words) => words.startsWith("Working"))).toBe(true);
  expect(ended(seen)).toEqual([]);
});
