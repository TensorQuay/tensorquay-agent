import { randomUUID } from "node:crypto";

import { pick } from "./choices";
import { signIn } from "./session";
import { axe, capture, fitsColumn, rowMenu, type } from "./settings-page";
import { expect, test, type Page } from "./test";

// The Connectors page on the registry admin API (G2, ADR-0031; WEB-U-043 browser part), keyboard first, at the widths
// UI-GUIDELINES §17.2 asks for in both themes. Each run adds and deletes its own connector in the shared fake store.
const CONNECTORS = "/api/v1/admin/connectors";

async function addConnector(page: Page, name: string) {
  await page.getByRole("button", { name: "Add connector" }).focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Add a connector" });
  await expect(dialog).toBeVisible();
  await type(page, "Name", name);
  await type(page, "Server address", `http://mcp.${name}.example/mcp`);
  await dialog.getByRole("button", { name: "Save connector" }).click();
  await expect(
    dialog.getByText(
      "Use the server’s https:// address, without a name, password or fragment.",
    ),
  ).toBeVisible();
  await type(page, "Server address", `https://mcp.${name}.example/mcp`);
  await pick(page, "Send a credential", "As a bearer token", dialog);
  await pick(page, "Shared key", "spare", dialog);
  await pick(page, "Tools the agent may use", "Only these", dialog);
  await type(page, "Tool names, one per line", "search");
  // PRD FR-10, superseded: no kind of action; no permission rules apply to a connector's tools.
  await expect(dialog.getByLabel("Kind of action")).toHaveCount(0);
  await axe(page);
  // WEB-U-054: the shared secret must be allowed for the connector first; the form offers it.
  await dialog.getByRole("button", { name: "Save connector" }).click();
  await expect(
    dialog.getByText("spare isn't allowed for this connector. Allow it first."),
  ).toBeVisible();
  await dialog.getByRole("button", { name: `Allow it for ${name}` }).focus();
  await page.keyboard.press("Enter");
  await expect(dialog.getByText("Allowed. Save again.")).toBeVisible();
  await axe(page);
  await dialog.getByRole("button", { name: "Save connector" }).click();
  await expect(page.getByText(`Saved connector ${name}.`)).toBeVisible();
  const row = page.getByRole("row", { name: new RegExp(name) });
  // A connector has no class to show any more (ADR-0035: the network allow-list is the only guardrail).
  await expect(row).toContainText("1 tool allowed");
}

async function editAfterConflict(page: Page, name: string) {
  await page.getByRole("button", { name: `Edit connector ${name}` }).click();
  await type(page, "Call timeout (seconds)", "120");
  const changed = await page.request.put(`${CONNECTORS}/${name}`, {
    headers: { "Idempotency-Key": randomUUID() },
    data: { url: `https://mcp.${name}.example/v2/mcp`, version: 1 },
  });
  expect(changed.status()).toBe(200);
  const save = page.getByRole("button", { name: "Save connector" });
  await save.click();
  await expect(
    page.getByText(/^Someone changed this connector since you opened it/),
  ).toBeVisible();
  await save.click();
  await expect(page.getByText(`Saved connector ${name}.`)).toBeVisible();
}

async function deleteConnector(page: Page, name: string) {
  await rowMenu(
    page,
    `More actions for connector ${name}`,
    `Delete connector ${name}`,
  );
  const dialog = page.getByRole("dialog", {
    name: `Delete connector ${name}?`,
  });
  await dialog.getByRole("button", { name: "Delete connector" }).click();
  await expect(page.getByText(`Deleted connector ${name}.`)).toBeVisible();
  await expect(page.getByRole("row", { name: new RegExp(name) })).toHaveCount(
    0,
  );
}

for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-043 connectors for administrators ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const name = "calendar";
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/settings/connectors");
      await expect(page).toHaveTitle("Connectors · TensorQuay Agent");
      const tracker = page.getByRole("row", { name: /Issue tracker/ });
      await expect(tracker).toContainText("Key missing");
      await expect(tracker).toContainText(
        "mcp.tracker.example · Configuration file",
      );
      await fitsColumn(page, /^More actions for connector /);
      await axe(page);
      await capture(page, `connectors-list-${width}-${colorScheme}`);
      await addConnector(page, name);
      await editAfterConflict(page, name);
      await fitsColumn(page, /^More actions for connector /);
      await deleteConnector(page, name);
    });

test("WEB-U-043 members see no Connectors section", async ({ page }) => {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "colleague@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
  await page.goto("/settings/connectors");
  await expect(
    page.getByText("Only administrators can manage connectors."),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Connectors" }),
  ).toHaveCount(0);
});
