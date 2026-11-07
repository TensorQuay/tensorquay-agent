import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { openCard } from "./cards";
import { chooseModel } from "./model";
import { axe, type } from "./settings-page";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// WEB-E2E-006 (FR-4, FR-22, ADR-0028): on the real stack, an administrator's changes in the browser apply to the next
// new conversation without a restart: the model route and the allow rule serve an invited member's new conversation,
// and deleting the rule blocks the next one again. The driver (crates/tqa-sandbox/tests/egress/web_admin.py) owns the
// upstream and the scripted model and checks the stored results afterwards.
test.skip(!process.env.TQA_ADMIN_URL, "Requires the owned admin fixture");

const env = (name: string) => {
  const value = process.env[name];
  if (!value) throw new Error(`The driver must supply ${name}`);
  return value;
};
/** A key of the upstream's JSON answer (upstream.py), never in a request or a refusal. */
const UPSTREAM_ANSWER = '"proxy_authorization"';
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const blocked = () =>
  `Blocked network access to ${env("TQA_ADMIN_UPSTREAM")}:${env("TQA_ADMIN_PORT")}: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.`;

/** Starts a new conversation on a route, asks for the fixture page and returns its id once the run is done. */
async function fetchInNewConversation(page: Page, route: string) {
  await page.goto("/");
  await chooseModel(page, route);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Fetch the fixture page.");
  await message.press("Enter");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(30_000),
  });
  await page.getByRole("button", { name: /^Worked for/ }).click();
  return new URL(page.url()).pathname.split("/").at(-1) ?? "";
}

/** The fetch was refused at the proxy: the blocked line shows, and the upstream never answered. */
async function expectBlocked(page: Page) {
  await expect(
    page.getByText(blocked(), { exact: true }).filter({ visible: true }),
  ).toHaveCount(1);
  // A blocked fetch's line is the refusal itself; its card holds no answer from the upstream.
  await openCard(
    page.getByRole("button", { name: new RegExp(`^${escape(blocked())}`) }),
  );
  const card = page.getByRole("article", { name: "fetch_url tool call" });
  await expect(card).toBeVisible();
  await expect(card).not.toContainText(UPSTREAM_ANSWER);
}

async function deleteRule(page: Page, host: string) {
  await page.goto("/settings/internet-access");
  // WEB-U-162: Delete is under the row's ⋯, as a danger item.
  await page
    .getByRole("button", { name: `More actions for rule ${host}` })
    .click();
  await page.getByRole("menuitem", { name: `Delete rule ${host}` }).click();
  await page
    .getByRole("dialog", { name: `Delete rule ${host}?` })
    .getByRole("button", { name: "Delete rule" })
    .click();
  await expect(page.getByText(`Deleted rule ${host}.`)).toBeVisible();
}

async function addRoute(page: Page) {
  await page.goto("/settings/models");
  await page.getByRole("link", { name: "Add route" }).click();
  // The route's name waits under Advanced, made from the display name until typed (F4).
  await page
    .locator("form.settings-form-page")
    .locator("summary", { hasText: "Advanced" })
    .click();
  await type(
    page,
    "Route name",
    "second",
    page.locator("form.settings-form-page"),
  );
  await type(
    page,
    "Display name",
    "Second model",
    page.locator("form.settings-form-page"),
  );
  await type(
    page,
    "Address",
    "http://fake-model:8080/v1",
    page.locator("form.settings-form-page"),
  );
  await type(page, "Model", "fixture", page.locator("form.settings-form-page"));
  await page
    .locator("form.settings-form-page")
    .getByRole("button", { name: "Use an existing shared key" })
    .click();
  await type(
    page,
    "Shared key",
    "local",
    page.locator("form.settings-form-page"),
  );
  await axe(page);
  const save = page.getByRole("button", { name: "Save" });
  await save.click();
  // Vault slice 3: the seeded secret is bound to its own route only, so the gateway refuses a new route naming it
  // (409 secret_not_allowed) and the form offers to allow it; the admin does, then saves again (WEB-U-054).
  const dialog = page.locator("form.settings-form-page");
  await expect(
    dialog.getByText("local isn't allowed for this route. Allow it first."),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "Allow it for second" }).click();
  await expect(dialog.getByText("Allowed. Save again.")).toBeVisible();
  await save.click();
  await expect(page.getByText("Saved route Second model.")).toBeVisible();
}

async function addRule(page: Page, host: string) {
  await page.goto("/settings/internet-access");
  await page.getByRole("button", { name: "Add a site" }).click();
  await type(page, "Host", host);
  await type(page, "Ports", env("TQA_ADMIN_PORT"));
  await page.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText(`Saved rule ${host}.`)).toBeVisible();
  await axe(page);
}

async function invite(page: Page) {
  await page.goto("/settings/people");
  await page
    .getByRole("textbox", { name: "Email" })
    .fill("member@example.invalid");
  await page.getByRole("button", { name: "Invite", exact: true }).click();
  const invited = page.getByRole("dialog", { name: "Invitation created" });
  const link = await invited
    .getByRole("textbox", { name: "Invitation link" })
    .inputValue();
  await invited.getByRole("button", { name: "I’ve copied it" }).click();
  return new URL(link).pathname;
}

/** The invited member joins and, on the route added moments ago, fetches through the rule added moments ago. */
async function memberFetches(page: Page, link: string) {
  const context = page
    .context()
    .browser()
    ?.newContext({
      baseURL: new URL(page.url()).origin,
    });
  const guest = await (await context)?.newPage();
  if (!guest) throw new Error("The member needs a browser");
  await guest.goto(link);
  await guest
    .getByLabel("Password", { exact: true })
    .fill(`synthetic-member-${Date.now()}`);
  await guest.getByRole("button", { name: "Join" }).click();
  await expect(guest.getByLabel("Message", { exact: true })).toBeVisible();
  await fetchInNewConversation(guest, "second");
  await expect(guest.getByText(blocked(), { exact: true })).toHaveCount(0);
  // The fetch's own result, from the upstream: the path it served. Finding A1: a fetch's line names the page it read.
  await openCard(guest.getByRole("button", { name: /^Read / }));
  // Only the upstream's own answer carries these keys (upstream.py); the URL alone is in the arguments too.
  await expect(
    guest.getByRole("article", { name: "fetch_url tool call" }),
  ).toContainText(UPSTREAM_ANSWER);
  await guest.context().close();
}

test("WEB-E2E-006 admin changes apply to the next new conversation", async ({
  page,
}) => {
  test.setTimeout(budget(1, 210_000));
  const host = env("TQA_ADMIN_UPSTREAM");
  await page.setViewportSize({ width: 1440, height: 900 });
  await bootstrap(page);
  await deleteRule(page, host);
  const before = await fetchInNewConversation(page, "local");
  await expectBlocked(page);
  await addRoute(page);
  await addRule(page, host);
  const link = await invite(page);
  await memberFetches(page, link);
  await deleteRule(page, host);
  const after = await fetchInNewConversation(page, "local");
  await expectBlocked(page);
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({ before, after }),
      { mode: 0o600 },
    );
});
