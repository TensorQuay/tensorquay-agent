import { readFile, writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { addFromPlus, picker } from "./choices";
import { afterStart, budget } from "./stack-start";
import {
  absentFromPage,
  EMAIL,
  env,
  finalFlush,
  flushed,
  watchResponses,
} from "./stack-watch";

// WEB-E2E-010 (profile half, ADR-0012): on the real stack, a person adds their connector credential, makes a profile
// that brings the connector (off by default, so only the profile can) and starts a task from it; the connector call
// runs at once (nothing asks first, ADR-0035) and the tool's answer and line appear. The credential never reaches the
// page or a response. The driver (web_connector.py --profile) checks the task's profile, its connectors, the remote,
// events, database and logs.
test.skip(
  process.env.TQA_STACK_SUITE !== "profile",
  "Requires the owned connector fixture in profile mode",
);
test.use({ trace: "retain-on-failure" });

const PROFILE = "tracker-work";
const DISPLAY = "Tracker work";

async function addCredential(page: Page, token: string) {
  await page.goto("/settings/keys");
  await page
    .getByRole("button", {
      name: `Add your key for ${env("TQA_CONNECTOR_NAME")}`,
    })
    .focus();
  await page.keyboard.press("Enter");
  await page
    .getByRole("dialog")
    .getByLabel("Header value", { exact: true })
    .focus();
  await page.keyboard.type(token);
  await page.keyboard.press("Enter");
  await expect(page.getByText(/^Saved your key for /)).toBeVisible();
}

test("WEB-E2E-010 a task made from a profile calls the connector the profile brings, never showing the credential", async ({
  page,
}) => {
  test.setTimeout(budget(1, 210_000));
  const forms = JSON.parse(
    await readFile(env("TQA_CONNECTOR_FORMS_FILE"), "utf8"),
  ) as string[];
  const token = (
    await readFile(env("TQA_CONNECTOR_TOKEN_FILE"), "utf8")
  ).trim();
  const connector = env("TQA_CONNECTOR_NAME");
  const seen = await watchResponses(page, forms);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(EMAIL);
  await page
    .getByLabel("Password", { exact: true })
    .fill(env("TQA_STACK_PASSWORD"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();

  await flushed(seen);
  await addCredential(page, token);
  await absentFromPage(page, forms);

  // The profile: the stack's route and the connector, which no task gets otherwise.
  await flushed(seen);
  await page.goto("/settings/agent-profiles");
  // Add opens the profile's own page (WEB-U-104); its ID follows the display name.
  await page.getByRole("link", { name: "Add agent profile" }).click();
  const form = page.locator("form.settings-form-page");
  await form.getByLabel("Display name", { exact: true }).fill(DISPLAY);
  await expect(form).toContainText(`ID: ${PROFILE} · Change`);
  // The stack's one route (the standard picker, WEB-U-134): the choice after "The task chooses".
  await picker(page, "Model", form).click();
  await page.getByRole("menuitemradio").nth(1).click();
  await form.getByLabel(connector, { exact: true }).check();
  await form.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText(`Saved agent profile ${DISPLAY}.`)).toBeVisible();
  await expect(
    page
      .getByRole("table", { name: "Your agent profiles" })
      .getByRole("row", { name: new RegExp(DISPLAY) }),
  ).toContainText("Ready");

  // New task from the profile: the model is fixed by it.
  await flushed(seen);
  await page.goto("/");
  // "+" adds the profile chip, its picker open (the UI standard's composer).
  await addFromPlus(page, "Agent profile…");
  await page.getByRole("menuitemradio", { name: new RegExp(DISPLAY) }).click();
  await expect(page.getByRole("button", { name: /^Model: / })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("File the crash as an issue in the tracker.");
  await message.press("Enter");
  await expect(page).toHaveURL(/\/c\//);
  const conversationId = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  // The task header names the profile the task came from: "Agent: <display name>".
  await expect(page.getByText(`Agent: ${DISPLAY}`)).toBeVisible();

  await expect(page.getByText("Created issue #7 in the tracker.")).toBeVisible({
    timeout: afterStart(30_000),
  });
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  await page.locator(".activity-heading").last().click();
  await expect(
    page.getByText(`Used create_issue on ${connector}`),
  ).toBeVisible();
  await absentFromPage(page, forms);

  await test.step("search the finished response bodies", () =>
    finalFlush(page, seen));
  await writeFile(
    env("TQA_STACK_RESULT"),
    JSON.stringify({
      conversationId,
      responses: seen.responses,
      unsearched: [...seen.open.values(), ...seen.cut],
      found: forms.flatMap((form, index) =>
        seen.found.has(form) ? [`form ${index}`] : [],
      ),
    }),
  );
});
