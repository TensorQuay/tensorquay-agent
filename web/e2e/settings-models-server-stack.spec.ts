import { randomUUID } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";

import { allowKey } from "./route-keys";
import { axe } from "./settings-page";
import { signedIn } from "./ux/flow";

// The Models check's findings about the model server itself, on the real stack, with the fake model presenting as
// the server would (crates/tqa-testkit/scenarios/demo.json, TK-U-046/047):
// - WEB-E2E-068 (D5; backs up WEB-U-269 and WEB-U-290): as Ollama, `ollama-small` is loaded with 4 096 tokens of
//   context and `ollama-roomy` with 32 768. Below the minimum the check fails with both sizes and the fixes, and offers
//   no shortcut; with room it passes and "Set Context window" fills the route's form in.
// - WEB-E2E-069 (D6's usage setting; backs up WEB-U-289): `no-usage` refuses `stream_options`. The check passes
//   without it and says to turn usage reporting off; the suggestion lands on its box, and once saved the check passes.
// Each route is the stack's own `local` route (its address and key) under another model, made through the admin API,
// then checked from the Models page as a person does, and removed.

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

/** The `local` route's connection: its address, key and auth, as the stack seeds it. */
async function localConnection(page: Page) {
  const routes = (await (
    await page.request.get("/api/v1/admin/routes")
  ).json()) as { items: Record<string, unknown>[] };
  const local = routes.items.find((item) => item.name === "local");
  if (!local) throw new Error("The stack has no local route");
  return {
    base_url: local.base_url,
    auth: local.auth,
    key_ref: local.key_ref,
  };
}

/** Makes route `name` for the fake model's `model` on Chat Completions, with usage asked for and no Context window. */
async function makeRoute(page: Page, name: string, model: string) {
  const connection = await localConnection(page);
  if (typeof connection.key_ref === "string")
    await allowKey(page, connection.key_ref, name);
  const response = await page.request.fetch(`/api/v1/admin/routes/${name}`, {
    method: "PUT",
    data: {
      ...connection,
      protocol: "openai_chat",
      model,
      stream_usage: true,
      context_window: null,
      display_name: `Route ${name}`,
    },
    headers: {
      Origin: new URL(page.url()).origin,
      "Idempotency-Key": randomUUID(),
    },
  });
  expect(
    response.ok(),
    `PUT ${name}: ${response.status()} ${await response.text()}`,
  ).toBe(true);
}

/**
 * Removes route `name` at its current version (a delete without `?version=` is a 409), as WEB-E2E-060 does; returns
 * the status, 404 when there was none. It never throws: called from a `finally`, a throw would hide the test's error.
 */
async function removeRoute(page: Page, name: string): Promise<number> {
  try {
    const listed = await page.request.get("/api/v1/admin/routes");
    if (!listed.ok()) return listed.status();
    const { items } = (await listed.json()) as {
      items: { name: string; version: number }[];
    };
    const route = items.find((item) => item.name === name);
    if (!route) return 404;
    const removed = await page.request.delete(
      `/api/v1/admin/routes/${name}?version=${route.version}`,
      {
        headers: {
          Origin: new URL(page.url()).origin,
          "Idempotency-Key": randomUUID(),
        },
      },
    );
    return removed.status();
  } catch {
    return 0;
  }
}

/** Runs a route's check from its menu on the Models page; returns the dialog and the real gateway's result. */
async function runCheck(page: Page, label: string) {
  await page.goto("/settings/models");
  await page
    .getByRole("button", { name: `More actions for route ${label}` })
    .click();
  await page.getByRole("menuitem", { name: `Check route ${label}` }).click();
  const dialog = page.getByRole("dialog", { name: `Check route ${label}` });
  await dialog.getByRole("button", { name: "Run check" }).click();
  const result = dialog.getByRole("region", { name: "Check result" });
  // A route saved again is a new version, which the gateway checks at once (GW-I-037), as 069's second check is.
  await expect(result).toBeVisible({ timeout: 60_000 });
  return { dialog, result };
}

const routePage = (page: Page) => page.locator("form.settings-form-page");

test("WEB-E2E-068 a local model's context, from the model server itself", async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signedIn(page);
  let removed: number[] = [];
  try {
    // Below the minimum: both sizes and the fixes, and no shortcut (matching the route wouldn't make tasks work).
    await makeRoute(page, "ollama-small", "ollama-small");
    const small = await runCheck(page, "Route ollama-small");
    await expect(small.result).toContainText(
      /The check failed\. Ollama gives this model 4\s?096 tokens of context; tasks need at least 16\s?384\./,
    );
    await expect(
      small.result.getByText(/^On Linux: run systemctl edit/),
    ).toBeVisible();
    await expect(
      small.result.getByRole("button", { name: /^Set Context window/ }),
    ).toHaveCount(0);
    await axe(page);
    await small.dialog.getByRole("button", { name: "Done" }).click();

    // With room: verified, and the one click fills the route's form in, unsaved, until it's saved.
    await makeRoute(page, "ollama-roomy", "ollama-roomy");
    const roomy = await runCheck(page, "Route ollama-roomy");
    await expect(roomy.result).toContainText(
      "Verified: this route works for tasks.",
    );
    const set = roomy.dialog.getByRole("button", {
      name: /^Set Context window to 32\s?768$/,
    });
    await expect(set).toBeVisible();
    await set.click();
    const form = routePage(page);
    const context = form.getByLabel("Context window");
    await expect(context).toHaveValue("32768");
    await expect(context).toBeFocused();
    await form.getByRole("button", { name: "Save" }).click();
    await expect(
      page.getByText("Saved route Route ollama-roomy."),
    ).toBeVisible();
    const saved = (await (
      await page.request.get("/api/v1/admin/routes")
    ).json()) as { items: { name: string; context_window?: number }[] };
    expect(
      saved.items.find((item) => item.name === "ollama-roomy")?.context_window,
    ).toBe(32768);
  } finally {
    removed = [
      await removeRoute(page, "ollama-small"),
      await removeRoute(page, "ollama-roomy"),
    ];
  }
  // Checked after the finally, so it is reached only when the test passed; a failure keeps its own error.
  expect(removed).toEqual([204, 204]);
});

test("WEB-E2E-069 a server refusing usage reporting: the suggestion lands on its box", async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await signedIn(page);
  let removed = 0;
  try {
    await makeRoute(page, "no-usage", "no-usage");
    const { dialog, result } = await runCheck(page, "Route no-usage");
    await expect(
      result.getByText(
        "The rest of the check worked without usage reporting. Turn off “Ask the server for token usage” for this route.",
      ),
    ).toBeVisible();
    await dialog
      .getByRole("button", { name: "Edit the route with this setting" })
      .click();
    const form = routePage(page);
    const box = form.getByRole("checkbox", {
      name: "Ask the server for token usage",
    });
    await expect(box).not.toBeChecked();
    await expect(box).toBeFocused();
    await axe(page);
    await form.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText("Saved route Route no-usage.")).toBeVisible();
    // Saved without usage reporting, the real server takes the check's requests.
    const again = await runCheck(page, "Route no-usage");
    await expect(again.result).toContainText(
      "Verified: this route works for tasks.",
    );
  } finally {
    removed = await removeRoute(page, "no-usage");
  }
  expect(removed).toBe(204);
});
