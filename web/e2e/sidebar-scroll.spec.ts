import { randomUUID } from "node:crypto";

import { capture } from "./evidence";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";

/** Thirty tasks today, every one shown (Show more until it's gone), so the list is longer than the sidebar. */
async function longList(page: Page) {
  await signIn(page);
  for (let n = 1; n <= 30; n += 1) {
    const made = await page.request.post("/api/v1/conversations", {
      headers: { "Idempotency-Key": randomUUID() },
      data: { title: `Scroll task ${n}`, model_route: "local" },
    });
    expect(made.status()).toBe(201);
  }
  await page.goto("/");
  const sidebar = page.locator(".app-sidebar");
  // The list first, then Show more until every task is in it (it may sit below the list's visible part).
  await expect(
    sidebar.getByRole("link", { name: /^Scroll task/ }).first(),
  ).toBeVisible();
  const more = sidebar.getByRole("button", { name: "Show more" });
  for (let shown = 0; shown < 10 && (await more.count()) > 0; shown += 1)
    await more.click();
  await expect(sidebar.getByRole("link", { name: /^Scroll task/ })).toHaveCount(
    30,
  );
}

// WEB-E2E-123 (18.0; the owner, 15 Nov 2026: "the scroll makes the whole area scroll"): with a long task list, only
// the list scrolls; the brand row, New task, Workspaces, Channels and Search above it and the account row below it
// stay where they are, in light and dark, on the expanded sidebar.
for (const colorScheme of ["light", "dark"] as const)
  test(`WEB-E2E-123 only the sidebar's task list scrolls ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 640 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await longList(page);
    const sidebar = page.locator(".app-sidebar");
    const list = sidebar.locator(".conversation-list");
    const newTask = sidebar.getByRole("link", { name: "New task" });
    const search = sidebar.getByRole("button", { name: /^Search/ });
    const account = sidebar.locator(".sidebar-account");
    const before = await newTask.boundingBox();
    // The list holds more than it shows, and scrolls by itself.
    const overflow = await list.evaluate(
      (element) => element.scrollHeight - element.clientHeight,
    );
    expect(overflow).toBeGreaterThan(100);
    await list.hover();
    await page.mouse.wheel(0, 2000);
    await expect
      .poll(() => list.evaluate((element) => element.scrollTop))
      .toBeGreaterThan(100);
    // Nothing above or below it moved, and none of it left the screen.
    expect(await sidebar.evaluate((element) => element.scrollTop)).toBe(0);
    expect(await newTask.boundingBox()).toEqual(before);
    await expect(newTask).toBeInViewport();
    await expect(search).toBeInViewport();
    await expect(account).toBeInViewport();
    // The list's last task can be reached at its end.
    await expect(
      sidebar.getByRole("link", { name: /^Scroll task/ }).last(),
    ).toBeInViewport();
    await capture(page, `sidebar-scroll-${colorScheme}`);
  });
