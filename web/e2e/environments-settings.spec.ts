import { signIn } from "./session";
import { axe, capture, fitsColumn, rowMenu, type } from "./settings-page";
import { expect, test, type Locator, type Page } from "./test";

// The Environments page on the templates admin API (slice 4, ADR-0032; WEB-U-047 browser part), keyboard first, at the
// widths UI-GUIDELINES §17.2 asks for in both themes. Each run adds, promotes, restores and deletes its own environment.
const DIGEST = `sha256:${"b".repeat(64)}`;

/**
 * Opens Add environment, a page of its own (WEB-U-097), by keyboard, with Advanced open: the image ID is typed only
 * there.
 */
async function openAdd(page: Page): Promise<Locator> {
  await page.getByRole("link", { name: "Add environment" }).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/settings\/environments\/new$/);
  const form = page.locator("form.settings-form-page");
  await expect(
    form.getByRole("heading", { level: 2, name: "Add an environment" }),
  ).toBeVisible();
  await form.getByText("Advanced", { exact: true }).click();
  return form;
}

async function addEnvironment(page: Page, name: string) {
  const form = await openAdd(page);
  await type(page, "Display name", `Audit ${name}`, form);
  await type(page, "Image ID", "tqa-audit:latest", form);
  await form.getByRole("checkbox", { name: "Python" }).check();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(form.getByText(/not by a tag/)).toBeVisible();
  await type(page, "Image ID", `tqa-audit@${DIGEST}`, form);
  await axe(page);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(
    page.getByText(
      `Saved environment Audit ${name}. The gateway checks it next.`,
    ),
  ).toBeVisible();
  // The checker answers, and the list asks again on its own while a check is pending.
  await expect(
    page.getByRole("row", { name: new RegExp(`Audit ${name}`) }),
  ).toContainText("Verified", { timeout: 15_000 });
}

/** Chooses one of an environment's actions from its "…" menu by keyboard (UI-GUIDELINES §7). */
const menu = (page: Page, display: string, item: string) =>
  rowMenu(page, `More actions for environment ${display}`, item);

async function moveDefault(page: Page, display: string) {
  await menu(page, display, `Make ${display} the default`);
  const dialog = page.getByRole("dialog", {
    name: `Make ${display} the default?`,
  });
  await dialog.getByRole("button", { name: "Make default" }).click();
  await expect(page.getByText(`${display} is now the default.`)).toBeVisible();
}

for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-047 environments for administrators ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `e2e-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/environments");
      await expect(page).toHaveTitle("Environments · TensorQuay Agent");
      await expect(
        page.getByRole("row", { name: /^Workstation/ }),
      ).toContainText("Default");
      await expect(
        page.getByRole("row", { name: /Broken tools/ }),
      ).toContainText("does not start from Workstation or Minimal");
      // The default can't be deleted: Delete stays in its menu, unavailable, saying why (UI-GUIDELINES §5b).
      await page
        .getByRole("button", {
          name: "More actions for environment Workstation",
        })
        .click();
      await expect(
        page.getByRole("menuitem", { name: "Delete environment Workstation" }),
      ).toHaveAttribute("aria-disabled", "true");
      await page.keyboard.press("Escape");
      await fitsColumn(page, /^More actions for environment /);
      await axe(page);
      await capture(page, `environments-list-${width}-${colorScheme}`);
      await addEnvironment(page, name);
      await moveDefault(page, `Audit ${name}`);
      await moveDefault(page, "Workstation");
      await menu(page, `Audit ${name}`, `Delete environment Audit ${name}`);
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete environment" })
        .click();
      await expect(
        page.getByText(`Deleted environment Audit ${name}.`),
      ).toBeVisible();
      await fitsColumn(page, /^More actions for environment /);
    });

test("WEB-U-047 says when the default cannot be picked, on Environments and Status", async ({
  page,
}) => {
  const notice = page.getByRole("region", {
    name: "New tasks cannot start in the default environment",
  });
  await page.setViewportSize({ width: 1024, height: 900 });
  await signIn(page);
  await page.goto("/settings/environments");
  await expect(page.getByRole("table", { name: "Environments" })).toBeVisible();
  await expect(notice).toHaveCount(0);
  const form = await openAdd(page);
  await type(page, "Display name", "Fragile tools", form);
  await type(page, "Image ID", `fragile@${DIGEST}`, form);
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("row", { name: /Fragile tools/ })).toContainText(
    "Verified",
    { timeout: 15_000 },
  );
  await moveDefault(page, "Fragile tools");
  // Its engine image changes: checked again, it is refused, and new tasks can no longer start in it.
  await menu(page, "Fragile tools", "Check the image of Fragile tools now");
  await expect(notice).toContainText(
    "Fragile tools is the default, but people cannot pick it now.",
    { timeout: 15_000 },
  );
  // First "not checked yet"; the check's answer arrives with the list's next poll (5 s while a check is pending).
  await expect(notice).toContainText("It has not passed its check yet.");
  await expect(notice).toContainText("agent runtime is too old", {
    timeout: 15_000,
  });
  await axe(page);
  await fitsColumn(page, /^More actions for environment /);
  await notice.getByRole("link", { name: "Go to Fragile tools" }).click();
  await expect(page.getByRole("row", { name: /Fragile tools/ })).toBeFocused();
  await page.goto("/settings/status");
  const status = page.getByRole("region", { name: "Default environment" });
  await expect(status).toContainText("Not available");
  await expect(status).toContainText("agent runtime is too old");
  await axe(page);
  await status.getByRole("link", { name: "Open Environments" }).click();
  await moveDefault(page, "Workstation");
  await expect(notice).toHaveCount(0);
  await menu(page, "Fragile tools", "Delete environment Fragile tools");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Delete environment" })
    .click();
  await expect(
    page.getByText("Deleted environment Fragile tools."),
  ).toBeVisible();
});

test("WEB-U-047 members see no Environments section", async ({ page }) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await page.goto("/settings/environments");
  await expect(
    page.getByText("Only administrators can manage environments."),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Environments" }),
  ).toHaveCount(0);
});

// An image holding credential files in its own layers is refused, naming each path and what it is, never its
// content; an acknowledged one is marked (ADR-0032 amendment; WEB-U-060 browser part): in the row's one muted line,
// whole in its tooltip, and in full on the environment's page (17.3). Each run adds and deletes its own.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-060 credential files in an image ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = `creds-${width}-${colorScheme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/environments");
      const form = await openAdd(page);
      await type(page, "Display name", `Credentials ${name}`, form);
      await type(page, "Image ID", `secrets@${DIGEST}`, form);
      await form.getByRole("button", { name: "Save" }).click();
      const row = page.getByRole("row", {
        name: new RegExp(`Credentials ${name}`),
      });
      await expect(row).toContainText("Refused", { timeout: 15_000 });
      const note = row
        .locator(".cell-note")
        .filter({ hasText: "Its image holds credential files" });
      await expect(note).toContainText(
        "Its image holds credential files that every task would get.",
      );
      await expect(note).toHaveAttribute(
        "title",
        / · \/root\/\.git-credentials: saved logins for a code host · .* · and 2 more · /,
      );
      await fitsColumn(page, /^More actions for environment /);
      await axe(page);
      await capture(
        page,
        `environment-credential-files-${width}-${colorScheme}`,
      );
      await row
        .getByRole("link", { name: `Edit environment Credentials ${name}` })
        .click();
      const status = page.getByRole("region", { name: "Status" });
      await expect(status).toContainText(
        "Its image holds credential files that every task would get.",
      );
      const list = status.getByRole("list", { name: "Credential files found" });
      await expect(list.getByRole("listitem")).toHaveText([
        "/root/.git-credentials: saved logins for a code host",
        "/opt/test/fixture_key: a private key (acknowledged)",
      ]);
      await expect(status).toContainText("and 2 more");
      await axe(page);
      await capture(
        page,
        `environment-credential-files-page-${width}-${colorScheme}`,
      );
      await page.goto("/settings/environments");
      await menu(
        page,
        `Credentials ${name}`,
        `Delete environment Credentials ${name}`,
      );
      await page
        .getByRole("dialog")
        .getByRole("button", { name: "Delete environment" })
        .click();
      await expect(row).toHaveCount(0);
    });
