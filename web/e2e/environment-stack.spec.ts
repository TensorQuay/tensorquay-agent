import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test } from "@playwright/test";

import { openCard } from "./cards";
import { chooseEnvironment, confirmEnvironment } from "./environment";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned environment demo stack",
);

test("E2E-ENV-010 retained file, pytest and headless browser after a switch", async ({
  page,
}) => {
  test.setTimeout(budget(2, 60_000));
  await bootstrap(page);
  await chooseEnvironment(page, "Minimal");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(
    "Create a small test file and keep it for the next environment.",
  );
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/([0-9a-f-]{36})$/);
  const conversationId = new URL(page.url()).pathname.split("/").at(-1);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await expect(
    page.getByText(/Created environment\/test_persisted.py in Minimal/),
  ).toBeVisible();
  await chooseEnvironment(page, "Workstation");
  await confirmEnvironment(page, "Workstation");
  await expect(
    page.getByText("Switched to Workstation", { exact: true }),
  ).toHaveCount(0);
  await input.fill(
    "Use the new Python and browser capabilities: run pytest on that file, then screenshot a local page.",
  );
  await input.press("Enter");
  await expect(
    page.getByText("Switched to Workstation", { exact: true }),
  ).toHaveCount(1);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await expect(page.locator(".markdown-answer").last()).toContainText(
    "Workstation's Python and browser tools ran pytest",
  );
  // The agent's requests are all to local pages, so a refused request would be the browser's own background call. The
  // image's managed policy (images/runtime/chromium-policy.json) and the switches file Debian's wrapper sources
  // (images/runtime/chromium-quiet-flags.sh) stop every one a probe of the image found, on a network like a sandbox's:
  // network time, the search provider, sign-in and the account checks, push messaging, updates, the optimization guide
  // and Safe Browsing. So none may show.
  const notes = await page
    .getByRole("region", { name: "Transcript" })
    .getByText(/^Blocked network access to /)
    .allTextContents();
  const hosts = notes.map(
    (note) => /^Blocked network access to ([^:]+):/.exec(note)?.[1] ?? note,
  );
  expect(hosts).toEqual([]);
  await page.locator(".activity-heading").last().click();
  await expect(
    page.getByRole("button", {
      name: /^Created environment\/(index\.html|browser\.cjs)/,
    }),
  ).toHaveCount(2);
  // A command is named on its line, and its card (the line's own list item) holds the output. The latest line opens
  // with its group (finding T2), so each is opened only if it is closed.
  const result = async (command: RegExp) => {
    const line = page.getByRole("button", { name: command });
    await openCard(line);
    return page
      .getByRole("listitem")
      .filter({ has: line })
      .getByRole("article", { name: "shell tool call" })
      .getByLabel("Result");
  };
  await expect(await result(/^Ran a command: python3 -m pytest/)).toContainText(
    "1 passed",
  );
  await expect(
    await result(/^Ran a command: node environment\/browser/),
  ).toContainText("Saved environment/page.png");
  const commands = page.getByRole("article", { name: "shell tool call" });
  for (const command of await commands.all())
    // Finding A5: a command that succeeded says nothing of its exit code; one that failed would say so.
    await expect(command.getByText(/Didn't succeed/)).toHaveCount(0);
  await page.locator(".activity-heading").last().click();
  await page
    .getByText("Switched to Workstation", { exact: true })
    .scrollIntoViewIfNeeded();
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (directory) {
    await mkdir(directory, { recursive: true });
    for (const colorScheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme });
      await page.screenshot({
        path: join(directory, `environment-stack-${colorScheme}.png`),
      });
    }
  }
  await page.reload();
  await expect(
    page.getByText("Switched to Workstation", { exact: true }),
  ).toHaveCount(1);
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ conversationId }),
      { mode: 0o600 },
    );
});

// E2E-ENV-012: the Environment page on the real stack (WEB-U-097, GW-U-093). A person adds an environment from the
// Workstation card, typing only a display name; meets the plain refusal for the reserved ID "new", from the page and
// from the real gateway; edits it and saves with the pinned bar; then starts a task in it, which runs there.
test("E2E-ENV-012 an environment added on its page runs a task", async ({
  page,
}) => {
  test.setTimeout(budget(1, 210_000));
  const evidence = process.env.TQA_WEB_EVIDENCE_DIR;
  const shot = async (name: string) => {
    if (!evidence) return;
    await mkdir(evidence, { recursive: true });
    await page.screenshot({ path: join(evidence, `${name}.png`) });
  };
  await bootstrap(page);
  await page.goto("/settings/environments");
  await page.getByRole("link", { name: "Add environment" }).click();
  await expect(page).toHaveURL(/\/settings\/environments\/new$/);
  const form = page.locator("form.settings-form-page");

  // (1) The Workstation card, from the engine's own images, and only a display name typed: the ID follows it.
  const cards = form.getByRole("radiogroup", { name: "Start from a built-in" });
  await cards.getByText("Workstation", { exact: true }).click();
  await expect(
    cards.getByRole("radio", { name: /^Workstation/ }),
  ).toBeChecked();
  // WEB-U-184: a built-in's tools are its own, said once.
  const tools = form.getByRole("region", { name: "Capabilities" });
  await expect(tools).toContainText("Set by Workstation:");
  await expect(tools).toContainText("Browser");
  await form.getByLabel("Display name", { exact: true }).fill("Data tools");
  const basics = form.getByRole("region", { name: "Environment" });
  await expect(basics).toContainText("ID: data-tools · Change");
  await shot("environment-012-add");

  // (2) The reserved ID: the page refuses it in plain words and sends nothing...
  await form.getByRole("button", { name: "Change the ID" }).click();
  const id = form.getByLabel("ID", { exact: true });
  await expect(id).toBeFocused();
  await id.fill("new");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    form.getByText("This ID is reserved for the Add page. Choose another."),
  ).toBeVisible();
  await shot("environment-012-reserved");
  // ...and the real gateway refuses it too, on the name field with its own reason, for a write that skips the page.
  const image = await form.getByLabel("Image ID", { exact: true }).inputValue();
  expect(image).toMatch(/^sha256:[0-9a-f]{64}$/);
  const refused = await page.request.put("/api/v1/admin/templates/new", {
    // A cookie's write carries its page's origin, as the browser's own would (the gateway refuses one without).
    headers: {
      "idempotency-key": `e2e-env-012-${Date.now()}`,
      origin: new URL(page.url()).origin,
    },
    data: {
      display_name: "New",
      description: null,
      image,
      capabilities: ["shell"],
      enabled: true,
    },
  });
  expect(refused.status()).toBe(400);
  expect(
    ((await refused.json()) as { error: { details: unknown[] } }).error.details,
  ).toEqual([{ field: "name", reason: "name_reserved" }]);
  await id.fill("data-tools");
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page).toHaveURL(/\/settings\/environments$/);
  await expect(
    page.getByText("Saved environment Data tools. The gateway checks it next."),
  ).toBeVisible();
  // The gateway's own check of the image; the list asks again on its own.
  const row = page.getByRole("row", { name: /^Data tools/ });
  await expect(row).toContainText("Verified", { timeout: 240_000 });

  // (3) Edit it, and save with the bar pinned flush on the window's edge.
  await page.getByRole("link", { name: "Edit environment Data tools" }).click();
  await expect(
    form.getByRole("heading", {
      level: 2,
      name: "Edit environment Data tools",
    }),
  ).toBeVisible();
  await form
    .getByLabel("Description", { exact: true })
    .fill("Workstation tools for data work");
  const bar = form.getByRole("group", { name: "Save or cancel" });
  // The Save bar holds two status lines: this one, and the count of fields to fix (WEB-U-110).
  await expect(bar.getByText("Unsaved changes")).toHaveRole("status");
  const box = await bar.boundingBox();
  expect(
    Math.round((box?.y ?? 0) + (box?.height ?? 0)),
    "the bar sits on the window's bottom edge",
  ).toBe(page.viewportSize()?.height);
  await shot("environment-012-edit");
  await bar.getByRole("button", { name: "Save" }).click();
  await expect(
    page.getByText("Saved environment Data tools. The gateway checks it next."),
  ).toBeVisible();
  await expect(row).toContainText("Workstation tools for data work");
  await expect(row).toContainText("Verified", { timeout: 240_000 });

  // (4) A task in it: picked before the first message, and it runs there.
  await page.goto("/");
  await chooseEnvironment(page, "Data tools");
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Run one command in this environment.");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/([0-9a-f-]{36})$/);
  const task = new URL(page.url()).pathname.split("/").at(-1);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await expect(
    page.getByText("The command ran in this task's environment."),
  ).toBeVisible();
  // The run's steps are folded under its activity heading, as in E2E-ENV-010: open it, then the command's card.
  await page.locator(".activity-heading").last().click();
  const line = page.getByRole("button", {
    name: /^Ran a command: uname -s && python3 --version/,
  });
  await openCard(line);
  await expect(
    page
      .getByRole("listitem")
      .filter({ has: line })
      .getByRole("article", { name: "shell tool call" })
      .getByLabel("Result"),
  ).toContainText("Linux");
  // The gateway's record: the task is in the environment the page added.
  const record = await page.request.get(`/api/v1/conversations/${task}`);
  expect(record.status()).toBe(200);
  expect(((await record.json()) as { template: string }).template).toBe(
    "data-tools",
  );
  await shot("environment-012-task");
});
