import { audit, record } from "./a11y";
import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// Phase 9 accessibility pass (WEB-U-007): every page in both themes, recorded as findings for the report.
// WEB-U-075: the sections by their addresses since the restructure (the overview is /settings itself).
const SETTINGS = [
  "account",
  "tokens",
  "keys",
  "profiles",
  "people",
  "keys/shared",
  "models",
  "internet-access",
  "limits",
  "connectors",
  "environments",
  "usage",
  "status",
];

async function ready(page: Page) {
  await expect(page.locator(".skeleton")).toHaveCount(0, { timeout: 15_000 });
}

// One page per test: a dozen pages in one test outgrew its 30 s under host load (4 of 200 at load ~50, each still
// working when the time ran out), and a per-page test names the page that fails.
const PAGES = [
  "/",
  "/workspaces",
  // Channels (ADR-0014): the list and the seeded channel's page.
  "/channels",
  "/channels/0195d8c0-0000-7000-8000-00000000c4a1",
  "/settings",
  ...SETTINGS.map((section) => `/settings/${section}`),
];

async function auditPage(page: Page, path: string, where: string) {
  await page.goto(path);
  await ready(page);
  record(await audit(page, where));
}

async function auditTaskAfterRun(page: Page, where: string) {
  await page.goto(await createConversation(page));
  await page.getByLabel("Message", { exact: true }).fill("Load the context");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
  record(await audit(page, where));
}

for (const colorScheme of ["light", "dark"] as const) {
  for (const path of PAGES)
    test(`A11Y ${path} signed in ${colorScheme}`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await auditPage(page, path, `${colorScheme} ${path}`);
    });

  test(`A11Y a task after a run ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await auditTaskAfterRun(page, `${colorScheme} /c/<task>`);
  });

  test(`A11Y /login signed out ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme });
    await page.goto("/login");
    await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
    record(await audit(page, `${colorScheme} /login`));
  });

  test(`A11Y /join signed out ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme });
    await page.goto("/join/not-a-valid-invitation");
    await expect(page.getByText(/not valid/)).toBeVisible();
    record(await audit(page, `${colorScheme} /join`));
  });
}

// The narrower layouts: the rail at 1024 and the drawer on a phone width (UI-GUIDELINES §6, §16).
for (const [width, height] of [
  [1024, 768],
  [390, 844],
  [320, 640],
] as const) {
  for (const path of PAGES)
    test(`A11Y ${path} at ${width}`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      await signIn(page);
      await auditPage(page, path, `${width} ${path}`);
    });

  test(`A11Y a task after a run at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await signIn(page);
    await auditTaskAfterRun(page, `${width} /c/<task>`);
  });
}

// A finished task with the files panel's tabs and an opened tool card (WEB-U-022; ADR-0035: nothing asks first). The
// run loads context, so the Context tab is there (finding F6).
test("A11Y a task's panel tabs and a tool card", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await signIn(page);
  await page.goto(await createConversation(page));
  await page.getByLabel("Message", { exact: true }).fill("Load the context");
  await page.getByRole("button", { name: "Send" }).click();
  const findings: string[] = [];
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
  const panel = await openFilesPanel(page);
  for (const tab of ["Changes", "Context"]) {
    await panel.getByRole("tab", { name: tab }).click();
    findings.push(...(await audit(page, `1440 /c/<task> ${tab} tab`)));
  }
  await page
    .getByRole("button", { name: /^Worked for/ })
    .first()
    .click();
  await page.locator(".activity-line button").first().click();
  findings.push(...(await audit(page, "1440 /c/<task> tool card open")));
  record(findings);
});
