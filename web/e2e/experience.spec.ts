import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { closeChoices, expectModel, openChoices } from "./choices";
import { chooseModel } from "./model";
import { settled } from "./motion";
import { signIn } from "./session";
import { expect, test, type Page } from "./test";
import { humanChrome } from "./ux/chrome";

async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

async function setup(page: Page, suffix: string) {
  await page.goto("/login");
  await expect(page).toHaveURL(/\/setup$/);
  await page
    .getByLabel("Email", { exact: true })
    .fill("member@example.invalid");
  await page
    .getByLabel("Password", { exact: true })
    .fill("synthetic-browser-password");
  await page.getByLabel("Setup token").fill("wrong");
  await page.getByRole("button", { name: "Create administrator" }).click();
  const setupError = page.getByRole("main").getByRole("alert");
  await expect(setupError).toHaveText(
    "That setup token is not correct. Copy it again from compose.env.",
  );
  await expect(page).toHaveURL(/\/setup$/);
  // WEB-U-380: the refusal marks the token field and puts focus back on it, not on the page (F-P02-1).
  await expect(page.getByLabel("Setup token")).toBeFocused();
  await expect(page.getByLabel("Setup token")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  // WEB-U-179: the variable is named once on the page, in the help.
  await expect(page.getByText(/TQA_BOOTSTRAP_TOKEN/)).toHaveCount(1);
  await capture(page, `setup-error-${suffix}`);
  await page.getByLabel("Setup token").fill("synthetic-browser-bootstrap");
  await expect(page.getByLabel("At least 12 characters: met")).toBeVisible();
  await page.getByRole("button", { name: "Show password" }).click();
  await expect(page.getByLabel("Password", { exact: true })).toHaveAttribute(
    "type",
    "text",
  );
  await page.getByRole("button", { name: "Hide password" }).click();
  await capture(page, `setup-${suffix}`);
  await page.getByRole("button", { name: "Create administrator" }).click();
  await expect(
    page,
    "auto sign-in must reach the composer without a second form",
  ).toHaveURL("/");
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  expect((await page.request.get("/api/v1/auth/me")).status()).toBe(200);
}

async function renameAndChoose(page: Page, suffix: string) {
  const bar = page.getByRole("region", { name: "Task header" });
  await bar.getByRole("heading", { level: 1 }).click();
  const field = bar.getByRole("textbox", { name: "Task title" });
  await field.fill("A workspace tour");
  await field.press("Enter");
  await expect(
    bar.getByRole("heading", { name: "A workspace tour" }),
  ).toBeVisible();
  await expect(page).toHaveTitle("A workspace tour · TensorQuay Agent");
  await chooseModel(page, "second");
  await closeChoices(page);
  await expectModel(page, "Second model");
  await expect(
    page.getByRole("form", { name: "Message composer" }).getByRole("combobox"),
  ).toHaveCount(0);
  await page.reload();
  await expect(
    bar.getByRole("heading", { name: "A workspace tour" }),
  ).toBeVisible();
  await expectModel(page, "Second model");
  await capture(page, `conversation-bar-${suffix}`);
}

for (const width of [1024, 1440])
  for (const theme of ["light", "dark"] as const) {
    test(`WEB-E2E-018 first run and conversation bar ${width} ${theme}`, async ({
      page,
      baseURL,
    }) => {
      if (!baseURL) throw new Error("The fake front must have a base URL");
      const suffix = `${width}-${theme}`;
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme: theme });
      await page
        .context()
        .addCookies([
          { name: "fixture_setup", value: "required", url: baseURL },
        ]);
      await setup(page, suffix);
      await expect(page).toHaveTitle("New task · TensorQuay Agent");
      await expect(
        page
          .getByRole("navigation")
          .locator("[disabled], [aria-disabled=true]"),
      ).toHaveCount(0);
      await capture(page, `start-${suffix}`);
      await page
        .getByRole("button", {
          name: "Draft a one-page plan for launching a newsletter",
        })
        .click();
      await expect(page.getByLabel("Message", { exact: true })).toHaveValue(
        "Draft a one-page plan for launching a newsletter",
      );
      await expect(page).toHaveURL("/");
      // The fake's run is held after its narration until resumed below, so the live run is really there to check: a
      // 2 s run once ended before the test looked.
      await page
        .context()
        .addCookies([
          { name: "fixture_hold", value: "narration", url: baseURL },
        ]);
      await page.getByRole("button", { name: "Send", exact: true }).click();
      await expect(page).toHaveURL(/\/c\/[^/]+$/);
      const task = page.url().split("/").at(-1) ?? "";
      await expect(page.getByLabel("Run status")).toHaveText(/Working…/);
      await openChoices(page);
      await expect(
        page.getByRole("button", { name: /^Model:/ }),
      ).toHaveAttribute("aria-disabled", "true");
      await closeChoices(page);
      // WEB-U-155 (a screen review before 17.0): one word while working; the count is in its tooltip.
      await expect(page.getByLabel("Run status")).toHaveAttribute(
        "data-short",
        "Working…",
      );
      await capture(page, `working-${suffix}`);
      // Held once its narration shows; a resume sent earlier is ignored, and the run would then stay held.
      await expect(
        page.getByText("I’ll check the project files."),
      ).toBeVisible();
      await page.context().clearCookies({ name: "fixture_hold" });
      expect(
        (
          await page.request.post(
            `/api/v1/conversations/${task}/fixture-resume`,
          )
        ).status(),
      ).toBe(204);
      await expect(page.getByLabel("Run status")).toHaveText("Done");
      await expect(
        page.locator(".markdown-answer"),
        "narration: only the final answer appears outside work",
      ).toHaveCount(1);
      const narration = page
        .getByRole("group", { name: "Activity" })
        .locator(".activity-narration");
      await expect(narration).toHaveCount(1);
      await expect(narration, "narration: folds with work").not.toBeVisible();
      await page.getByRole("button", { name: /Worked for/ }).click();
      await expect(
        narration,
        "narration: remains accessible inside work",
      ).toBeVisible();
      await page.getByRole("button", { name: /Worked for/ }).click();
      await renameAndChoose(page, suffix);
      await humanChrome(page);
      await expect(page.locator(".conversation-bar")).not.toContainText(
        /sandbox|idle/i,
      );
      await page.getByRole("button", { name: "Account", exact: true }).click();
      await expect(
        page
          .getByRole("menu", { name: "Account" })
          .getByRole("menuitem", { name: "Settings", exact: true }),
      ).toBeVisible();
      await settled(page);
      await capture(page, `account-${suffix}`);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await page.getByRole("menuitem", { name: "Sign out" }).click();
      await expect(page).toHaveURL(/\/login$/);
      await capture(page, `login-${suffix}`);
      await page
        .getByLabel("Email", { exact: true })
        .fill("member@example.invalid");
      await page.getByLabel("Password", { exact: true }).fill("wrong");
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await expect(page.getByRole("main").getByRole("alert")).toContainText(
        "Email or password is incorrect",
      );
      // WEB-U-380: the refusal marks the password and puts focus back on it, not on the page (F-P01-1).
      const password = page.getByLabel("Password", { exact: true });
      await expect(password).toBeFocused();
      await expect(password).toHaveAttribute("aria-invalid", "true");
      await capture(page, `login-error-${suffix}`);
    });
  }

test("WEB-E2E-018 an accepted action failure ends Sending and retries with a new key", async ({
  page,
}) => {
  await signIn(page);
  const keys: (string | undefined)[] = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/messages"))
      keys.push(request.headers()["idempotency-key"]);
  });
  await page.goto("/");
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  await page
    .getByLabel("Message", { exact: true })
    .fill("Fail before starting");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page).toHaveURL(/\/c\//);
  await expect(
    page
      .getByRole("article", { name: "Your message" })
      .getByText(
        "Not sent. The agent stopped while starting. Send a message to try again; if it happens again, ask an administrator to rebuild the environment.",
      ),
  ).toBeVisible();
  await humanChrome(page);
  await expect(page.locator(".conversation-page")).not.toContainText(
    /sandbox|runtime_start_failed/i,
  );
  await expect(
    page.getByRole("button", { name: "Sending", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByLabel("Message", { exact: true })).toHaveValue(
    "Fail before starting",
  );
  await page.getByRole("button", { name: "Retry message" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/Done/);
  await expect(page.getByRole("article", { name: "Your message" })).toHaveCount(
    1,
  );
  expect(keys).toHaveLength(2);
  expect(keys[0]).toMatch(/^[a-f0-9-]{36}$/);
  expect(keys[1]).not.toBe(keys[0]);
});
