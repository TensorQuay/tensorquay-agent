import { signIn } from "./session";
import { axe, capture, fitsColumn, rowMenu, type } from "./settings-page";
import { expect, test, type Locator, type Page } from "./test";

/** Opens Add agent profile, a page of its own (WEB-U-104), by keyboard. */
async function openAdd(page: Page): Promise<Locator> {
  await page.getByRole("link", { name: "Add agent profile" }).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/settings\/agent-profiles\/new$/);
  const form = page.locator("form.settings-form-page");
  await expect(
    form.getByRole("heading", { level: 2, name: "Add an agent profile" }),
  ).toBeVisible();
  return form;
}

// Settings → Profiles on the fake front (ADR-0012; WEB-U-049 browser part), keyboard first, at the widths
// UI-GUIDELINES §17.2 asks for in both themes. Each run adds, then deletes, its own personal profile.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-049 profiles ${width} ${colorScheme}`, async ({ page }) => {
      const name = `e2e-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      // Renamed from Task profiles (the owner's ask); the old address still leads here.
      await page.goto("/settings/profiles");
      await page.waitForURL("**/settings/agent-profiles");
      await expect(page).toHaveTitle("Agent profiles · TensorQuay Agent");
      const shared = page.getByRole("table", { name: "Shared agent profiles" });
      await expect(
        shared.getByRole("row", { name: /Legacy tools/ }),
      ).toContainText("Its environment can't be picked now.");
      await fitsColumn(page, /^More actions for agent profile /);
      await axe(page);
      await capture(page, `profiles-list-${width}-${colorScheme}`);
      const form = await openAdd(page);
      await expect(form).toContainText(
        "An agent profile can only take away from what this installation allows.",
      );
      await type(page, "Display name", `Audit ${name}`, form);
      await axe(page);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(
        page.getByText(`Saved agent profile Audit ${name}.`),
      ).toBeVisible();
      await expect(
        page
          .getByRole("table", { name: "Your agent profiles" })
          .getByRole("row", { name: new RegExp(`Audit ${name}`) }),
      ).toContainText("Ready");
      await rowMenu(
        page,
        `More actions for agent profile Audit ${name}`,
        `Delete agent profile Audit ${name}`,
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete agent profile" })
        .click();
      await expect(
        page.getByText(`Deleted agent profile Audit ${name}.`),
      ).toBeVisible();
    });

// A shared profile brings the administrator's tool servers, chosen by keyboard (WEB-U-059 browser part). Each run adds,
// then deletes, its own shared profile.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-059 tool servers in a shared profile ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `servers-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/agent-profiles");
      const form = await openAdd(page);
      await type(page, "Display name", `Servers ${name}`, form);
      await expect(form).toContainText("Tool servers: none.");
      await form.getByRole("checkbox", { name: /^Shared/ }).focus();
      await page.keyboard.press("Space");
      const servers = form.getByRole("region", { name: "Tool servers" });
      await servers
        .getByRole("checkbox", { name: "tickets (only in Workstation)" })
        .focus();
      await page.keyboard.press("Space");
      await axe(page);
      await capture(page, `profile-servers-${width}-${colorScheme}`);
      await form.getByRole("button", { name: "Save" }).click();
      await expect(
        page.getByText(`Saved agent profile Servers ${name}.`),
      ).toBeVisible();
      await page
        .getByRole("link", { name: `Edit agent profile Servers ${name}` })
        .click();
      const edit = page.locator("form.settings-form-page");
      await expect(
        edit.getByRole("heading", { name: `Edit Servers ${name}` }),
      ).toBeVisible();
      await expect(
        edit.getByRole("checkbox", { name: "tickets (only in Workstation)" }),
      ).toBeChecked();
      await expect(
        edit.getByRole("checkbox", { name: "search (on by default)" }),
      ).not.toBeChecked();
      await edit.getByRole("link", { name: "Agent profiles" }).click();
      await rowMenu(
        page,
        `More actions for agent profile Servers ${name}`,
        `Delete agent profile Servers ${name}`,
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete agent profile" })
        .click();
      await expect(
        page.getByText(`Deleted agent profile Servers ${name}.`),
      ).toBeVisible();
    });

// WEB-E2E-033: the Agent profile page (WEB-U-104) and its tools (WEB-U-105) in the browser, at 1280 and 390 in both
// themes: every tool listed in its group and ticked, each with its line; All mixed, then every tool again; the saved
// shape read back; the bar flush on the window's edge. Each run adds, then deletes, its own profile.
for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-033 an agent profile's tools ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const display = "Release helper";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/agent-profiles");
      const form = await openAdd(page);
      await type(page, "Display name", display, form);
      await expect(form.getByRole("region", { name: "Basics" })).toContainText(
        "ID: release-helper · Change",
      );
      const tools = form.getByRole("region", { name: "Tools" });
      await expect(tools.getByRole("group")).toHaveText([
        /^Files/,
        /^Finding/,
        /^Commands/,
        /^Notes/,
        /^Web/,
        /^Skills/,
        /^Channels/,
      ]);
      const boxes = tools.getByRole("checkbox");
      await expect(boxes).toHaveCount(15);
      // All sits above the group cards, at the grid's left edge (WEB-U-105, WEB-U-131).
      const allBox = await tools
        .getByRole("checkbox", { name: "All", exact: true })
        .boundingBox();
      const firstGroup = await tools
        .getByRole("group", { name: "Files" })
        .boundingBox();
      expect(Math.abs((allBox?.x ?? 0) - (firstGroup?.x ?? -99))).toBeLessThan(
        2,
      );
      expect(allBox?.y ?? 0).toBeLessThan(firstGroup?.y ?? 0);
      for (const box of await boxes.all()) await expect(box).toBeChecked();
      const shell = tools.getByRole("checkbox", {
        name: "Run commands",
        exact: true,
      });
      await expect(shell).toHaveAccessibleDescription(
        "Run commands in the task's computer",
      );
      // By keyboard: Space unticks a tool, and All shows it is mixed.
      await shell.focus();
      await page.keyboard.press("Space");
      await expect(shell).not.toBeChecked();
      const all = tools.getByRole("checkbox", { name: "All", exact: true });
      expect(
        await all.evaluate((box: HTMLInputElement) => box.indeterminate),
      ).toBe(true);
      await tools.scrollIntoViewIfNeeded();
      const bar = form.getByRole("group", { name: "Save or cancel" });
      const box = await bar.boundingBox();
      expect(Math.round((box?.y ?? 0) + (box?.height ?? 0))).toBe(900);
      await axe(page);
      await capture(page, `profile-tools-${width}-${colorScheme}`);
      await bar.getByRole("button", { name: "Save" }).click();
      await expect(
        page.getByText(`Saved agent profile ${display}.`),
      ).toBeVisible();
      // Read back: every tool but the one unticked.
      await page
        .getByRole("link", { name: `Edit agent profile ${display}` })
        .click();
      const edit = page.locator("form.settings-form-page");
      const again = edit.getByRole("region", { name: "Tools" });
      await expect(
        again.getByRole("checkbox", { name: "Run commands", exact: true }),
      ).not.toBeChecked();
      await expect(
        again.getByRole("checkbox", { name: "Read files", exact: true }),
      ).toBeChecked();
      await edit.getByRole("link", { name: "Agent profiles" }).click();
      await rowMenu(
        page,
        `More actions for agent profile ${display}`,
        `Delete agent profile ${display}`,
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete agent profile" })
        .click();
      await expect(
        page.getByText(`Deleted agent profile ${display}.`),
      ).toBeVisible();
    });
