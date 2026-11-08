import { randomUUID } from "node:crypto";
import { readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type BrowserContext, type Page } from "@playwright/test";

import { chooseModel } from "./model";
import { afterStart, budget, scaled } from "./stack-start";
import { bootstrap } from "./ux/flow";

// The feature matrix's gaps on the task page before 17.0 (A7, A11, T15 and T21): what had a unit test and a
// fake-front spec but no real-stack test, now on the task-page suite's stack with the scripted model in
// deploy/tests/fixtures/task-page.json. Each case makes its own tasks and leaves the suite's own spec and result alone.
test.skip(
  process.env.TQA_STACK_SUITE !== "task-page",
  "Requires the task-page stack suite",
);
// One sign-in for the whole file, its cookies reused by each test's own browser context: the gateway counts every
// sign-in from an address, 10 a minute (auth/rate.rs), and a sign-in per test took the suite past it (the 15b rerun:
// WEB-E2E-025's own sign-in was the 11th, refused with 429).
let session: Awaited<ReturnType<BrowserContext["storageState"]>>;
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const context = await browser.newContext({ baseURL });
  try {
    await bootstrap(await context.newPage());
    session = await context.storageState();
  } finally {
    await context.close();
  }
});
test.beforeEach(async ({ page }) => {
  expect(session, "beforeAll's sign-in, reused by every test").toBeDefined();
  await page.context().addCookies(session.cookies);
});

const RUN = randomUUID().slice(0, 8);
const writing = (baseURL: string) => ({
  "Idempotency-Key": randomUUID(),
  Origin: baseURL,
});

/** A new task on `route`, by the API; returns its id. */
async function created(
  page: Page,
  baseURL: string,
  title: string,
  route = "local",
) {
  const response = await page.request.post("/api/v1/conversations", {
    headers: writing(baseURL),
    data: { title, model_route: route },
  });
  expect(response.status()).toBe(201);
  return ((await response.json()) as { id: string }).id;
}

/**
 * A route of the stack's own model server, copied from `local` with another model name and display name, then
 * checked, so the pickers offer it. Returns a best-effort removal: a route a task still uses may be refused.
 */
async function routeLike(
  page: Page,
  baseURL: string,
  name: string,
  model: string,
  display: string,
) {
  const routes = (await (
    await page.request.get("/api/v1/admin/routes")
  ).json()) as { items: Record<string, unknown>[] };
  const local = routes.items.find((item) => item.name === "local");
  expect(local, "the stack's local route").toBeDefined();
  const keep = [
    "protocol",
    "base_url",
    "auth",
    "key_ref",
    "native_tools",
    "strict",
  ];
  const body = Object.fromEntries(
    Object.entries(local ?? {}).filter(([key]) => keep.includes(key)),
  );
  // A shared key goes only to the routes it's allowed for (the vault's rule): allow it for this one first, as an
  // administrator does with "Allow it for …", or the gateway refuses the route (409 secret_not_allowed).
  if (typeof body.key_ref === "string")
    await allowKey(page, baseURL, body.key_ref, name);
  const put = await page.request.put(`/api/v1/admin/routes/${name}`, {
    headers: writing(baseURL),
    data: { ...body, model, display_name: display },
  });
  expect(
    put.ok(),
    `create ${name}: ${put.status()} ${put.ok() ? "" : await put.text()}`,
  ).toBe(true);
  const { version } = (await put.json()) as { version: number };
  await page.request.post(`/api/v1/admin/routes/${name}/probe`, {
    headers: writing(baseURL),
    timeout: afterStart(60_000),
  });
  return async () => {
    await page.request.delete(
      `/api/v1/admin/routes/${name}?version=${version}`,
      {
        headers: writing(baseURL),
      },
    );
  };
}

/** Adds `route` to the routes a shared key may serve, keeping the ones it already has (the key's own version). */
async function allowKey(
  page: Page,
  baseURL: string,
  keyRef: string,
  route: string,
) {
  const listed = await page.request.get("/api/v1/admin/secrets");
  expect(
    listed.ok(),
    `shared keys: ${listed.status()} ${listed.ok() ? "" : await listed.text()}`,
  ).toBe(true);
  const { items } = (await listed.json()) as {
    items: {
      ref: string;
      version: number;
      consumers?: { kind: string; name: string }[];
    }[];
  };
  const key = items.find((item) => item.ref === keyRef);
  expect(key, `the shared key ${keyRef}`).toBeDefined();
  const bound = await page.request.put(
    `/api/v1/admin/secrets/${encodeURIComponent(keyRef.replace(/^secrets\//, ""))}/consumers`,
    {
      headers: writing(baseURL),
      data: {
        consumers: [...(key?.consumers ?? []), { kind: "route", name: route }],
        version: key?.version,
      },
    },
  );
  expect(
    bound.ok(),
    `allow ${keyRef} for ${route}: ${bound.status()} ${bound.ok() ? "" : await bound.text()}`,
  ).toBe(true);
}

/** Types `text` in the message box and sends it with Enter, as a person on a keyboard does. */
async function send(page: Page, text: string) {
  const box = page.getByRole("textbox", { name: "Message", exact: true });
  await box.fill(text);
  await box.press("Enter");
}

// A7 (WEB-U-218): Ctrl K finds a task by its title on the server's search, and Enter opens it.
test("WEB-E2E-050 Ctrl K finds a task by its title and opens it", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(1, 120_000));
  // Search shows once the list doesn't fit (eight tasks or more), so the case makes nine of its own.
  const ids: string[] = [];
  for (let n = 1; n <= 9; n += 1)
    ids.push(await created(page, baseURL, `Search target ${RUN} ${n}`));
  await page.goto("/");
  await expect(
    page.getByRole("link", { name: new RegExp(`^Search target ${RUN} 9`) }),
  ).toBeVisible({ timeout: afterStart(30_000) });
  await page.keyboard.press("Control+k");
  const search = page.getByRole("dialog", { name: "Search tasks" });
  await expect(search).toHaveCount(1);
  const box = search.getByRole("searchbox", { name: "Search tasks" });
  await expect(box).toBeFocused();
  await page.keyboard.type(`target ${RUN} 7`);
  const match = search.getByRole("link", {
    name: new RegExp(`^Search target ${RUN} 7`),
  });
  await expect(match).toBeVisible({ timeout: afterStart(30_000) });
  await match.click();
  await expect(page).toHaveURL(new RegExp(`/c/${ids[6]}$`));
  await expect(search).toHaveCount(0);
});

// A11 (WEB-U-273, WEB-U-084, WEB-U-089): the keyboard on a real run. Stop from the keyboard keeps the focus on the
// composer's button; Esc closes a menu and gives the focus back to what opened it, with the one focus ring showing.
test("WEB-E2E-051 Stop from the keyboard keeps the focus, and Esc closes a menu back to its button", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 180_000));
  const title = `Keyboard ${RUN}`;
  const id = await created(page, baseURL, title);
  await page.goto(`/c/${id}`);
  await send(page, "Hold this reply for the keyboard case.");
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeEnabled({ timeout: afterStart(60_000) });
  await stop.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
    timeout: afterStart(60_000),
  });
  // The same button, now Send, still has the focus: the keyboard didn't lose its place (WEB-U-273).
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeFocused();
  // The header's ⋯ (the sidebar's row has one of the same name).
  const more = page
    .getByRole("region", { name: "Task header" })
    .getByRole("button", { name: `More actions for ${title}`, exact: true });
  await more.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(more).toBeFocused();
  // The one focus ring (WEB-U-089): a keyboard focus is outlined, never left bare.
  const outline = await more.evaluate(
    (element) => getComputedStyle(element).outlineStyle,
  );
  expect(outline).not.toBe("none");
});

// T15 (WEB-U-304): a route whose model server answers 503 every time; the runtime retries, then the run fails. The
// retry's reasons are words, with the codes only after "Details:", and no code shows as the explanation.
test("WEB-E2E-052 a retry's reasons are words, the codes after Details", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  const name = `flaky-${RUN}`;
  const remove = await routeLike(
    page,
    baseURL,
    name,
    "flaky-503",
    `Flaky ${RUN}`,
  );
  try {
    const id = await created(page, baseURL, `Retries ${RUN}`, name);
    await page.goto(`/c/${id}`);
    await send(page, "Answer in one line.");
    await expect(page.getByLabel("Run status")).toHaveText(/^Failed/, {
      timeout: afterStart(180_000),
    });
    const transcript = page.getByTestId("transcript");
    const titled = transcript.locator("[title*='Details:']").first();
    await expect(titled).toHaveAttribute(
      "title",
      /^The model server answered with an error\. Details: http 503/,
    );
    // The code is never the explanation: the failure says what happened and what to do first, and the code comes after
    // its words as a muted part of the same line (WEB-U-016), as the fake front's failures.spec.ts reads it.
    const outcome = transcript.locator(".turn-outcome");
    await expect(outcome).toHaveText(
      /^The model provider refused the request\..* Code: http 503$/,
    );
    await expect(outcome.locator(".turn-code")).toHaveClass(/muted/);
  } finally {
    await remove();
  }
});

// N3 (WEB-U-018, WEB-U-103): the model chosen on New task is the one its task runs on. A second route of the stack's
// model server is chosen in the composer's picker; the task the message makes uses it.
test("WEB-E2E-054 the model chosen on New task is the task's", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  const name = `alt-${RUN}`;
  const remove = await routeLike(
    page,
    baseURL,
    name,
    "fixture",
    `Alternate ${RUN}`,
  );
  try {
    await page.goto("/");
    await chooseModel(page, name);
    await send(page, "Hold this reply, on the chosen model.");
    await expect(page).toHaveURL(/\/c\/[0-9a-f-]+$/, {
      timeout: afterStart(30_000),
    });
    const id = page.url().split("/c/")[1] ?? "";
    const task = (await (
      await page.request.get(`/api/v1/conversations/${id}`)
    ).json()) as { model_route: string };
    expect(task.model_route).toBe(name);
    const stop = page.getByRole("button", { name: "Stop", exact: true });
    await expect(stop).toBeEnabled({ timeout: afterStart(60_000) });
    await stop.click();
    await expect(page.getByLabel("Run status")).toHaveText(/^Stopped/, {
      timeout: afterStart(60_000),
    });
  } finally {
    await remove();
  }
});

// T21 (WEB-U-046): the Context tab shows what the latest run loaded. The first run writes AGENTS.md at the
// workspace's root; the second run loads it, and the tab lists it under Instructions.
test("WEB-E2E-053 the Context tab lists the instructions the run loaded", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 240_000));
  const id = await created(page, baseURL, `Context ${RUN}`);
  await page.goto(`/c/${id}`);
  const status = page.getByLabel("Run status");
  await send(page, "Write the context file at the root.");
  await expect(status).toHaveText(/^Done/, { timeout: afterStart(120_000) });
  await send(page, "Now follow it.");
  await expect(page.getByText("The rules are loaded.")).toBeVisible({
    timeout: afterStart(120_000),
  });
  await expect(status).toHaveText(/^Done/, { timeout: afterStart(60_000) });
  const toggle = page.getByRole("button", { name: "Files", exact: true });
  if ((await toggle.getAttribute("aria-expanded")) !== "true")
    await toggle.click();
  const panel = page.getByRole("complementary", { name: "Files" });
  await panel
    .getByRole("tablist", { name: "Task views" })
    .getByRole("tab", { name: "Context" })
    .click();
  const view = panel.getByRole("tabpanel", { name: "Context" });
  await expect(
    view
      .getByRole("region", { name: /^Instructions · / })
      .getByRole("listitem"),
  ).toContainText(["AGENTS.md"]);
});

/** Asks the driver to restart the gateway (demo_agents.Restarter, started for this suite) and waits until it has. */
async function restartGateway(reason: string) {
  const markers = process.env.TQA_TASK_PAGE_MARKERS ?? "";
  if (!markers) throw new Error("The driver must supply TQA_TASK_PAGE_MARKERS");
  await rm(join(markers, "restarted"), { force: true });
  await writeFile(join(markers, "restart-wanted"), reason);
  const bound = scaled(240_000);
  const started = Date.now();
  for (;;) {
    const failed = await readFile(
      join(markers, "restart-failed"),
      "utf8",
    ).catch(() => null);
    if (failed !== null)
      throw new Error(`The driver's restart failed: ${failed}`);
    if (await readFile(join(markers, "restarted"), "utf8").catch(() => null))
      return;
    if (Date.now() - started > bound)
      throw new Error(`No restart after ${bound / 1000} s`);
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

// T13 (WEB-U-234, WEB-U-304): a gateway restart while a step waits on the model. The page reconnects by itself, the
// step is asked again and finishes; the step's summary says the connection dropped, and its tooltip says why in words.
test("WEB-E2E-055 a gateway restart mid-step reconnects, starts the step over and says so", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(3, 360_000));
  const id = await created(page, baseURL, `Restart ${RUN}`);
  await page.goto(`/c/${id}`);
  const status = page.getByLabel("Run status");
  await send(page, "Restart while this step waits.");
  await expect(status).toHaveText(/^Working/, { timeout: afterStart(60_000) });
  await restartGateway("while a step waits on the model");
  await expect(
    page.getByText("The step started over and finished."),
  ).toBeVisible({
    timeout: afterStart(180_000),
  });
  await expect(status).toHaveText(/^Done/, { timeout: afterStart(60_000) });
  // Never a red banner for a restart (WEB-U-234): the page carried on by itself.
  // Where the app puts an alert, not the whole page: Next's route announcer is an empty role="alert" on every page.
  // The task's own (a failed run, a failed message) and the notifications' (an error toast is role="alert").
  await expect(
    page.getByRole("region", { name: "Task", exact: true }).getByRole("alert"),
  ).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Notifications" }).getByRole("alert"),
  ).toHaveCount(0);
  const summary = page.getByRole("button", { name: /Worked for/ });
  await expect(summary).toHaveText(/the connection dropped, retried once$/);
  await expect(summary.locator("[title]")).toHaveAttribute(
    "title",
    "The connection to the server dropped. Details: gateway_link",
  );
});
