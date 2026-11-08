import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";
import {
  auditFocus,
  auditHover,
  auditView,
  type Finding,
} from "./ux/polish-audit";

async function evidence(page: Page, name: string, findings: Finding[]) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `polish-${name}.png`) });
  await writeFile(
    join(directory, `polish-${name}.json`),
    JSON.stringify(findings, null, 2),
  );
}

// WEB-U-021: every main view at the three designed widths in both themes follows UI-GUIDELINES §5b and §10 copy.
for (const width of [1024, 1440, 1920])
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-021 polish audit ${width} ${colorScheme}`, async ({
      page,
      baseURL,
    }) => {
      if (!baseURL) throw new Error("The fake front must have a base URL");
      const deep = width === 1440 && colorScheme === "light";
      // Every view, and at 1440 light also focus and hover on each control: longer than the default budget.
      test.setTimeout(deep ? 180_000 : 90_000);
      const suffix = `${width}-${colorScheme}`;
      const findings: Finding[] = [];
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      async function audit(view: string) {
        const found = [
          ...(await auditView(page, view)),
          ...(deep ? await auditFocus(page, view) : []),
          ...(deep ? await auditHover(page, view) : []),
        ];
        findings.push(...found);
        await evidence(page, `${view}-${suffix}`, found);
      }

      await page
        .context()
        .addCookies([
          { name: "fixture_setup", value: "required", url: baseURL },
        ]);
      await page.goto("/setup");
      await expect(page.getByLabel("Setup token")).toBeVisible();
      await audit("setup");
      await page.context().clearCookies();
      await page.goto("/login");
      await expect(page.getByLabel("Email", { exact: true })).toBeVisible();
      await audit("login");

      await signIn(page);
      await page.goto("/");
      await expect(
        page.getByRole("textbox", { name: "Message", exact: true }),
      ).toBeFocused();
      await audit("start");
      await page.getByRole("button", { name: "Account" }).click();
      await expect(page.getByRole("menu", { name: "Account" })).toBeVisible();
      await audit("account");
      await page.keyboard.press("Escape");

      await page.goto(await createConversation(page));
      const input = page.getByRole("textbox", { name: "Message", exact: true });
      await input.fill("Summarize the workspace.");
      await input.press("Enter");
      await expect(page.getByLabel("Run status")).toHaveText("Done");
      await page.getByRole("button", { name: /Worked for/ }).click();
      await audit("task");

      // Settings, the users table with a shown-once link, a person's tasks, the private view and joining.
      await page.goto("/settings/tokens");
      await page.getByRole("textbox", { name: "Name" }).fill("Audit");
      await page.getByRole("button", { name: "Create token" }).click();
      await page
        .getByRole("dialog", { name: "Token created" })
        .getByRole("button", { name: "I’ve copied it" })
        .click();
      await expect(page.getByRole("row", { name: /Audit/ })).toBeVisible();
      await audit("tokens");
      await page.goto("/settings/people");
      await page
        .getByRole("textbox", { name: "Email" })
        .fill("audit@example.invalid");
      await page.getByRole("button", { name: "Invite", exact: true }).click();
      const invited = page.getByRole("dialog", { name: "Invitation created" });
      const link = await invited
        .getByRole("textbox", { name: "Invitation link" })
        .inputValue();
      await audit("invited");
      await invited.getByRole("button", { name: "I’ve copied it" }).click();
      await audit("users");
      await page
        .getByRole("row", { name: /colleague@example\.invalid/ })
        .getByRole("link", { name: "2 tasks" })
        .click();
      const personTasks = page.getByRole("table", {
        name: "Tasks of colleague@example.invalid",
      });
      await expect(personTasks.getByRole("link")).toHaveCount(2);
      await audit("person-tasks");
      await personTasks.getByRole("link").first().click();
      await expect(
        page.getByRole("heading", { level: 1, name: "Private task" }),
      ).toBeVisible();
      await audit("private-task");
      // The Workspaces page and its share dialog.
      await page.goto("/workspaces");
      await expect(
        page.getByRole("table", { name: "Workspaces" }),
      ).toBeVisible();
      await audit("workspaces");
      await page
        .getByRole("main")
        .getByRole("button", { name: /^More actions for / })
        .first()
        .click();
      await page.getByRole("menuitem", { name: "Share", exact: true }).click();
      await expect(page.getByRole("dialog", { name: /^Share / })).toBeVisible();
      await expect(page.getByText("Not shared with anyone yet.")).toBeVisible();
      await audit("share");
      await page.keyboard.press("Escape");
      await page.context().clearCookies();
      await page.goto(new URL(link).pathname);
      await expect(page.getByLabel("Password", { exact: true })).toBeVisible();
      await audit("join");

      expect(findings, "UI-GUIDELINES §5b / §10 findings").toEqual([]);
    });
