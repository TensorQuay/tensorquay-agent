import { execFile } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { settled } from "./motion";
import { addSharedKey } from "./settings-page";
import { bootstrap } from "./ux/flow";

const run = promisify(execFile);

test.skip(!process.env.TQA_STACK_URL, "Requires the owned accounts stack");
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await settled(page);
  await mkdir(directory, { recursive: true });
  for (const width of [1024, 1440])
    for (const colorScheme of ["light", "dark"] as const) {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await page.screenshot({
        path: join(directory, `stack-${name}-${width}-${colorScheme}.png`),
      });
    }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.emulateMedia({ colorScheme: "light" });
}

/** A program's view of the API: curl with the token as a Bearer header, no cookie, no Origin. */
async function curl(origin: string, token: string) {
  const { stdout } = await run(
    "curl",
    [
      "--silent",
      "--output",
      "/dev/null",
      "--write-out",
      "%{http_code}",
      "--max-time",
      "20",
      "--header",
      `Authorization: Bearer ${token}`,
      `${origin}/api/v1/conversations`,
    ],
    { timeout: 30_000 },
  );
  return Number(stdout);
}

// FR-26 / GW-I-002 / GW-I-023: invite -> accept -> the member's token works with curl -> revoked -> refused.
test("WEB-E2E-020 team accounts on the real stack", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(180_000);
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  // The old address leads to People (WEB-U-075).
  await page.goto("/settings/users");
  await expect(page).toHaveURL(/\/settings\/people$/);
  await page
    .getByRole("textbox", { name: "Email" })
    .fill("person@example.invalid");
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  const invited = page.getByRole("dialog", { name: "Invitation created" });
  const link = await invited
    .getByRole("textbox", { name: "Invitation link" })
    .inputValue();
  expect(new URL(link).pathname).toMatch(/^\/join\/[A-Za-z0-9_-]+$/);
  await capture(page, "invited");
  await invited.getByRole("button", { name: "I’ve copied it" }).click();

  const member = await browser.newContext({ baseURL });
  const guest = await member.newPage();
  await guest.setViewportSize({ width: 1440, height: 900 });
  await guest.goto(new URL(link).pathname);
  await expect(guest.getByRole("textbox", { name: "Email" })).toHaveValue(
    "person@example.invalid",
  );
  await capture(guest, "join");
  await guest
    .getByLabel("Password", { exact: true })
    .fill(`synthetic-member-${Date.now()}`);
  await guest.getByRole("button", { name: "Join" }).click();
  await expect(
    guest.getByRole("heading", { name: "What should the agent do?" }),
  ).toBeVisible();

  await guest.goto("/settings/tokens");
  await guest.getByRole("textbox", { name: "Name" }).fill("Stack check");
  await guest.getByRole("button", { name: "Create token" }).click();
  const created = guest.getByRole("dialog", { name: "Token created" });
  const token = await created
    .getByRole("textbox", { name: "Token" })
    .inputValue();
  expect(token).toMatch(/^tqa_pat_[A-Za-z0-9_-]{43}$/);
  await created.getByRole("button", { name: "I’ve copied it" }).click();
  const before = await curl(baseURL, token);
  await expect(guest.getByRole("row", { name: /Stack check/ })).toBeVisible();
  await capture(guest, "tokens");
  expect((await new AxeBuilder({ page: guest }).analyze()).violations).toEqual(
    [],
  );

  // Revoke is under the row's ⋯ (WEB-U-191).
  await guest
    .getByRole("button", { name: "More actions for the token “Stack check”" })
    .click();
  await guest
    .getByRole("menuitem", { name: "Revoke token “Stack check”" })
    .click();
  await guest
    .getByRole("dialog", { name: "Revoke this token?" })
    .getByRole("button", { name: "Revoke token" })
    .click();
  await expect(guest.getByText("No tokens yet.")).toBeVisible();
  const after = await curl(baseURL, token);
  await member.close();

  await page.reload();
  // The person is listed once: People no longer lists an accepted invitation (WEB-U-023).
  await expect(
    page.getByRole("row", { name: /person@example\.invalid/ }),
  ).toHaveCount(1);
  await capture(page, "users");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

  expect({ before, after }).toEqual({ before: 200, after: 401 });
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ bearer: [before, after], joined: true }),
      { mode: 0o600 },
    );
});

// FR-21, FR-22, FR-28: the administration pages against the real gateway's admin API.
test("WEB-E2E-022 status, secrets and usage on the real stack", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  await page.goto("/settings/status");
  await expect(
    page.getByText("New tasks can start.", { exact: true }),
  ).toBeVisible();
  await page.goto("/settings/keys/shared");
  await expect(page.getByRole("table", { name: "Shared keys" })).toBeVisible();
  const name = "stack-check";
  const value = `synthetic-${Date.now()}`;
  await addSharedKey(page, name, value);
  await expect(page.getByText(`Added shared key ${name}.`)).toBeVisible();
  await page
    .getByRole("button", { name: `Replace shared key ${name}` })
    .click();
  const dialog = page.getByRole("dialog", {
    name: `Replace shared key ${name}`,
  });
  await dialog.getByLabel("New value").fill(`${value}-2`);
  await dialog.getByRole("button", { name: "Replace key" }).click();
  await expect(page.getByText(`Updated shared key ${name}.`)).toBeVisible();
  await page
    .getByRole("button", { name: `More actions for shared key ${name}` })
    .click();
  await page
    .getByRole("menuitem", { name: `Delete shared key ${name}` })
    .click();
  await page
    .getByRole("dialog", { name: `Delete shared key ${name}?` })
    .getByRole("button", { name: "Delete key" })
    .click();
  await expect(page.getByText(`Deleted shared key ${name}.`)).toBeVisible();
  expect(await page.content()).not.toContain(value);
  expect(await new AxeBuilder({ page }).analyze()).toMatchObject({
    violations: [],
  });
  await page.goto("/settings/usage");
  await expect(
    page
      .getByRole("region", { name: "Total" })
      .or(page.getByText("No model requests in this period.")),
  ).toBeVisible();
  await capture(page, "admin-usage");
});

// ADR-0037 §7: a key for a Git host is a key for a web service, added, replaced and deleted through the page against the
// real gateway; the value never comes back.
test("WEB-E2E-023 a key for a Git host on the real stack", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  await page.goto("/settings/keys");
  const key = `synthetic-${Date.now()}`;
  const name = "CODE_STACK_CHECK";
  await page.getByRole("link", { name: "Add a key for a web service" }).click();
  await page
    .getByLabel("Web address", { exact: true })
    .fill("https://code.example.invalid/team/stack-check");
  await page.getByLabel("Name", { exact: true }).fill(name);
  await page.getByRole("radio", { name: /^With a user name/ }).check();
  await page.getByLabel("User name", { exact: true }).fill("builder");
  await page.getByLabel("Key", { exact: true }).fill(key);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(`Saved ${name}.`)).toBeVisible();
  const row = page.getByRole("row", { name: new RegExp(name) });
  await expect(row).toContainText("code.example.invalid/team/stack-check");
  const listed = await page.request.get("/api/v1/host-keys");
  expect(await listed.text()).not.toContain(key);

  await row.getByRole("link", { name }).click();
  await page.getByRole("button", { name: "Replace…" }).click();
  await page.getByLabel("New key", { exact: true }).fill(`${key}-2`);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(`Saved ${name}.`)).toBeVisible();
  expect(
    await (await page.request.get("/api/v1/host-keys")).text(),
  ).not.toContain(key);

  await page
    .getByRole("row", { name: new RegExp(name) })
    .getByRole("link", { name })
    .click();
  await page.getByRole("button", { name: `More actions for ${name}` }).click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await page
    .getByRole("dialog", { name: `Delete ${name}?` })
    .getByRole("button", { name: "Delete key" })
    .click();
  await expect(page.getByText(`Deleted ${name}.`)).toBeVisible();
  expect(await page.content()).not.toContain(key);
});
