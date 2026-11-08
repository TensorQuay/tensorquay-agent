import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { addFromPlus } from "./choices";
import { settled } from "./motion";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// Task 076 on the fake front: the Workspaces page, its dialogs and the picker (WEB-U-007 accessibility and keyboard,
// WEB-U-025 behaviour end to end). The real stack runs WEB-E2E-011 in workspaces-stack.spec.ts.
async function serious(page: Page) {
  // Measure the settled layer: an open menu's entrance fade would lower contrast mid-way.
  await settled(page);
  const { violations } = await new AxeBuilder({ page }).analyze();
  return violations
    .filter((violation) =>
      ["serious", "critical"].includes(violation.impact ?? ""),
    )
    .map((violation) => violation.id);
}

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `workspaces-${name}.png`) });
}

async function open(page: Page) {
  await signIn(page);
  await page.goto(await createConversation(page));
  await page.goto("/workspaces");
  await expect(page.getByRole("table", { name: "Workspaces" })).toBeVisible();
}
/** A workspace's row in the table (UI-GUIDELINES §7: a table, and cards on a phone). */
const card = (page: Page, name: string) =>
  page
    .getByRole("main")
    .getByRole("row")
    .filter({
      has: page.locator(".data-name > span").getByText(name, { exact: true }),
    });
/** Chooses one of a workspace's actions from its menu. */
async function choose(page: Page, name: string, action: string) {
  await page
    .getByRole("main")
    .getByRole("button", { name: `More actions for ${name}` })
    .click();
  await page.getByRole("menuitem", { name: action, exact: true }).click();
}

test("WEB-U-007 the Workspaces page and its dialogs pass the accessibility checks", async ({
  page,
}) => {
  await open(page);
  expect(await serious(page)).toEqual([]);
  await capture(page, "table");
  // A phone gets cards, and the filters, search and sort still fit.
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("article").first()).toBeVisible();
  expect(await serious(page)).toEqual([]);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await capture(page, "cards-390");
  await page.setViewportSize({ width: 1440, height: 1000 });

  await choose(page, "Workspace review", "Share");
  const share = page.getByRole("dialog", { name: "Share “Workspace review”" });
  await share
    .getByRole("textbox", { name: "Email" })
    .fill("nobody@example.invalid");
  await share.getByRole("button", { name: "Share", exact: true }).click();
  await expect(share.getByText("No account uses that email.")).toBeVisible();
  expect(await serious(page)).toEqual([]);
  await share
    .getByRole("textbox", { name: "Email" })
    .fill("li@example.invalid");
  // WEB-U-173: a share is Can view unless the owner chooses more.
  await expect(
    share.getByRole("button", { name: "Access: Can view" }),
  ).toBeVisible();
  await share.getByRole("button", { name: "Share", exact: true }).click();
  await expect(share.getByText("li@example.invalid")).toBeVisible();
  await capture(page, "share");
  expect(await serious(page)).toEqual([]);
  // No Done: each change applies at once, and its close button closes it (the UI standard's audit).
  await share.getByRole("button", { name: "Close dialog" }).click();
  await expect(
    card(page, "Workspace review").getByText("Shared with 1 person"),
  ).toBeAttached();

  await choose(page, "Workspace review", "Delete");
  const remove = page.getByRole("dialog", { name: "Delete workspace" });
  await expect(remove).toContainText(
    "Delete “Workspace review” and its 1 task? The tasks and the files are removed.",
  );
  expect(await serious(page)).toEqual([]);
  await remove.getByRole("button", { name: "Cancel" }).click();
});

// A pass over the pages before 17.0: on a phone the share dialog gives the email its own row, wide enough for an
// address, with the access and Share on the row below.
test("WEB-U-007 at 390 the share dialog's email has its own row", async ({
  page,
}) => {
  await open(page);
  await choose(page, "Workspace review", "Share");
  const share = page.getByRole("dialog", { name: "Share “Workspace review”" });
  await page.setViewportSize({ width: 390, height: 844 });
  const email = await share
    .getByRole("textbox", { name: "Email" })
    .boundingBox();
  const button = await share
    .getByRole("button", { name: "Share", exact: true })
    .boundingBox();
  const dialog = await share.boundingBox();
  expect(email && button && dialog).toBeTruthy();
  if (!email || !button || !dialog) return;
  expect(email.width).toBeGreaterThan(dialog.width * 0.7);
  expect(button.y).toBeGreaterThanOrEqual(email.y + email.height);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await capture(page, "share-390");
});

test("WEB-U-007 keyboard: a workspace's row and its menu, and dialogs return focus", async ({
  page,
}) => {
  await open(page);
  const review = card(page, "Workspace review");
  const more = review.getByRole("button", {
    name: "More actions for Workspace review",
  });
  // The row opens the workspace; Tab moves from it to its menu.
  await review.focus();
  await page.keyboard.press("Tab");
  await expect(more).toBeFocused();
  // The tip is a layer of its own on the page (WEB-U-429).
  await expect(
    page.locator(".action-tooltip", {
      hasText: "More actions for Workspace review",
    }),
  ).toBeVisible();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu", {
    name: "More actions for Workspace review",
  });
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  // Open, New task here, then Make a copy.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(
    menu.getByRole("menuitem", { name: "Make a copy" }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Copy “Workspace review”" });
  await expect(dialog.getByRole("textbox", { name: "Name" })).toHaveValue(
    "Workspace review (copy)",
  );
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(more).toBeFocused();
});

test("WEB-U-025 fork shows the copy at once, then offers a task in it on the chosen workspace", async ({
  page,
}) => {
  await open(page);
  await choose(page, "Workspace review", "Make a copy");
  await page
    .getByRole("dialog", { name: "Copy “Workspace review”" })
    .getByRole("button", { name: "Make a copy", exact: true })
    .click();
  const copy = card(page, "Workspace review (copy)");
  await expect(copy.getByText("Copying “Workspace review”…")).toBeVisible();
  await capture(page, "fork");
  await expect(copy.getByRole("meter")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("button", { name: "Start a task in the copy" }).click();
  await expect(page).toHaveURL(/\/$/);
  // New task shows the workspace it will use as a chip in its composer (the UI standard's composer).
  await expect(
    page.getByRole("button", { name: "Workspace: Workspace review (copy)" }),
  ).toBeVisible();
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Check the copied files");
  await input.press("Enter");
  await expect(page).toHaveURL(/\/c\/[0-9a-f-]{36}$/);
  // The task says which workspace it is in, quietly under its title (candidate12's workspaces suite).
  await expect(
    page
      .getByRole("region", { name: "Task header" })
      .getByText("In Workspace review (copy)", { exact: true }),
  ).toBeVisible();
  const row = page
    .getByRole("region", { name: "Tasks" })
    .getByRole("link", { name: /Check the copied files/ });
  // The task's workspace is on hover, then its time (UI-GUIDELINES §7, WEB-U-070).
  await expect(row).toHaveAttribute(
    "title",
    /^Check the copied files · Workspace review \(copy\) · /,
  );
});

test("WEB-U-025 the picker lists busy and read-only workspaces, and Open filters the task list", async ({
  page,
}) => {
  await open(page);
  await page.goto("/");
  await addFromPlus(page, "Workspace…");
  const menu = page.getByRole("menu", { name: "Workspace" });
  await expect(
    menu.getByRole("menuitemradio", { name: /Q3 planning/ }),
  ).toBeDisabled();
  await expect(
    menu.getByRole("menuitemradio", { name: /Q3 planning/ }),
  ).toContainText("view only");
  expect(await serious(page)).toEqual([]);
  await capture(page, "picker");
  await menu.getByRole("menuitemradio", { name: /Team handbook/ }).click();
  await expect(
    page.getByText("In use: your task will wait for it."),
  ).toBeVisible();

  await page.goto("/workspaces");
  await card(page, "Workspace review").getByRole("cell").nth(2).click();
  const chip = page.getByRole("button", {
    name: "Remove filter Workspace: Workspace review",
  });
  await expect(chip).toBeFocused();
  await expect(
    page.getByRole("region", { name: "Tasks" }).getByRole("link"),
  ).toHaveCount(1);
  await chip.click();
  await expect(chip).toHaveCount(0);
});

// Linux bring-up follow-up: a long workspace name wrapped the 260 px sidebar's label line. Since UI-GUIDELINES §7 the row is one
// line: the name is on hover (UI-GUIDELINES §5b).
test("WEB-U-026 a task row stays one line: a long workspace name is on hover", async ({
  page,
}) => {
  const name = "Quarterly revenue reconciliation for every regional office";
  await open(page);
  await choose(page, "Workspace review", "Make a copy");
  const dialog = page.getByRole("dialog", { name: "Copy “Workspace review”" });
  await dialog.getByRole("textbox", { name: "Name" }).fill(name);
  await dialog
    .getByRole("button", { name: "Make a copy", exact: true })
    .click();
  await expect(card(page, name).getByRole("meter")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByRole("button", { name: "Start a task in the copy" }).click();
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Check the totals");
  await input.press("Enter");
  const row = page
    .getByRole("region", { name: "Tasks" })
    .getByRole("link", { name: /Check the totals/ });
  // UI-GUIDELINES §7: the workspace's name is on hover, never a second line, so a long one can't wrap the row.
  await expect(row).toHaveAttribute(
    "title",
    new RegExp(`^Check the totals · ${name} · `),
  );
  await expect(row.locator(".conversation-workspace")).toHaveCount(0);
  const box = await row.boundingBox();
  if (!box) throw new Error("The task row must be visible");
  expect(box.height).toBeLessThanOrEqual(36);
  await capture(page, "long-label");
});

// The UI standard's In-place edit, with a workspace name as its example: Rename makes the name itself the field, in
// the table row and on a phone's card; Enter saves, Esc puts it back; no dialog.
for (const width of [1280, 390])
  test(`WEB-U-092 a workspace is renamed in place ${width}`, async ({
    page,
  }) => {
    await open(page);
    await page.setViewportSize({ width, height: 844 });
    const main = page.getByRole("main");
    await choose(page, "Workspace review", "Rename");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const field = main.getByRole("textbox", { name: "Workspace name" });
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("Workspace review");
    await capture(page, `workspace-rename-${width}`);
    await field.fill("Something else");
    await field.press("Escape");
    await expect(field).toHaveCount(0);
    await expect(
      main.getByText("Workspace review", { exact: true }),
    ).toBeVisible();
    await choose(page, "Workspace review", "Rename");
    await field.fill("Quarterly review");
    await field.press("Enter");
    await expect(field).toHaveCount(0);
    await expect(
      main.getByText("Quarterly review", { exact: true }),
    ).toBeVisible();
  });
