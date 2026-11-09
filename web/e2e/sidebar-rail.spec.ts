import { axeViolations } from "./a11y";
import { capture } from "./evidence";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

/** The rail's global actions, by role and name (UI-GUIDELINES §6). */
const GLOBAL = [
  ["link", "TensorQuay Agent"],
  ["button", "Expand sidebar"],
  ["link", "New task"],
  ["button", "Search tasks"],
  ["link", "Workspaces"],
  ["link", "Channels"],
  ["button", "Account"],
] as const;

// WEB-E2E-102: collapsed at desktop width, the sidebar is a rail of the global actions, each named and with a tooltip;
// of the task list only the open task shows, its icon with its run mark: no other task, chevron, count or Show more.
for (const theme of ["light", "dark"] as const)
  test(`WEB-E2E-102 the collapsed rail 1440 ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
    await signIn(page);
    // Nine tasks, so the list offers its search; one opened from the list, so its row is surely there.
    for (let index = 0; index < 9; index++) await createConversation(page);
    await page.goto("/");
    const sidebar = page.getByRole("complementary", { name: "Sidebar" });
    await sidebar.locator(".conversation-link").first().click();
    const open = sidebar.locator('.conversation-link[aria-current="page"]');
    await expect(open).toHaveCount(1);
    await sidebar.getByRole("button", { name: "Collapse sidebar" }).click();
    await expect(
      sidebar.getByRole("button", { name: "Expand sidebar" }),
    ).toBeVisible();

    // The global actions: each shown, named, with its tooltip: the app's own beside it on hover, never a native title
    // (UI-GUIDELINES §6, WEB-E2E-115); the toggle, an icon-only control, keeps its title with its key.
    for (const [role, name] of GLOBAL) {
      const control = sidebar.getByRole(role, { name, exact: true });
      await expect(control, name).toBeVisible();
      if (name === "Expand sidebar") {
        expect(await control.getAttribute("title"), name).toBeTruthy();
        continue;
      }
      expect(await control.getAttribute("title"), name).toBeNull();
      await control.hover();
      await expect(
        page.locator(".action-tooltip").filter({ hasText: /\S/ }),
        name,
      ).toBeVisible();
      await page.mouse.move(900, 600);
    }
    // Of the task list, the open task alone; and nothing else shows.
    await expect(sidebar.locator(".conversation-link:visible")).toHaveCount(1);
    await expect(open).toBeVisible();
    await expect(sidebar.locator("a:visible, button:visible")).toHaveCount(
      GLOBAL.length + 1,
    );
    // Its icon carries its mark; nothing loose: no other status, chevron, count, group name or Show more.
    await expect(
      sidebar.locator(
        ".task-helpers-toggle:visible, .task-helpers-count:visible, .sidebar-label:visible, .task-more-button:visible",
      ),
    ).toHaveCount(0);
    expect(await sidebar.locator(".task-status:visible").count()).toBe(
      await open.locator(".task-status:visible").count(),
    );
    await capture(page, `sidebar-rail-1440-${theme}`);
    expect(await axeViolations(page)).toEqual([]);
  });
