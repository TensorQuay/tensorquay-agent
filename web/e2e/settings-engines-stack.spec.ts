import { randomUUID } from "node:crypto";

import { expect, test, type APIResponse, type Page } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { signedIn } from "./ux/flow";

// Decision engines, a gap in the feature matrix before 17.0 (WEB-E2E-061): what had a unit test and a fake-front spec
// but no real-stack test. On the settings suite's stack: an engine added, edited, turned off and on, and deleted in
// Settings › Decision engines, through the real gateway; and a task whose profile names an engine that can't be
// reached, which falls back and says so once (WEB-U-093). An engine that answers is the lead's live check (c16 brings
// a fake engine service). It writes no result, and leaves no engine or profile behind.
test.skip(
  process.env.TQA_STACK_SUITE !== "settings",
  "Requires the settings stack suite",
);

const RUN = randomUUID().slice(0, 8);
/** A cleanup call's status, or 0 when it couldn't be made: cleanup never throws over the test's own failure. */
const status = async (call: Promise<APIResponse>) =>
  (await call.catch(() => undefined))?.status() ?? 0;
/** An https address that never resolves: the form accepts it, and the gateway's relay can't reach it. */
const UNREACHABLE = "https://engine.invalid";
const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

type Engine = {
  name: string;
  display_name?: string;
  enabled: boolean;
  version: number;
};
async function engineNamed(page: Page, display: string) {
  const response = await page.request.get("/api/v1/admin/decision/engines");
  expect(response.ok()).toBe(true);
  const { items } = (await response.json()) as { items: Engine[] };
  return items.find((item) => item.display_name === display);
}

/** The engines page's dialog: the kind first, then its fields, saved with Save engine. */
async function addEngine(page: Page, display: string) {
  await page.getByRole("button", { name: "Add decision engine" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a decision engine" });
  await dialog.getByRole("radio", { name: "Your own engine" }).check();
  await dialog.getByRole("button", { name: "Continue" }).click();
  await dialog.getByLabel("Display name", { exact: true }).fill(display);
  await dialog.getByLabel("Engine address", { exact: true }).fill(UNREACHABLE);
  // Key is the app's own picker (ui/Select), a button whose choices are menu rows, never a native select.
  await dialog.getByRole("button", { name: /^Key:/ }).click();
  await page
    .getByRole("menuitemradio", { name: "None: the engine needs no key" })
    .click();
  await expect(dialog.getByRole("button", { name: /^Key:/ })).toHaveText(
    "None: the engine needs no key",
  );
  await dialog.getByRole("button", { name: "Save engine" }).click();
  await expect(page.getByText(`Saved decision engine ${display}.`)).toBeVisible(
    {
      timeout: afterStart(30_000),
    },
  );
}

async function menu(page: Page, display: string, action: string) {
  await page
    .getByRole("button", { name: `More actions for ${display}`, exact: true })
    .click();
  await page.getByRole("menuitem", { name: action }).click();
}

test("WEB-E2E-061 decision engines: add, edit, turn off and on, delete; an unreachable engine falls back, said once", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 300_000));
  await signedIn(page);
  const first = `Engine ${RUN}`;
  const renamed = `Engine ${RUN} renamed`;
  await page.goto("/settings/decision-engines");

  // Add, through the dialog's two steps (WEB-U-074).
  await addEngine(page, first);
  const added = await engineNamed(page, first);
  expect(added, "the engine the page added").toBeDefined();
  const name = added?.name ?? "";

  // Edit its display name.
  await page
    .getByRole("button", { name: `Edit decision engine ${first}`, exact: true })
    .click();
  const edit = page.getByRole("dialog", {
    name: `Edit decision engine ${first}`,
  });
  await edit.getByLabel("Display name", { exact: true }).fill(renamed);
  await edit.getByRole("button", { name: "Save engine" }).click();
  await expect(
    page.getByText(`Saved decision engine ${renamed}.`),
  ).toBeVisible();

  // Off, after a dialog that says what that does to tasks; then on again.
  await menu(page, renamed, "Turn off");
  const off = page.getByRole("dialog", { name: `Turn off ${renamed}?` });
  await expect(off).toContainText(
    "While it is off, tasks whose profile uses it are refused",
  );
  await off.getByRole("button", { name: "Turn off" }).click();
  await expect(page.getByText(`Turned off ${renamed}.`)).toBeVisible();
  expect((await engineNamed(page, renamed))?.enabled).toBe(false);
  await menu(page, renamed, "Turn on");
  await expect(page.getByText(`Turned on ${renamed}.`)).toBeVisible();
  expect((await engineNamed(page, renamed))?.enabled).toBe(true);

  // A task whose profile names it: the engine can't be reached, so the run goes on deciding by itself, and the page
  // says so once, in words, with no code (WEB-U-093).
  const made = await page.request.post("/api/v1/profiles", {
    headers: writing(baseURL),
    data: {
      name: `engine-${RUN}`,
      display_name: `Engine profile ${RUN}`,
      decision_engine: name,
    },
  });
  expect(made.status(), "the profile naming the engine").toBe(201);
  const profile = (await made.json()) as { id: string; version: number };
  let task: string | undefined;
  const removed: Record<string, number> = {};
  try {
    const created = await page.request.post("/api/v1/conversations", {
      headers: writing(baseURL),
      data: {
        title: `Engine task ${RUN}`,
        model_route: "local",
        profile: profile.id,
      },
    });
    expect(created.status()).toBe(201);
    const { id } = (await created.json()) as { id: string };
    task = id;
    await page.goto(`/c/${id}`);
    const box = page.getByRole("textbox", { name: "Message", exact: true });
    await box.fill("Make a short note.");
    await box.press("Enter");
    const fallback = page.getByText(
      /^The decision engine couldn't be reached, so the agent is deciding on its own for this run\./,
    );
    await expect(fallback).toBeVisible({ timeout: afterStart(120_000) });
    await expect(page.getByLabel("Run status")).not.toHaveText(/^Working/, {
      timeout: afterStart(120_000),
    });
    await expect(fallback).toHaveCount(1);
    await expect(page.getByTestId("transcript")).not.toContainText(
      /decision\.fallback|engine_/,
    );
  } finally {
    // Its own task first: an open task keeps the engine in use, and the engine's delete below would be refused (the
    // 15b rerun: "1 open task use this engine"). The statuses are checked after, so they never hide a failure above.
    if (task)
      removed.task = await status(
        page.request.delete(`/api/v1/conversations/${task}`, {
          headers: writing(baseURL),
        }),
      );
    removed.profile = await status(
      page.request.delete(
        `/api/v1/profiles/${profile.id}?version=${profile.version}`,
        { headers: writing(baseURL) },
      ),
    );
  }
  expect(removed, "the test's own task and profile, removed").toEqual({
    task: 204,
    profile: 204,
  });

  // Delete, after its dialog.
  await page.goto("/settings/decision-engines");
  await menu(page, renamed, "Delete");
  const remove = page.getByRole("dialog", {
    name: `Delete decision engine ${renamed}?`,
  });
  await remove.getByRole("button", { name: "Delete engine" }).click();
  await expect(
    page.getByText(`Deleted decision engine ${renamed}.`),
  ).toBeVisible();
  expect(await engineNamed(page, renamed)).toBeUndefined();
});
