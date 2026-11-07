import { auditDialog, record } from "./a11y";
import { openChoices } from "./choices";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// Phase 9 accessibility pass (WEB-U-007): every dialog and menu opened by keyboard, in both themes, as findings.
const button = (page: Page, name: string) =>
  page.getByRole("button", { name, exact: true });

// One settings section per test (and a task's, and the workspaces', below): walking every dialog in one test outgrew
// its 30 s under host load (2,538 actions), and a per-section test names the section that fails.
const SETTINGS_DIALOGS: readonly [string, string[]][] = [
  [
    "secrets",
    [
      "Add a shared key",
      "Replace shared key local",
      "More actions for shared key local › Set the reminder for shared key local",
      "More actions for shared key spare › Delete shared key spare",
    ],
  ],
  [
    "models",
    [
      // Add and Edit open the route's page (WEB-U-115); its check and delete stay dialogs.
      "More actions for route Local model › Check route Local model",
      "More actions for route DeepSeek Flash › Delete route DeepSeek Flash",
    ],
  ],
  [
    "internet-access",
    [
      "Add a site",
      "Edit rule pypi.org",
      "More actions for rule pypi.org › Delete rule pypi.org",
      // Open web asks first (WEB-U-118); Escape leaves the list as it was.
      "Add Open web: any website (ports 80 and 443)",
    ],
  ],
  ["limits", ["Edit Tasks running at once"]],
  [
    "connectors",
    [
      "Add connector",
      "Edit connector wiki",
      "More actions for connector wiki › Delete connector wiki",
    ],
  ],
  ["keys", ["Add your key for Issue tracker"]],
  [
    "environments",
    [
      // Add and Edit open the environment's page (WEB-U-097), which WEB-E2E-031 checks; these are its dialogs.
      "More actions for environment Minimal › Make Minimal the default",
      "More actions for environment Data science › Reset Data science to built-in",
      "More actions for environment Broken tools › Delete environment Broken tools",
    ],
  ],
  [
    "agent-profiles",
    [
      // Add, Edit and Duplicate open the profile's page (WEB-U-104), which WEB-E2E-033 checks; Delete is its dialog.
      "More actions for agent profile Team default › Delete agent profile Team default",
    ],
  ],
];

async function settingsDialogs(
  page: Page,
  theme: string,
  section: string,
  triggers: string[],
) {
  const findings: string[] = [];
  await page.goto(`/settings/${section}`);
  await expect(page.locator(".skeleton")).toHaveCount(0, { timeout: 15_000 });
  for (const name of triggers) {
    // "Menu › Item": a row's action from its "…" menu; focus returns to the menu's button.
    const [menuName, item] = name.split(" › ");
    const more = button(page, menuName ?? name);
    if (item) await more.click();
    findings.push(
      ...(await auditDialog(
        page,
        item ? page.getByRole("menuitem", { name: item }) : more,
        `${theme} /settings/${section} "${name}"`,
        item ? { returnsTo: more } : {},
      )),
    );
  }
  return findings;
}

async function taskDialogs(page: Page, theme: string) {
  const findings: string[] = [];
  await page.goto(await createConversation(page));
  await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
  const menu = { role: "menu" as const, modal: false };
  // The model and environment menus sit in the header's Change panel (UI-GUIDELINES §0).
  await openChoices(page);
  findings.push(
    ...(await auditDialog(
      page,
      button(page, "Account"),
      `${theme} task "Account"`,
      menu,
    )),
    ...(await auditDialog(
      page,
      page.getByRole("button", { name: /^Model: / }),
      `${theme} task "Model"`,
      menu,
    )),
    ...(await auditDialog(
      page,
      page.getByRole("button", { name: /^Environment: / }),
      `${theme} task "Environment"`,
      menu,
    )),
    ...(await auditDialog(
      page,
      page
        .getByRole("region", { name: "Tasks" })
        .getByRole("button", { name: /^More actions for / })
        .first(),
      `${theme} task "More actions"`,
      menu,
    )),
  );
  const more = page
    .getByRole("region", { name: "Tasks" })
    .getByRole("button", { name: /^More actions for / })
    .first();
  await more.click();
  findings.push(
    ...(await auditDialog(
      page,
      page.getByRole("menuitem", { name: "Delete task" }),
      `${theme} task "Delete task"`,
      { returnsTo: more },
    )),
  );
  return findings;
}

/** Audits a workspace action's dialog, opened from the first workspace's menu; focus returns to that menu (UI-GUIDELINES §7). */
async function fromMenu(page: Page, action: string, where: string) {
  const more = page
    .getByRole("main")
    .getByRole("button", { name: /^More actions for / })
    .first();
  await more.click();
  return auditDialog(
    page,
    page.getByRole("menuitem", { name: action, exact: true }),
    where,
    { returnsTo: more },
  );
}

async function workspaceDialogs(page: Page, theme: string) {
  const findings: string[] = [];
  // A task makes the workspace whose Share, Fork and More dialogs are audited.
  await page.goto(await createConversation(page));
  await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
  const menu = { role: "menu" as const, modal: false };
  await page.goto("/workspaces");
  await expect(page.locator(".skeleton")).toHaveCount(0, { timeout: 15_000 });
  findings.push(
    ...(await auditDialog(
      page,
      button(page, "New workspace"),
      `${theme} /workspaces "New workspace"`,
    )),
    ...(await fromMenu(page, "Share", `${theme} /workspaces "Share"`)),
    ...(await fromMenu(
      page,
      "Make a copy",
      `${theme} /workspaces "Make a copy"`,
    )),
    ...(await auditDialog(
      page,
      page
        .getByRole("main")
        .getByRole("button", { name: /^More actions for / })
        .first(),
      `${theme} /workspaces "More"`,
      menu,
    )),
  );
  return findings;
}

for (const colorScheme of ["light", "dark"] as const) {
  for (const [section, triggers] of SETTINGS_DIALOGS)
    test(`A11Y /settings/${section} dialogs ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      record(await settingsDialogs(page, colorScheme, section, triggers));
    });

  test(`A11Y task dialogs ${colorScheme}`, async ({ page }) => {
    // Five audits in one test take 4-6 s, and over 30 s on a host running ~10x slow (TEST-PLAN §19a): a budget sized to
    // them, as the audits after a change have.
    test.setTimeout(90_000);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    record(await taskDialogs(page, colorScheme));
  });

  test(`A11Y workspace dialogs ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    record(await workspaceDialogs(page, colorScheme));
  });
}

// The dialogs that need data first: a created token and its revocation, an invitation, a key for a web service, and Stop.
test("A11Y dialogs after a change", async ({ page }) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  const findings: string[] = [];
  await page.goto("/settings/tokens");
  // A name of its own, so repeated runs on the shared fake front never find two tokens to revoke.
  const token = `Audit ${Date.now()}`;
  await page.getByRole("textbox", { name: "Name" }).fill(token);
  findings.push(
    ...(await auditDialog(
      page,
      button(page, "Create token"),
      '/settings/tokens "Token created"',
    )),
    ...(await auditDialog(
      page,
      button(page, `Revoke token “${token}”`),
      '/settings/tokens "Revoke"',
    )),
  );
  await page.goto("/settings/people");
  await page
    .getByRole("textbox", { name: "Email" })
    .fill("audit@example.invalid");
  findings.push(
    ...(await auditDialog(
      page,
      button(page, "Invite"),
      '/settings/people "Invitation created"',
    )),
  );
  // A key for a web service: its page replaces the value in place, so its one dialog is Delete, in the page's ⋯.
  await page.goto("/settings/keys/new");
  await page
    .getByLabel("Web address", { exact: true })
    .fill("https://git.example.com/team/audit");
  await page.getByLabel("Name", { exact: true }).fill("AUDIT_TOKEN");
  await page.getByLabel("Key", { exact: true }).fill("audit-token-1");
  await button(page, "Save").click();
  await expect(page.getByText("Saved AUDIT_TOKEN.")).toBeVisible();
  await page
    .getByRole("row", { name: /AUDIT_TOKEN/ })
    .getByRole("link", { name: "AUDIT_TOKEN" })
    .click();
  const more = button(page, "More actions for AUDIT_TOKEN");
  await more.click();
  findings.push(
    ...(await auditDialog(
      page,
      page.getByRole("menuitem", { name: "Delete…" }),
      '/settings/keys/[id] "Delete"',
      { returnsTo: more },
    )),
  );
  // Stop has no dialog since c9: it acts at once.
  record(findings);
});
