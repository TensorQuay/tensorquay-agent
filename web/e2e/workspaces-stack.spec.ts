import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { addFromPlus } from "./choices";
import { settled } from "./motion";
import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";
import { auditView, type Finding } from "./ux/polish-audit";

// WEB-E2E-011: on the real stack, a task on an existing workspace holds its lease while its scripted model
// call is held (the workspaces fixture); a fork of that workspace runs at once in its own copy; a second task on the
// workspace waits for the lease and runs when the first is stopped. demo_workspaces.py then checks both workspaces on
// disk: the stopped task wrote no mark.
test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned workspaces demo stack",
);
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const NAME = "Team handbook";
const COPY = "Team handbook (copy)";
const MEMBER = "member@example.invalid";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(
    page.locator(".skeleton:not(.workspace-copying .skeleton)"),
  ).toHaveCount(0);
  await settled(page);
  await mkdir(directory, { recursive: true });
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await page.screenshot({
      path: join(directory, `stack-workspaces-${name}-${colorScheme}.png`),
    });
  }
  await page.emulateMedia({ colorScheme: "light" });
}

/** A workspace's row in the page's table (its cards are for a phone), as the fake-front workspaces spec finds it. */
const card = (page: Page, name: string) =>
  page
    .getByRole("main")
    .getByRole("row")
    .filter({
      has: page.locator(".data-name > span").getByText(name, { exact: true }),
    });

/** Chooses one of a workspace's actions from its row's menu (Open, New task here, Make a copy, Share, …). */
async function choose(page: Page, name: string, action: string) {
  await page
    .getByRole("main")
    // Exact: "Team handbook" must not also match "Team handbook (copy)".
    .getByRole("button", { name: `More actions for ${name}`, exact: true })
    .click();
  await page.getByRole("menuitem", { name: action, exact: true }).click();
}

/** New task names the workspace a task will use on its chip in the composer (the UI standard's composer). */
function chosenWorkspace(page: Page, name: string) {
  return page
    .getByRole("form", { name: "Message composer" })
    .getByRole("button", { name: `Workspace: ${name}`, exact: true });
}

/** A task on a named workspace says which on the quiet line under its title: "In <name>". */
function inWorkspace(page: Page, name: string) {
  return page
    .getByRole("region", { name: "Task header" })
    .getByText(`In ${name}`, { exact: true });
}

async function workspaceId(page: Page, name: string) {
  const response = await page.request.get("/api/v1/workspaces");
  expect(response.status()).toBe(200);
  const { items } = (await response.json()) as {
    items: { id: string; name?: string }[];
  };
  const found = items.find((item) => item.name === name);
  if (!found) throw new Error(`No workspace named ${name}`);
  return found.id;
}

/** Sends the first prompt from the start page and returns the new task's path. */
async function start(page: Page, prompt: string) {
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill(prompt);
  await expect(page.getByRole("button", { name: "Send" })).toBeEnabled({
    timeout: 30_000,
  });
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  return new URL(page.url()).pathname;
}

async function done(page: Page) {
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await expect(
    page.getByText("The run was recorded in runs.txt."),
  ).toBeVisible();
}

test("WEB-E2E-011 a shared workspace takes turns, and a fork runs at once in its own copy", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(budget(5, 60_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  const order: string[] = [];

  // A member to share with, invited and joined through the public API.
  const invited = await page.request.post("/api/v1/invitations", {
    headers: { Origin: baseURL },
    data: { email: MEMBER, role: "member" },
  });
  expect(invited.status()).toBe(201);
  const { token } = (await invited.json()) as { token: string };
  const member = await browser.newContext({ baseURL });
  const guest = await member.newPage();
  await guest.setViewportSize({ width: 1440, height: 900 });
  const joined = await guest.request.post("/api/v1/invitations/accept", {
    headers: { Origin: baseURL },
    data: { token, password: `synthetic-member-${Date.now()}` },
  });
  expect(joined.ok()).toBe(true);

  // The page starts empty; a workspace is made by name.
  await page.goto("/workspaces");
  await expect(
    page.getByText("A workspace keeps a task’s files."),
  ).toBeVisible();
  await capture(page, "empty");
  await page.getByRole("button", { name: "New workspace" }).click();
  const naming = page.getByRole("dialog", { name: "New workspace" });
  await naming.getByRole("textbox", { name: "Name" }).fill(NAME);
  await naming.getByRole("button", { name: "Create" }).click();
  await expect(card(page, NAME)).toBeVisible();

  // Shared with the member by exact email.
  await choose(page, NAME, "Share");
  const share = page.getByRole("dialog", { name: `Share “${NAME}”` });
  await share
    .getByRole("textbox", { name: "Email" })
    .fill("nobody@example.invalid");
  await share.getByRole("button", { name: "Share", exact: true }).click();
  await expect(share.getByText("No account uses that email.")).toBeVisible();
  await share.getByRole("textbox", { name: "Email" }).fill(MEMBER);
  // Can view is the default (WEB-U-173); this member is to start tasks on it.
  await share.getByRole("button", { name: "Access: Can view" }).click();
  await page.getByRole("menuitemradio", { name: /Can edit/ }).click();
  await share.getByRole("button", { name: "Share", exact: true }).click();
  await expect(share.getByText(MEMBER)).toBeVisible();
  await settled(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await capture(page, "share");
  await share.getByRole("button", { name: "Close dialog" }).click();
  // The row's people mark says it in words for assistive technology.
  await expect(
    card(page, NAME).getByText("Shared with 1 person"),
  ).toBeAttached();

  // The first task on the existing workspace: its model call is held, so the run keeps the lease.
  await choose(page, NAME, "New task here");
  await expect(chosenWorkspace(page, NAME)).toBeVisible();
  const first = await start(page, "Record the first run in the handbook");
  // Once started, the task says which workspace it is in.
  await expect(inWorkspace(page, NAME)).toBeVisible();
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/, {
    timeout: afterStart(30_000),
  });
  order.push("first");

  await page.goto("/workspaces");
  const held = card(page, NAME).locator(".workspace-lease");
  await expect(held).toHaveText(/^In use by “Record the first run/);
  await expect(held).toHaveAttribute("data-lease", "running");
  await settled(page);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await capture(page, "page");
  await page.setViewportSize({ width: 1024, height: 900 });
  await capture(page, "page-rail");
  await page.setViewportSize({ width: 1440, height: 900 });

  // The member sees it shared, in use by a task that is not theirs.
  await guest.goto("/workspaces");
  const theirs = card(guest, NAME);
  // Access to a workspace shared with you is in its owner's cell: "<owner> · Can edit".
  await expect(theirs.getByText(/· Can edit$/)).toBeVisible();
  await expect(theirs.locator(".workspace-lease")).toHaveText(
    "In use by another task",
  );
  await capture(guest, "member");
  await member.close();

  // A fork runs at once in its own copy, while the first task still holds the workspace.
  await choose(page, NAME, "Make a copy");
  await page
    .getByRole("dialog", { name: `Copy “${NAME}”` })
    .getByRole("button", { name: "Make a copy", exact: true })
    .click();
  await expect(card(page, COPY)).toBeVisible();
  await capture(page, "fork");
  // The copy's notice offers to start a task in it.
  await page.getByRole("button", { name: "Start a task in the copy" }).click();
  await expect(chosenWorkspace(page, COPY)).toBeVisible();
  const forked = await start(page, "Record a run in the copy");
  await expect(inWorkspace(page, COPY)).toBeVisible();
  await done(page);
  order.push("fork");

  // A second task on the held workspace waits for it.
  await page.goto("/");
  // "+" adds the workspace chip, its picker open (the UI standard's composer).
  await addFromPlus(page, "Workspace…");
  const menu = page.getByRole("menu", { name: "Workspace" });
  await expect(
    menu.getByRole("menuitemradio", {
      // The name and its detail are two spans; the computed name may join them without a space.
      name: new RegExp(`^${NAME}\\s*You · In use`),
    }),
  ).toBeVisible();
  await capture(page, "picker");
  await menu
    .getByRole("menuitemradio", { name: new RegExp(`^${NAME}\\s*You`) })
    .click();
  await expect(
    page.getByText("In use: your task will wait for it."),
  ).toBeVisible();
  const second = await start(page, "Record the second run in the handbook");
  await expect(page.getByLabel("Run status")).toHaveText(
    "Waiting for the workspace",
    { timeout: 60_000 },
  );
  await capture(page, "waiting");

  // The first is stopped before it writes; the second then runs on the same workspace.
  await page.goto(first);
  // Stop acts at once (c9), with nothing to confirm.
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText("Stopped");
  await page.goto(second);
  await done(page);
  order.push("second");
  expect(forked).not.toBe(first);

  // The task list names each task's workspace; Open filters it.
  const tasks = page.getByRole("region", { name: "Tasks" });
  // The task list names the task's workspace on hover, then its time (UI-GUIDELINES §7, WEB-U-070).
  const escapedCopy = COPY.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  await expect(
    tasks.getByRole("link", { name: /Record a run in the copy/ }),
  ).toHaveAttribute(
    "title",
    new RegExp(`^Record a run in the copy · ${escapedCopy} · `),
  );
  await page.goto("/workspaces");
  await choose(page, NAME, "Open");
  await expect(
    page.getByRole("button", { name: `Remove filter Workspace: ${NAME}` }),
  ).toBeVisible();
  await expect(tasks.getByRole("link")).toHaveCount(2);

  // UX invariants on the page at the three designed widths in both themes (WEB-E2E-018's rules).
  const findings: Finding[] = [];
  for (const width of [1024, 1440, 1920])
    for (const colorScheme of ["light", "dark"] as const) {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await page.goto("/workspaces");
      await expect(card(page, COPY)).toBeVisible();
      await expect(page.locator(".skeleton")).toHaveCount(0);
      expect(
        await page.evaluate(() => ({
          width: document.documentElement.scrollWidth,
          height: document.documentElement.scrollHeight,
        })),
        `layout: the page fits ${width} px`,
      ).toEqual(page.viewportSize());
      await settled(page);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      findings.push(
        // The page under test only: the shared sidebar has its own layout audit.
        ...(await auditView(
          page,
          `workspaces-${width}-${colorScheme}`,
          "main",
        )),
      );
    }
  expect(findings, "UI-GUIDELINES §5b / §10 findings").toEqual([]);

  const result = {
    workspace: await workspaceId(page, NAME),
    fork: await workspaceId(page, COPY),
    order,
  };
  if (process.env.TQA_STACK_RESULT)
    await writeFile(process.env.TQA_STACK_RESULT, JSON.stringify(result), {
      mode: 0o600,
    });
});
