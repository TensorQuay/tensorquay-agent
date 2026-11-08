import { randomUUID } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";

import { pick } from "./choices";
import { axe, type } from "./settings-page";
import { signedIn } from "./ux/flow";

// WEB-E2E-060 on the real stack (WEB-E2E-039 and WEB-U-281/-287 back it up on the fake front): the Models check
// run by the real gateway against real addresses.
// - A route at localhost: inside the gateway's container localhost is the gateway itself, so nothing answers. This
//   stack's gateway has no host.docker.internal entry (deploy/compose/docker.yml), the plain-Linux case, so the
//   result says to use the computer's network address and offers no one-click change.
// - A server without the route's protocol endpoint: the stack's own web server (on the gateway's `web` network)
//   answers POST /v1/chat/completions with a 404 that names no model, so the check reports the missing endpoint.
// The usage setting (WEB-U-289) needs a model server refusing `stream_options`, which the stack's fake model can't do
// yet: it is checked live.

test.skip(!process.env.TQA_STACK_URL, "Requires the owned settings stack");
test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await signedIn(page);
  } finally {
    await page.close();
  }
});

/** A route's own page (WEB-U-115): its form, from the title down to Save and Cancel. */
const routePage = (page: Page) => page.locator("form.settings-form-page");

/** Adds a keyless Chat Completions route at `address`, named `name` and shown as "Route <name>". */
async function addRoute(page: Page, name: string, address: string) {
  await page.goto("/settings/models");
  await page.getByRole("link", { name: "Add route" }).click();
  const form = routePage(page);
  await expect(
    form.getByRole("heading", { level: 2, name: "Add a route" }),
  ).toBeVisible();
  await form.locator("summary", { hasText: "Advanced" }).click();
  await type(page, "Route name", name, form);
  await type(page, "Display name", `Route ${name}`, form);
  await type(page, "Address", address, form);
  await type(page, "Model", "qwen3:4b", form);
  await pick(page, "Send the key", "No key", form);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved route Route ${name}.`)).toBeVisible();
}

/** Runs a route's check from its menu; returns the dialog and its result once the real gateway has answered. */
async function runCheck(page: Page, label: string) {
  await page
    .getByRole("button", { name: `More actions for route ${label}` })
    .click();
  await page.getByRole("menuitem", { name: `Check route ${label}` }).click();
  const dialog = page.getByRole("dialog", { name: `Check route ${label}` });
  await dialog.getByRole("button", { name: "Run check" }).click();
  const result = dialog.getByRole("region", { name: "Check result" });
  // The real probe waits on the network: at most its connect and request timeouts.
  await expect(result).toBeVisible({ timeout: 60_000 });
  return { dialog, result };
}

/**
 * Removes a route this spec added, so the suite's other specs see only their own. A delete names the version it read
 * (without one the gateway answers 409, admin.rs version_query). Returns the status, 404 when it was never added, or 0
 * when the call couldn't be made: cleanup never throws over the test's own failure.
 */
async function removeRoute(page: Page, name: string): Promise<number> {
  try {
    const origin = new URL(page.url()).origin;
    const listed = await page.request.get("/api/v1/admin/routes");
    if (!listed.ok()) return listed.status();
    const { items } = (await listed.json()) as {
      items: { name: string; version: number }[];
    };
    const route = items.find((item) => item.name === name);
    if (!route) return 404;
    const removed = await page.request.delete(
      `/api/v1/admin/routes/${name}?version=${route.version}`,
      { headers: { "Idempotency-Key": randomUUID(), Origin: origin } },
    );
    return removed.status();
  } catch {
    return 0;
  }
}

test("WEB-E2E-060 the Models check against real addresses: localhost, and a server without the endpoint", async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signedIn(page);
  const removed: number[] = [];
  try {
    // A route at localhost: unreachable from the gateway's container, and here no host.docker.internal to offer.
    await addRoute(page, "stack-localhost", "http://localhost:11434/v1");
    const local = await runCheck(page, "Route stack-localhost");
    await expect(local.result).toContainText(
      "The check failed. The server could not be reached.",
    );
    await expect(
      local.result.getByText(
        "Here localhost is the gateway's own container, not this computer, and host.docker.internal doesn't resolve on this installation. Use this computer's network address instead, with the same port.",
      ),
    ).toBeVisible();
    await expect(
      local.result.getByRole("button", { name: "Use host.docker.internal" }),
    ).toHaveCount(0);
    await axe(page);
    await local.dialog.getByRole("button", { name: "Done" }).click();

    // A real server that has no Chat Completions endpoint: the stack's web server.
    await addRoute(page, "stack-noendpoint", "http://web:3000/v1");
    const missing = await runCheck(page, "Route stack-noendpoint");
    await expect(missing.result).toContainText(
      "The check failed. The server has no endpoint for this route's protocol at this address.",
    );
    await expect(
      missing.result.getByText(
        "This route speaks Chat Completions. Check the address (it often ends in /v1), or edit the route and try Responses or Messages under Advanced › Connection.",
      ),
    ).toBeVisible();
    await axe(page);
    await missing.dialog.getByRole("button", { name: "Done" }).click();
  } finally {
    removed.push(await removeRoute(page, "stack-localhost"));
    removed.push(await removeRoute(page, "stack-noendpoint"));
  }
  // Checked after, so a failed cleanup never hides a failure in the checks above.
  expect(removed, "both routes this spec added, removed").toEqual([204, 204]);
});
