import { randomUUID } from "node:crypto";

import { expect, type Page } from "@playwright/test";

import { afterStart } from "./stack-start";

// What the agent-team stack specs share (agents-stack.spec.ts, agents-edges-stack.spec.ts): the team and its
// events as the person's API reads them, the edge cases' leads, and stopping every team a test started.

export type Team = {
  state: string;
  stop_reason: string | null;
  lead: { id: string; read_seq: number; unread: number };
  waiting_for_you: { seq: number } | null;
  helpers: { conversation: string; name: string }[];
};

/**
 * Every lead a test starts, so its team is stopped after the test whatever happened: a team left running holds slots,
 * and the next test's lead then waits for one and never starts its helpers (the first edge run, 7 Oct).
 */
export const started: string[] = [];

export async function stopStarted(page: Page, baseURL: string | undefined) {
  for (const lead of started.splice(0)) {
    const now = await team(page, lead).catch(() => null);
    if (!baseURL || !now || now.state === "ended") continue;
    await page.request
      .post(`/api/v1/conversations/${lead}/team/stop`, {
        headers: writing(baseURL),
      })
      .catch(() => undefined);
  }
}

/** Headers for a write from the page's own origin. */
export const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

export async function team(page: Page, lead: string): Promise<Team | null> {
  const response = await page.request.get(`/api/v1/conversations/${lead}/team`);
  if (response.status() === 404) return null;
  expect(response.status()).toBe(200);
  return (await response.json()) as Team;
}

/** Every event of `task`'s journal, in order: page after page (a page holds at most 1000). */
export async function events(page: Page, task: string) {
  type Frame = {
    seq: number;
    event?: { type: string; lead_brief?: unknown; content?: string };
  };
  const frames: Frame[] = [];
  for (;;) {
    const after = frames.at(-1)?.seq ?? 0;
    const response = await page.request.get(
      `/api/v1/conversations/${task}/events?limit=1000&after=${after}`,
    );
    expect(response.ok()).toBe(true);
    const { items } = (await response.json()) as { items: Frame[] };
    frames.push(...items);
    if (items.length < 1000) break;
  }
  return frames.flatMap((frame) => (frame.event ? [frame.event] : []));
}

// The edge cases (E2E-AGENTS-021..025, the owner's acceptance bar of 7 Oct, point 2), each a lead of its own whose
// person's message holds the EDGE- phrase that picks its script in agents.json. Helpers named "holder-…" hold every
// reply for ten minutes, so they stay working until stopped, deleted or restarted; "ghost" has no script, so the fake
// model refuses it and its run fails. Every case stops its team at the end, so held helpers never keep a slot.
export type EdgeHelper = { conversation: string; name: string; state: string };

/** Starts a leadId with `phrase` in its person's message, as a person would, and returns its id. */
export async function edgeLead(page: Page, baseURL: string, phrase: string) {
  const created = await page.request.post("/api/v1/conversations", {
    headers: writing(baseURL),
    data: { title: phrase, model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  started.push(id);
  const sent = await page.request.post(`/api/v1/conversations/${id}/messages`, {
    headers: writing(baseURL),
    data: { text: `${phrase}.`, attachments: [] },
  });
  expect(sent.ok()).toBe(true);
  return id;
}

/** Each helper's state, by name, as the team route says it. */
export async function helperStates(
  page: Page,
  leadId: string,
): Promise<Record<string, string>> {
  const response = await page.request.get(
    `/api/v1/conversations/${leadId}/team`,
  );
  if (response.status() === 404) return {};
  expect(response.status()).toBe(200);
  const { helpers } = (await response.json()) as { helpers: EdgeHelper[] };
  return Object.fromEntries(
    helpers.map((helper): [string, string] => [helper.name, helper.state]),
  );
}

/** Stops the team if it still runs (each case's end), and waits for it to have ended. */
export async function stopIfRunning(
  page: Page,
  baseURL: string,
  leadId: string,
) {
  const now = await team(page, leadId);
  if (!now || now.state === "ended") return;
  const stopped = await page.request.post(
    `/api/v1/conversations/${leadId}/team/stop`,
    { headers: writing(baseURL) },
  );
  expect(stopped.ok(), "stop the team").toBe(true);
  await expect
    .poll(async () => (await team(page, leadId))?.state, {
      timeout: afterStart(60_000),
    })
    .toBe("ended");
}

/** The task list's region and a lead's row in it. */
export function leadRow(page: Page, leadId: string) {
  // By the task's address, not its title: two suites' teams may share a title (WEB-E2E-062 and E2E-AGENTS-021).
  return page
    .getByRole("region", { name: "Tasks", exact: true })
    .getByRole("link")
    .and(page.locator(`[href="/c/${encodeURIComponent(leadId)}"]`));
}
