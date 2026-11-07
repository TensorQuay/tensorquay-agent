import { writeFile } from "node:fs/promises";

import { expect, test, type Locator, type Page } from "@playwright/test";

import { answered } from "./answered";
import { pick } from "./choices";
import { chooseModel } from "./model";
import { axe, rowMenu } from "./settings-page";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-026: the Models and People functions the Settings round trip (WEB-E2E-025) leaves to this suite, on the real
// stack. Models: a key pasted while adding a route (stored as a shared secret, never shown again), List models against
// the stack's model server, and an edit of the address, the protocol and the Advanced fields (limits, tool settings,
// extra headers), each read back through the admin API, with List models then using the saved address. People: a
// person's page lists their task by its details, never its title (titles are content, SEC-044), and neither the page, its private
// view nor the admin's API reads hold the task's words. Every result value is a read; the driver (deploy/tests/demo.py,
// suite "models-people") checks the result file.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned models-people stack");

const RUN = Date.now().toString(36);
const MODEL_SERVER = "http://fake-model:8080/v1";
const result: Record<string, unknown> = {};
/** A read value with this run's random part named, so the driver can compare it. */
const named = (value: string | null) => value?.replaceAll(RUN, "RUN") ?? null;

async function saveResult() {
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify(result));
}
async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.ok(), `${path}: ${response.status()}`).toBe(true);
  return response.json() as Promise<T>;
}

type Route = {
  name: string;
  protocol: string;
  base_url: string;
  model: string;
  key_ref: string | null;
  concurrency: number | null;
  max_output: number | null;
  strict: string;
  schema_keywords: string;
  extra_headers: Record<string, string>;
};
async function route(page: Page, name: string) {
  const routes = await read<{ items: Route[] }>(page, "/api/v1/admin/routes");
  const found = routes.items.find((item) => item.name === name);
  expect(found, `route ${name}`).toBeDefined();
  return found as Route;
}

/**
 * Clicks List models and returns the gateway's answer: its status, the failure code if any, and the models. The gateway
 * allows one listing per route every little while (429 with Retry-After, while the form shows the last list), so a
 * listing that comes too soon waits as the gateway asks and lists again.
 */
async function listModels(page: Page, form: Locator, name: string) {
  const path = `/api/v1/admin/routes/${name}/models`;
  for (let attempt = 0; ; attempt += 1) {
    // Read as it arrives, and never an empty answer for one that can't be read (WEB-E2E-089).
    const response = await answered(page, path, () =>
      form.getByRole("button", { name: "List models" }).click(),
    );
    const wait = Number(response.headers["retry-after"] ?? "0");
    if (response.status === 429 && attempt < 2 && wait > 0 && wait <= 120) {
      await page.waitForTimeout(wait * 1_000 + 500);
      continue;
    }
    const body = response.body as {
      models?: { id: string }[];
      failure?: { code: string } | null;
    };
    return {
      status: response.status,
      failure: body.failure?.code ?? null,
      models: (body.models ?? []).map((model) => model.id),
    };
  }
}

/** Saves the route form; a key not yet allowed for the route is allowed from the form, then saved again. */
async function saveRoute(
  page: Page,
  form: Locator,
  name: string,
  display: string,
) {
  await form.getByRole("button", { name: "Save" }).click();
  const saved = page.getByText(`Saved route ${display}.`);
  const allow = form.getByRole("button", { name: `Allow it for ${name}` });
  await expect(saved.or(allow)).toBeVisible();
  if (await allow.isVisible()) {
    await allow.click();
    await expect(form.getByText("Allowed. Save again.")).toBeVisible();
    await form.getByRole("button", { name: "Save" }).click();
  }
  await expect(saved).toBeVisible();
}

async function editRoute(page: Page, display: string) {
  await page.getByRole("link", { name: `Edit route ${display}` }).click();
  return page.locator("form.settings-form-page");
}

test("WEB-E2E-026 Models: Paste key, List models and an edit of every group on the real stack", async ({
  page,
}) => {
  test.setTimeout(360_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  const name = `edit-${RUN}`;
  const display = `Edit ${RUN}`;
  const pasted = `synthetic-stack-key-${RUN}`;
  // Every API answer the page receives, to show the pasted key never comes back from the gateway.
  const bodies: Promise<string>[] = [];
  page.on("response", (response) => {
    // An event stream never ends, so its body is never awaited (a task page would open one).
    const type = response.headers()["content-type"] ?? "";
    if (
      new URL(response.url()).pathname.startsWith("/api/") &&
      !type.startsWith("text/event-stream")
    )
      bodies.push(response.text().catch(() => ""));
  });

  // Added with a key pasted in the form: stored as a shared secret, never shown back (WEB-U-056).
  await page.goto("/settings/models");
  await page.getByRole("link", { name: "Add route" }).click();
  const form = page.locator("form.settings-form-page");
  await form.locator("summary", { hasText: "Advanced" }).click();
  await form.getByLabel("Route name").fill(name);
  await form.getByLabel("Display name").fill(display);
  await form.getByLabel("Address").fill(MODEL_SERVER);
  await form.getByLabel("Model", { exact: true }).fill("to-be-listed");
  await form
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  await form.getByRole("button", { name: "Paste key" }).click();
  const panel = form.getByRole("group", { name: "Paste a key" });
  await expect(panel.getByLabel("Secret name")).toHaveValue(`${name}-key`);
  await panel.getByLabel("Key", { exact: true }).fill(pasted);
  await panel.getByRole("button", { name: "Store key" }).click();
  await expect(
    form.getByText(`Stored the key as ${name}-key. Save the route to use it.`),
  ).toBeVisible();
  await saveRoute(page, form, name, display);
  const added = await route(page, name);
  const secrets = await read<{ items: { ref: string }[] }>(
    page,
    "/api/v1/admin/secrets",
  );
  result.pasteKey = {
    keyRef: named(added.key_ref),
    stored: secrets.items
      .map((item) => item.ref)
      .filter((stored) => stored.endsWith(`${name}-key`))
      .map(named),
    shown: (await page.content()).includes(pasted),
  };

  // List models asks the stack's model server with the saved address and key; picking one fills Model (WEB-U-067).
  let edit = await editRoute(page, display);
  result.listModels = await listModels(page, edit, name);
  const listed = edit.getByRole("button", { name: /^fixture/ });
  await expect(listed).toBeVisible();
  await axe(page);
  await listed.click();
  await expect(edit.getByLabel("Model", { exact: true })).toHaveValue(
    "fixture",
  );
  await saveRoute(page, edit, name, display);
  result.picked = (await route(page, name)).model;

  // An edit of the address, the protocol and the Advanced fields: limits, tool settings, extra headers (WEB-U-039,
  // WEB-U-057). The address points nowhere, so List models then fails with the saved address.
  edit = await editRoute(page, display);
  await edit.getByRole("button", { name: "Change the address" }).click();
  await edit.getByLabel("Address").fill("http://127.0.0.1:9/v1");
  await edit.locator("summary", { hasText: "Advanced" }).click();
  await pick(page, "Protocol", "Responses", edit);
  await edit.getByLabel("Concurrency").fill("2");
  await edit.getByLabel("Max output").fill("1024");
  await pick(page, "Strict tool schemas", "Never", edit);
  await pick(page, "Schema keywords", "Basic only", edit);
  await edit
    .getByLabel("Extra headers, one per line as Name: value")
    .fill("X-Stack-Check: yes");
  await axe(page);
  await saveRoute(page, edit, name, display);
  const changed = await route(page, name);
  result.edited = {
    base_url: changed.base_url,
    protocol: changed.protocol,
    concurrency: changed.concurrency,
    max_output: changed.max_output,
    strict: changed.strict,
    schema_keywords: changed.schema_keywords,
    extra_headers: changed.extra_headers,
    key_ref: named(changed.key_ref),
  };
  await expect(
    page.getByRole("row", { name: new RegExp(display) }),
  ).toContainText("Responses");
  // Nothing listens at the saved address: the gateway reports it unreachable, and the form says so.
  edit = await editRoute(page, display);
  result.listAfterEdit = await listModels(page, edit, name);
  await expect(
    edit.getByText("The server could not be reached."),
  ).toBeVisible();
  await edit.getByRole("button", { name: "Cancel" }).click();

  await rowMenu(
    page,
    `More actions for route ${display}`,
    `Delete route ${display}`,
  );
  await page
    .getByRole("dialog", { name: `Delete route ${display}?` })
    .getByRole("button", { name: "Delete route" })
    .click();
  await expect(page.getByText(`Deleted route ${display}.`)).toBeVisible();
  const left = await read<{ items: Route[] }>(page, "/api/v1/admin/routes");
  result.deleted = !left.items.some((item) => item.name === name);
  // The pasted key stays under Keys › Shared, as the form says; Keys belong to WEB-E2E-025, and the stack is thrown away.
  result.keyInResponses = (await Promise.all(bodies)).some((body) =>
    body.includes(pasted),
  );
  await saveResult();
});

test("WEB-E2E-026 People: a person's page lists their task by its details, never its content", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(budget(1, 150_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  const email = `owner-${RUN}@example.invalid`;
  // The task's first message (its title) and a follow-up: both are content, which an administrator never sees.
  const firstWords = `Person page task ${RUN}`;
  const secretWords = `private follow-up ${RUN}`;
  await page.goto("/settings/people");
  await page.getByRole("textbox", { name: "Email" }).fill(email);
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  const invited = page.getByRole("dialog", { name: "Invitation created" });
  const link = await invited
    .getByRole("textbox", { name: "Invitation link" })
    .inputValue();
  await invited.getByRole("button", { name: "I’ve copied it" }).click();

  // The person joins and starts a task of their own.
  const member = await browser.newContext({ baseURL });
  const guest = await member.newPage();
  await guest.goto(new URL(link).pathname);
  await guest
    .getByLabel("Password", { exact: true })
    .fill(`synthetic-member-${RUN}`);
  await guest.getByRole("button", { name: "Join" }).click();
  await expect(
    guest.getByRole("heading", { name: "What should the agent do?" }),
  ).toBeVisible();
  // The stack's own route, whatever other routes an earlier step left.
  await chooseModel(guest, "local");
  const message = guest.getByLabel("Message", { exact: true });
  await message.fill(firstWords);
  await message.press("Enter");
  await expect(guest).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  const task = new URL(guest.url()).pathname.split("/").at(-1) ?? "";
  await expect(guest.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  const followUp = await guest.request.post(
    `/api/v1/conversations/${task}/messages`,
    {
      data: { text: secretWords },
      headers: {
        Origin: new URL(guest.url()).origin,
        "Idempotency-Key": `follow-up-${RUN}`,
      },
    },
  );
  expect(followUp.status()).toBe(202);
  // The owner reads it in the task's history.
  await expect
    .poll(
      async () =>
        (
          await guest.request.get(`/api/v1/conversations/${task}/events`)
        ).text(),
      {
        timeout: 120_000,
      },
    )
    .toContain(secretWords);
  await member.close();

  // The People list links to the person's page, which lists the task by its details; its created date opens it.
  await page.reload();
  const row = page.getByRole("row", {
    name: new RegExp(email.replace(/[.]/g, "\\.")),
  });
  await row.getByRole("link", { name: "1 task" }).click();
  await expect(page).toHaveURL(/\/settings\/people\/[0-9a-f-]{36}$/);
  await expect(
    page.getByRole("heading", { name: `Tasks of ${email}` }),
  ).toBeVisible();
  await expect(
    page.getByText("Administrators see a task’s details, never its content."),
  ).toBeVisible();
  const tasks = page.getByRole("table", { name: `Tasks of ${email}` });
  await expect(tasks.getByRole("row")).toHaveCount(2);
  await expect(tasks.getByRole("link")).toHaveCount(1);
  await expect(tasks.getByText("Private task")).toHaveCount(0);
  await expect(tasks).toContainText("Active");
  await axe(page);
  const personPage = await page.content();

  // Its private view names the owner and shows no content; the API refuses the content to the administrator too.
  await tasks.getByRole("link").first().click();
  await expect(page).toHaveURL(new RegExp(`/c/${task}$`));
  await expect(
    page.getByText(
      `This task belongs to ${email}. Administrators can see its details, not its content.`,
    ),
  ).toBeVisible();
  await axe(page);
  const privateView = await page.content();
  // The admin's reads: the task itself and every task (what the person's page lists) carry no title; the history is
  // refused.
  const own = await page.request.get(`/api/v1/conversations/${task}`);
  const detail = (await own.json()) as Record<string, unknown>;
  const all = await read<{ items: { id: string; title?: string }[] }>(
    page,
    "/api/v1/conversations?all=true",
  );
  const listedTask = all.items.find((item) => item.id === task);
  const events = await page.request.get(`/api/v1/conversations/${task}/events`);
  const shown = (words: string) =>
    [personPage, privateView].some((html) => html.includes(words));
  result.person = {
    listed: listedTask !== undefined,
    listedTitle: listedTask ? "title" in listedTask : null,
    readStatus: own.status(),
    readTitle: "title" in detail,
    titleShown: shown(firstWords),
    followUpShown: shown(secretWords),
    eventsStatus: events.status(),
  };
  await saveResult();
});
