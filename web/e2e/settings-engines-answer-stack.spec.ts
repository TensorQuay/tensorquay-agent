import { randomUUID } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { signedIn } from "./ux/flow";

// WEB-E2E-070, the decision engines' positive path on the real stack (WEB-E2E-061 has the engines' settings and the
// unreachable fallback): a task under a decision engine that answers. The settings suite's fake model serves two
// engines (TK-U-048), declared in its gateway.toml (deploy/tests/demo_decision_engines.py: plain http is file-only):
// - `stopper` stops the run at its first step: the page says the engine stopped the task, once, with its reason;
// - `passer` lets the run go on as without an engine: the task finishes, and each step's decision is recorded.
// Neither falls back. Each task runs under a profile naming its engine; the spec leaves no profile or task behind.

test.skip(
  process.env.TQA_STACK_SUITE !== "settings",
  "Requires the settings stack suite",
);

const RUN = randomUUID().slice(0, 8);
const STOPPED = "The test engine stops here.";

const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

type Frame = {
  event?: {
    type: string;
    engine?: string;
    choice?: { kind?: string; decision?: string };
  };
};

/** A task under a profile naming `engine`, with one message sent from its page; returns its id and its profile's. */
async function underEngine(page: Page, baseURL: string, engine: string) {
  const made = await page.request.post("/api/v1/profiles", {
    headers: writing(baseURL),
    data: {
      name: `${engine}-${RUN}`,
      display_name: `${engine} ${RUN}`,
      decision_engine: engine,
    },
  });
  expect(made.status(), `the profile naming ${engine}`).toBe(201);
  const profile = (await made.json()) as { id: string; version: number };
  const created = await page.request.post("/api/v1/conversations", {
    headers: writing(baseURL),
    data: {
      title: `Engine ${engine} ${RUN}`,
      model_route: "local",
      profile: profile.id,
    },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  await page.goto(`/c/${id}`);
  const box = page.getByRole("textbox", { name: "Message", exact: true });
  await box.fill("Make a short note.");
  await box.press("Enter");
  await expect(page.getByLabel("Run status")).not.toHaveText(
    /^(Sending|Starting|Working)/,
    { timeout: afterStart(120_000) },
  );
  return { id, profile };
}

/** The task's stored events, as a reload replays them. */
async function events(page: Page, id: string) {
  const response = await page.request.get(`/api/v1/conversations/${id}/events`);
  expect(response.ok()).toBe(true);
  return ((await response.json()) as { items: Frame[] }).items.flatMap(
    (frame) => (frame.event ? [frame.event] : []),
  );
}

async function cleanUp(
  page: Page,
  baseURL: string,
  made: { id: string; profile: { id: string; version: number } }[],
) {
  for (const { id, profile } of made) {
    await page.request.delete(`/api/v1/conversations/${id}`, {
      headers: writing(baseURL),
    });
    await page.request.delete(
      `/api/v1/profiles/${profile.id}?version=${profile.version}`,
      {
        headers: writing(baseURL),
      },
    );
  }
}

test("WEB-E2E-070 a decision engine that answers: a stop said once, and a passing engine's run finished", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 300_000));
  await signedIn(page);
  const made: { id: string; profile: { id: string; version: number } }[] = [];
  try {
    // The stopping engine: the run ends at its first step, and the page says why, once.
    const stopped = await underEngine(page, baseURL, "stopper");
    made.push(stopped);
    await expect(page.getByLabel("Run status")).toHaveText(/^Failed/);
    // Said once in the transcript (the status line may name it too).
    const transcript = page.getByTestId("transcript");
    await expect(
      transcript.getByText(/The decision engine stopped the task/),
    ).toHaveCount(1);
    await expect(transcript.getByText(STOPPED)).toHaveCount(1);
    await expect(page.getByTestId("transcript")).not.toContainText(
      /decision_stop|decision\.fallback/,
    );
    const stopEvents = await events(page, stopped.id);
    expect(
      stopEvents.some(
        (event) => event.type === "decision" && event.engine === "stopper",
      ),
    ).toBe(true);
    expect(stopEvents.some((event) => event.type === "decision.fallback")).toBe(
      false,
    );
    // Opened again: the same, once.
    await page.reload();
    await expect(
      page
        .getByTestId("transcript")
        .getByText(/The decision engine stopped the task/),
    ).toHaveCount(1);

    // The passing engine: the run goes on as without one, and finishes; every step's decision is the engine's.
    const passed = await underEngine(page, baseURL, "passer");
    made.push(passed);
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
    const passEvents = await events(page, passed.id);
    const decided = passEvents.filter(
      (event) => event.type === "decision" && event.engine === "passer",
    );
    expect(decided.length, "consulted before each step").toBeGreaterThan(0);
    expect(passEvents.some((event) => event.type === "decision.fallback")).toBe(
      false,
    );
    await expect(
      page.getByTestId("transcript").getByText(/The decision engine/),
    ).toHaveCount(0);
  } finally {
    await cleanUp(page, baseURL, made);
  }
});
