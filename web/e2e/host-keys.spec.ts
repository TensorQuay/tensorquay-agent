import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { axe, capture } from "./settings-page";
import { expect, resetFront, test, type Page } from "./test";

// Keys for web services (ADR-0037 §9) on the fake front's host-key API, which answers as the gateway's contract says.
// The key's value is typed once and never appears in the page again.
const VALUE = "ghp-e2e-4b3a2f1e0d9c";
const NEW_VALUE = "ghp-e2e-9c8b7a6f5e4d";

const field = (page: Page, name: string) =>
  page.getByLabel(name, { exact: true });

async function signInAsMember(page: Page) {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
}

for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-142 a key added, opened, replaced and deleted ${width} ${colorScheme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/keys");
      await expect(
        page.getByText(
          "No keys for web services yet. Add one for a service your tasks call, such as GitHub.",
        ),
      ).toBeVisible();
      await page
        .getByRole("link", { name: "Add a key for a web service" })
        .click();
      await expect(page).toHaveURL(/\/settings\/keys\/new$/);
      await expect(
        page.getByRole("heading", { name: "Add a key" }),
      ).toBeVisible();
      await field(page, "Web address").fill("https://github.com/acme/");
      await expect(field(page, "Name")).toHaveValue("GITHUB_TOKEN");
      await field(page, "Key").fill(VALUE);
      await axe(page);
      await capture(page, `host-key-add-${width}-${colorScheme}`);
      await page.getByRole("button", { name: "Save", exact: true }).click();

      await expect(page).toHaveURL(/\/settings\/keys$/);
      await expect(page.getByText("Saved GITHUB_TOKEN.")).toBeVisible();
      const row = page.getByRole("row", { name: /GITHUB_TOKEN/ });
      await expect(row).toContainText("github.com/acme");
      // No rule lets tasks reach github.com on the fake front: an administrator is shown where to allow it.
      await expect(row).toContainText("Tasks can't reach github.com yet.");
      await expect(
        row.getByRole("link", { name: "Allow github.com" }),
      ).toHaveAttribute("href", "/settings/internet-access?host=github.com");
      expect(await page.content()).not.toContain(VALUE);
      await axe(page);
      await capture(page, `host-keys-${width}-${colorScheme}`);

      await row.getByRole("link", { name: "GITHUB_TOKEN" }).click();
      await expect(page).toHaveURL(/\/settings\/keys\/[0-9a-f-]{36}$/);
      await page.getByRole("button", { name: "Replace…" }).click();
      await field(page, "New key").fill(NEW_VALUE);
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page.getByText("Saved GITHUB_TOKEN.")).toBeVisible();
      expect(await page.content()).not.toContain(NEW_VALUE);

      await page.getByRole("link", { name: "GITHUB_TOKEN" }).click();
      await page
        .getByRole("button", { name: "More actions for GITHUB_TOKEN" })
        .click();
      await page.getByRole("menuitem", { name: "Delete…" }).click();
      const confirm = page.getByRole("dialog", {
        name: "Delete GITHUB_TOKEN?",
      });
      await axe(page);
      await confirm.getByRole("button", { name: "Delete key" }).click();
      await expect(page.getByText("Deleted GITHUB_TOKEN.")).toBeVisible();
      await expect(page.getByRole("row", { name: /GITHUB_TOKEN/ })).toHaveCount(
        0,
      );
    });

test("WEB-U-142 the Add page says what it can't take, before anything is sent", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/keys/new");
  const address = field(page, "Web address");
  const save = page.getByRole("button", { name: "Save", exact: true });
  await field(page, "Key").fill(VALUE);
  for (const [typed, words] of [
    ["http://api.github.com", "Keys are sent only over https."],
    [
      "https://me@github.com",
      "Use the service's address only, without a name, password, or anything after ? or #.",
    ],
    ["*.github.com", "One exact address; add another key for another host."],
    ["fd00::5", "Put an IPv6 address in brackets, such as [fd00::5]."],
    [
      "api .github.com",
      "That isn't a web address, such as github.com/acme or 10.0.0.5.",
    ],
  ] as const) {
    await address.fill(typed);
    await save.click();
    await expect(page.getByText(words)).toBeVisible();
  }
  await address.fill("github.com");
  await field(page, "Name").fill("PATH");
  await save.click();
  await expect(
    page.getByText("PATH is kept for the task's own use. Pick another name."),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/settings\/keys\/new$/);
});

// ADR-0037 Amendment 2: a task's "Add a key for {address}" for an IPv6 address, through to the rule that lets tasks
// reach exactly that address.
test("WEB-U-141 a key for an exact IPv6 address, and the rule it needs", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(
    `/settings/keys/new?address=${encodeURIComponent("[fd00::5]:8443")}`,
  );
  await expect(field(page, "Web address")).toHaveValue("[fd00::5]:8443");
  await expect(field(page, "Name")).toHaveValue("HOST_FD00_5_TOKEN");
  await field(page, "Key").fill(VALUE);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  const row = page.getByRole("row", { name: /HOST_FD00_5_TOKEN/ });
  await expect(row).toContainText("[fd00::5]:8443");
  await expect(row).toContainText(
    "Tasks can't reach fd00::5 yet: an address needs a rule for exactly that address.",
  );
  await row.getByRole("link", { name: "Allow fd00::5" }).click();
  const add = page.getByRole("dialog", { name: "Add a rule" });
  await expect(add.getByLabel("Host")).toHaveValue("fd00::5");
  await expect(add.getByLabel("Ports")).toHaveValue("8443");
  await add.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText("Saved rule fd00::5.")).toBeVisible();
  await page.goto("/settings/keys");
  await expect(
    page.getByRole("row", { name: /HOST_FD00_5_TOKEN/ }),
  ).not.toContainText("Tasks can't reach");
});

test("WEB-U-141 a refused last try in words", async ({ page, request }) => {
  await resetFront(request, ["host-key-refused"]);
  await signIn(page);
  await page.goto("/settings/keys");
  const row = page.getByRole("row", { name: /GITHUB_WORK/ });
  await expect(row).toContainText(
    "Last try refused: the key was used by something left over from an earlier run.",
  );
  await expect(row).not.toContainText("foreign_run");
});

// The Keys sketch §6, decision 2: a member reads the shared keys they may use; an administrator manages them.
test("WEB-U-146 members read the shared keys they may use", async ({
  page,
  request,
}) => {
  await resetFront(request, ["shared-host-keys"]);
  await signInAsMember(page);
  await page.goto("/settings/keys");
  await page.getByRole("link", { name: "Shared" }).click();
  await expect(page).toHaveURL(/\/settings\/keys\/shared$/);
  for (const name of ["GITHUB_TEAM", "JIRA_TEAM"])
    await expect(
      page.getByRole("row", { name: new RegExp(name) }),
    ).toBeVisible();
  await expect(
    page.getByRole("link", { name: /Add a shared key/ }),
  ).toHaveCount(0);
  await axe(page);
  await page.goto("/settings/keys/shared/new");
  await expect(
    page.getByText("Only administrators can manage shared keys."),
  ).toBeVisible();
});

test("WEB-U-145 administrators see who may use each shared key", async ({
  page,
  request,
}) => {
  await resetFront(request, ["shared-host-keys"]);
  await signIn(page);
  await page.goto("/settings/keys/shared");
  await expect(page.getByRole("row", { name: /GITHUB_TEAM/ })).toContainText(
    "Everyone",
  );
  await expect(page.getByRole("row", { name: /JIRA_TEAM/ })).toContainText(
    "1 person",
  );
  await axe(page);
});

// WEB-U-262 (a first-hour check before 17.0): the Keys page heads each kind of key and says what it is for; right after
// a key for a web service is saved, the connectors' section never reads as "no keys", and with a connector that takes a
// person's key it offers its own add.
for (const width of [1280, 390] as const)
  test(`WEB-U-262 each kind of key is headed and says what it is for ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await signIn(page);
    await page.goto("/settings/keys");
    await expect(
      page.getByRole("heading", { level: 2, name: "Keys for web services" }),
    ).toBeVisible();
    await expect(
      page.getByText(
        "For a service your tasks call over the web, such as GitHub or a package registry. A task uses the key by its name and never sees it.",
      ),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { level: 2, name: "Keys for connectors" }),
    ).toBeVisible();
    await expect(
      page.getByText(
        /^For a connector your administrator set up that works as you/,
      ),
    ).toBeVisible();
    await page
      .getByRole("link", { name: "Add a key for a web service" })
      .click();
    await field(page, "Web address").fill("https://github.com/acme/");
    await field(page, "Key").fill(VALUE);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByText("Saved GITHUB_TOKEN.")).toBeVisible();
    // The key just saved is never contradicted below it.
    await expect(page.getByText(/No keys yet/)).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Add a key for a connector" }),
    ).toBeVisible();
    await axe(page);
    await capture(page, `keys-sections-${width}`);
  });

test("WEB-U-262 no connector takes a key: the section says so and offers no add", async ({
  page,
}) => {
  await signIn(page);
  // The fake front's only connector that takes a person's key goes (resetFront brings it back for the next test).
  const gone = await page.request.delete(
    "/api/v1/admin/connectors/tracker?version=1",
    { headers: { "Idempotency-Key": randomUUID() } },
  );
  expect(gone.status()).toBe(204);
  await page.goto("/settings/keys");
  await expect(
    page.getByText("None of the connectors here needs a key of yours."),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Add a key for a connector" }),
  ).toHaveCount(0);
  await expect(page.getByText(/No keys yet/)).toHaveCount(0);
  await axe(page);
});
