import { readdirSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { createConversation, signIn } from "./session";
import { expect, resetFront, test, type Page } from "./test";

// WEB-E2E-086 (the 16f page pass, E17): below 768 px the app sidebar is a drawer (globals.css), so every page inside the
// app shell must bring its own Open navigation button, or a phone is stuck on it. The routes are read from the app
// folder itself, so a new page is walked without editing this list; a new dynamic route needs its id below, and fails
// by name until it has one.

const APP = fileURLToPath(new URL("../src/app/(app)/", import.meta.url));
/** Not a product page: the design reference exists only in test builds (P37). */
const SKIP = new Set(["/design"]);

function routes(directory = APP): string[] {
  const found: string[] = [];
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) found.push(...routes(path));
    else if (name === "page.tsx") {
      const route = `/${relative(APP, directory).split(sep).filter(Boolean).join("/")}`;
      if (!SKIP.has(route)) found.push(route);
    }
  }
  return found.sort();
}

/** A real value for each dynamic segment, read from the fake front's seed (and the two key scenarios). */
async function ids(page: Page): Promise<Record<string, string>> {
  const first = async (
    path: string,
    pick: (item: Record<string, unknown>) => boolean = () => true,
  ) => {
    const answer = await page.request.get(path);
    expect(answer.ok(), path).toBe(true);
    const { items } = (await answer.json()) as {
      items: Record<string, unknown>[];
    };
    const item = items.find(pick);
    if (!item) throw new Error(`The fake front lists nothing at ${path}`);
    return item;
  };
  const conversation = await createConversation(page);
  const channel = await first("/api/v1/channels");
  const own = await first("/api/v1/host-keys", (item) => item.shared !== true);
  const shared = await first("/api/v1/admin/host-keys");
  const profile = await first("/api/v1/profiles");
  const person = await first("/api/v1/users");
  const route = await first("/api/v1/admin/routes");
  const template = await first("/api/v1/admin/templates");
  return {
    "/c/[id]": conversation,
    "/channels/[id]": String(channel.id),
    "/settings/keys/[id]": String(own.id),
    "/settings/keys/shared/[id]": String(shared.id),
    "/settings/agent-profiles/[id]": String(profile.id),
    "/settings/agent-profiles/[id]/duplicate": String(profile.id),
    "/settings/people/[id]": String(person.id),
    "/settings/models/[name]": String(route.name),
    "/settings/environments/[name]": String(template.name),
  };
}

test("WEB-E2E-086 every page in the app has Open navigation below 768 px", async ({
  page,
  request,
}) => {
  test.setTimeout(240_000);
  await resetFront(request, ["shared-host-keys", "host-key-refused"]);
  await signIn(page);
  await page.setViewportSize({ width: 390, height: 844 });
  const values = await ids(page);
  const all = routes();
  // The walk covers the shell's top-level pages: guard against an empty or wrong folder read.
  expect(all).toEqual(
    expect.arrayContaining([
      "/",
      "/c/[id]",
      "/channels",
      "/settings",
      "/workspaces",
    ]),
  );
  const missing: string[] = [];
  for (const route of all) {
    const id = route.includes("[") ? values[route] : undefined;
    if (route.includes("[") && id === undefined) {
      missing.push(`${route}: no id for this dynamic route in WEB-E2E-086`);
      continue;
    }
    const path = route.replace(/\[[^\]]+\]/g, encodeURIComponent(id ?? ""));
    await page.goto(path);
    await page.waitForLoadState("networkidle").catch(() => undefined);
    const button = page.getByRole("button", {
      name: "Open navigation",
      exact: true,
    });
    if (!(await button.isVisible().catch(() => false))) {
      missing.push(`${route} (${new URL(page.url()).pathname})`);
      continue;
    }
    // It opens the navigation, and the drawer offers New task.
    await button.click();
    const drawer = page.getByRole("dialog", { name: "Navigation" });
    await expect(drawer, route).toBeVisible();
    await expect(
      drawer.getByRole("link", { name: "New task" }).first(),
      route,
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(drawer, route).toBeHidden();
  }
  expect(missing, "Pages with no way to the navigation below 768 px").toEqual(
    [],
  );
});

test("WEB-E2E-086 Open navigation in Simplified Chinese", async ({ page }) => {
  await signIn(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/settings/account");
  await page.evaluate(() =>
    localStorage.setItem("tensorquay-language", "zh-Hans"),
  );
  await page.reload();
  await expect(
    page.getByRole("button", { name: "打开导航", exact: true }),
  ).toBeVisible();
});

// WEB-E2E-087 (the 16f page pass, F1): the sidebar's account row is hidden below 768 px, so the drawer holds the
// account menu at its foot. From it a phone reaches Settings, and signs out; each closes the drawer.
test("WEB-E2E-087 the drawer's account menu at 390 px: Settings, then sign out", async ({
  page,
  request,
}) => {
  await resetFront(request);
  await signIn(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const open = page.getByRole("button", {
    name: "Open navigation",
    exact: true,
  });
  const drawer = page.getByRole("dialog", { name: "Navigation" });
  await open.click();
  const account = drawer.getByRole("button", { name: "Account", exact: true });
  await expect(account).toBeVisible();
  // At the drawer's foot: its bottom edge within the window's last 96 px.
  const box = await account.boundingBox();
  expect(box && box.y + box.height).toBeGreaterThan(844 - 96);
  await account.click();
  const menu = drawer.getByRole("menu", { name: "Account" });
  await menu.getByRole("menuitem", { name: "Settings", exact: true }).click();
  await expect(page).toHaveURL(/\/settings(\/|$)/);
  await expect(drawer).toBeHidden();

  await open.click();
  await drawer.getByRole("button", { name: "Account", exact: true }).click();
  await menu.getByRole("menuitem", { name: "Sign out", exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await expect(drawer).toBeHidden();
  expect((await page.request.get("/api/v1/auth/me")).status()).toBe(401);
});
