import { randomUUID } from "node:crypto";

import { expect, test, type BrowserContext, type Page } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import { signedIn } from "./ux/flow";

// The c15 feature matrix's gaps in Settings (feature-matrix-c15, N8, D13 and D7): what had a unit test and a fake-front
// spec but no real-stack test. On the settings suite's stack, before its own spec (files run in name order), so the
// installation still has no internet access rule when N8 looks; D13 takes off what it adds. It writes no result.
test.skip(
  process.env.TQA_STACK_SUITE !== "settings",
  "Requires the settings stack suite",
);
// One sign-in for the whole file, its cookies reused by each test's own browser context: the gateway counts every
// sign-in from an address, 10 a minute (auth/rate.rs), and a sign-in per test took the suite past it (the 15b rerun:
// WEB-E2E-025's own sign-in was the 11th, refused with 429).
let session: Awaited<ReturnType<BrowserContext["storageState"]>>;
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const context = await browser.newContext({ baseURL });
  try {
    await signedIn(await context.newPage());
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
/** Made-up keys' shapes, put together here so the source holds nothing key-shaped for the secrets scan (G-10). */
const PASTED = ["sk", "proj", "AbC_123"].join("-");
const KEY_SHAPED = ["sk", "a1B2c3D4e5F6g7H8i9J0k"].join("-");

const lists = (page: Page) =>
  page.getByRole("region", { name: "Ready-made lists" });
const card = (page: Page, name: string) =>
  lists(page).getByRole("group", { name });

// N8 (WEB-U-119, WEB-U-153): with no site allowed, New task says tasks can't reach the internet, in words for an
// administrator, with the way to allow sites.
test("WEB-E2E-057 New task says tasks can't reach the internet, and how to allow sites", async ({
  page,
}) => {
  test.setTimeout(budget(1, 120_000));
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "What should the agent do?" }),
  ).toBeVisible({ timeout: afterStart(30_000) });
  await expect(
    page.getByText("Tasks can't reach the internet: no sites are allowed yet."),
  ).toBeVisible();
  await page.getByRole("link", { name: "Allow sites" }).click();
  await expect(page).toHaveURL(/\/settings\/internet-access$/);
});

// D13 (WEB-U-118, WEB-U-162): a ready-made list added through the real gateway: its sites in the allow-list marked
// with its name, its card On, then removed, leaving the list as it was.
test("WEB-E2E-058 a ready-made list is added, marked and removed", async ({
  page,
}) => {
  test.setTimeout(budget(1, 120_000));
  await page.goto("/settings/internet-access");
  await expect(lists(page).getByRole("group")).toHaveText(
    [/^Package registries/, /^Code hosting/, /^Open web/, /^Custom/],
    { timeout: afterStart(30_000) },
  );
  await card(page, "Package registries")
    .getByRole("button", { name: "Add Package registries" })
    .click();
  try {
    await expect(
      page.getByText("Added Package registries: 8 sites."),
    ).toBeVisible();
    await expect(page.getByRole("row", { name: /^crates\.io/ })).toContainText(
      "Package registries",
    );
    await expect(
      card(page, "Package registries").getByRole("status"),
    ).toHaveText("On");
  } finally {
    await card(page, "Package registries")
      .getByRole("button", { name: "Remove Package registries" })
      .click();
    await expect(page.getByText("Removed Package registries.")).toBeVisible();
  }
  await expect(page.getByRole("row", { name: /^crates\.io/ })).toHaveCount(0);
  await expect(card(page, "Package registries").getByRole("status")).toHaveText(
    "Off",
  );
});

// D7 (WEB-U-294, WEB-U-295, WEB-U-303): a key pasted where a key's name belongs is refused on the page and never
// sent, in the route form's Shared key and as a new shared key's Name; the gateway refuses the name too (SEC-033).
test("WEB-E2E-059 a pasted or key-shaped key is refused where a name belongs, and never sent", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(1, 180_000));
  const bodies: string[] = [];
  page.on("request", (request) => {
    bodies.push(`${request.url()} ${request.postData() ?? ""}`);
  });
  const sent = (text: string) => bodies.filter((body) => body.includes(text));

  await page.goto("/settings/models");
  await page.getByRole("link", { name: "Add route" }).click();
  const form = page.locator("form.settings-form-page");
  await form.locator("summary", { hasText: "Advanced" }).click();
  await form.getByLabel("Route name", { exact: true }).fill(`pasted-${RUN}`);
  await form.getByLabel("Display name", { exact: true }).fill(`Pasted ${RUN}`);
  await form
    .getByLabel("Address", { exact: true })
    .fill("https://models.example.invalid/v1");
  await form.getByLabel("Model", { exact: true }).fill("big-model");
  await form
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  for (const pasted of [PASTED, `secrets/${PASTED}`]) {
    await form.getByLabel("Shared key", { exact: true }).fill(pasted);
    await form.getByRole("button", { name: "Save" }).click();
    await expect(
      form.getByText(
        "Enter the name of a shared key, or use Paste key to store one.",
      ),
    ).toBeVisible();
  }
  expect(sent("AbC_123")).toEqual([]);

  await page.goto("/settings/keys/shared");
  await page.getByRole("button", { name: "Add a shared key" }).click();
  const add = page.getByRole("dialog", { name: "Add a shared key" });
  await add.getByLabel("Name", { exact: true }).fill(KEY_SHAPED);
  await add.getByLabel("Value", { exact: true }).fill("x");
  await add.getByRole("button", { name: "Add key" }).click();
  await expect(
    add.getByText(/^This looks like a key, not a name\./),
  ).toBeVisible();
  expect(sent(KEY_SHAPED)).toEqual([]);
  await page.keyboard.press("Escape");

  // The gateway's own refusal, for a client that isn't the page (SEC-033): nothing is stored.
  const refused = await page.request.put(
    `/api/v1/admin/secrets/${encodeURIComponent(KEY_SHAPED)}`,
    {
      headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
      data: { value: "x" },
    },
  );
  expect(refused.status()).toBe(400);
  // The gateway names the field and its reason in the error's details; the code is the shared invalid_request
  // (secrets.rs, as the gateway's own key_shaped_names.rs reads it).
  const { error } = (await refused.json()) as {
    error: { code: string; details?: { field: string; reason: string }[] };
  };
  expect(error.code).toBe("invalid_request");
  expect(error.details).toContainEqual({
    field: "name",
    reason: "name_key_shaped",
  });
  await page.reload();
  await expect(page.getByText(KEY_SHAPED)).toHaveCount(0);
});
