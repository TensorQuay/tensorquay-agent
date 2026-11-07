import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { expect, test, type Page } from "./test";

// The review of d9fb1917 (MUST 1): one Ctrl/Cmd K, one search, and it can always be seen and typed into: at desktop
// width, and at phone width with the drawer closed (the hidden sidebar's list once opened an invisible modal) and open
// (the drawer's list once opened a second one). Choosing a match opens its task and closes the drawer.
async function nine(page: Page) {
  await signIn(page);
  for (let n = 1; n <= 9; n += 1) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: { "Idempotency-Key": randomUUID() },
      data: { title: `Quarterly report ${n}`, model_route: "local" },
    });
    expect(made.status()).toBe(201);
  }
}

/** Ctrl K opens exactly one search, on screen, with the box focused and taking what is typed. */
async function searchOnce(page: Page) {
  await page.keyboard.press("Control+k");
  const dialogs = page.getByRole("dialog", { name: "Search tasks" });
  await expect(dialogs).toHaveCount(1);
  const box = dialogs.getByRole("searchbox", { name: "Search tasks" });
  await expect(box).toBeFocused();
  const size = await dialogs.boundingBox();
  expect(size?.width).toBeGreaterThan(200);
  expect(size?.height).toBeGreaterThan(50);
  await page.keyboard.type("report 9");
  await expect(box).toHaveValue("report 9");
  return dialogs;
}

test("WEB-U-071 one visible search from Ctrl K at 1280", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await nine(page);
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Search tasks" }),
  ).toBeVisible();
  const search = await searchOnce(page);
  // Enter opens the first match, and the search closes. (The fake front lists every task for any words; the gateway
  // matches the title, GW-I-121.)
  const first = search.getByRole("link").first();
  await expect(first).toBeVisible();
  const name = await first.getAttribute("aria-label");
  await page.keyboard.press("Enter");
  await expect(search).toHaveCount(0);
  await expect(page).toHaveTitle(`${name} · TensorQuay Agent`);
});

test("WEB-U-071 one visible search from Ctrl K at 390, drawer closed and open", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await nine(page);
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Open navigation" }),
  ).toBeVisible();
  // Ctrl K works once the list is known to be long enough to search; the hidden sidebar's Search row says so.
  await expect(
    page
      .getByRole("button", { name: "Search tasks", includeHidden: true })
      .first(),
  ).toBeAttached();
  // Drawer closed: the sidebar's list is hidden, yet the search opens on screen.
  let search = await searchOnce(page);
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);
  // Drawer open: two lists are mounted, and still one search.
  await page.getByRole("button", { name: "Open navigation" }).click();
  const drawer = page.getByRole("dialog", { name: "Navigation" });
  await expect(drawer).toBeVisible();
  // Ctrl K is ignored over another dialog (the review's nit 5); the drawer's Search row opens it instead.
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("dialog", { name: "Search tasks" })).toHaveCount(
    0,
  );
  await drawer.getByRole("button", { name: "Search tasks" }).click();
  search = page.getByRole("dialog", { name: "Search tasks" });
  await expect(search).toHaveCount(1);
  await search.getByRole("searchbox").fill("report 9");
  await search.getByRole("link", { name: /^Quarterly report 9/ }).click();
  await expect(search).toHaveCount(0);
  await expect(drawer).toHaveCount(0);
  await expect(page).toHaveTitle("Quarterly report 9 · TensorQuay Agent");
});
