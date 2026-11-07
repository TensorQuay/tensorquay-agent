import { readFile, writeFile } from "node:fs/promises";

import { expect, test } from "@playwright/test";

import { afterStart, budget } from "./stack-start";
import {
  absentFromPage,
  EMAIL,
  env,
  finalFlush,
  flushed,
  watchResponses,
} from "./stack-watch";

// WEB-E2E-010 (connector half, ADR-0031): on the real stack, a person adds their own connector credential, then the
// task's connector call runs with it (nothing asks first, ADR-0035); its step names the connector and the tool, and its
// Details hold every argument of the call; the tool's answer appears. The
// credential and its encoded forms must never reach the page or any response the browser receives. The driver
// (crates/tqa-sandbox/tests/egress/web_connector.py) seeds the connector and checks the remote, events, database and logs.
test.skip(
  !process.env.TQA_CONNECTOR_CONVERSATION,
  "Requires the owned connector fixture",
);

// A failure keeps its trace (what the page was waiting on); the driver's evidence directory is private to the run.
test.use({ trace: "retain-on-failure" });

/** A driver-supplied name as literal text in a pattern. */
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

test("WEB-E2E-010 a connector call runs with the person's credential and never shows it", async ({
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
  const args = JSON.parse(env("TQA_CONNECTOR_ARGUMENTS")) as Record<
    string,
    unknown
  >;
  const seen = await watchResponses(page, forms);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/login");
  await page.getByLabel("Email", { exact: true }).fill(EMAIL);
  await page
    .getByLabel("Password", { exact: true })
    .fill(env("TQA_STACK_PASSWORD"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();

  // The administrator sees the seeded connector (no display name: the file cannot set one) offered in new tasks, with its credential required from each person.
  await flushed(seen);
  await page.goto("/settings/connectors");
  const row = page.getByRole("row", { name: /\btracker\b/ });
  await expect(row).toContainText("In new tasks");
  await expect(row).not.toContainText("Key missing");

  // The person adds their own credential by keyboard; the page never shows it again.
  await flushed(seen);
  await page.goto("/settings/keys");
  const own = page.getByRole("row", { name: /\btracker\b/ });
  await expect(own).toContainText("Needed");
  await page
    .getByRole("button", { name: `Add your key for ${connector}` })
    .focus();
  await page.keyboard.press("Enter");
  const field = page
    .getByRole("dialog")
    .getByLabel("Header value", { exact: true });
  await field.focus();
  await page.keyboard.type(token);
  await page.keyboard.press("Enter");
  await expect(
    page.getByText(`Saved your key for ${connector}.`),
  ).toBeVisible();
  await expect(own).toContainText("Added");
  await absentFromPage(page, forms);

  // The call runs at once with the person's credential; the answer arrives.
  await flushed(seen);
  await page.goto(`/c/${env("TQA_CONNECTOR_CONVERSATION")}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("File the crash as an issue in the tracker.");
  await message.press("Enter");
  await expect(page.getByText("Created issue #7 in the tracker.")).toBeVisible({
    timeout: afterStart(30_000),
  });
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  // The turn's activity folds away once the answer arrives; opened, it names the connector call.
  await page.locator(".activity-heading").last().click();
  // The latest step of a finished turn is already open, so the spec opens the line only if it isn't, until it stays
  // open. (A first run's "collapse" was the spec's own click toggling it shut, not the page; a separate fold of a step
  // opened while the turn runs was a real bug, fixed in WEB-U-016.) The click count is printed for the record.
  const line = page.getByRole("button", {
    name: new RegExp(`^Used create_issue on ${escape(connector)}`),
  });
  let clicks = 0;
  await expect(async () => {
    if ((await line.getAttribute("aria-expanded")) !== "true") {
      clicks += 1;
      await line.click();
    }
    await expect(line).toHaveAttribute("aria-expanded", "true", {
      timeout: 2_000,
    });
  }).toPass({ timeout: 30_000 });
  console.log(
    `WEB-E2E-010: the step line stayed open after ${clicks} click(s)`,
  );
  // The step's Details hold exactly the call's arguments.
  const card = page.getByRole("article", { name: /create_issue tool call$/ });
  await card.getByRole("button", { name: "Details", exact: true }).click();
  const shown = JSON.parse(
    (await card.getByLabel("All arguments").textContent()) ?? "",
  ) as Record<string, unknown>;
  expect(shown).toEqual(args);
  await absentFromPage(page, forms);

  await test.step("search the finished response bodies", () =>
    finalFlush(page, seen));
  await writeFile(
    env("TQA_STACK_RESULT"),
    JSON.stringify({
      conversationId: env("TQA_CONNECTOR_CONVERSATION"),
      responses: seen.responses,
      // Bodies not searched here, by path and type only: still open at the end, or cut off.
      unsearched: [...seen.open.values(), ...seen.cut],
      // Which forms were found (never their values): an empty list is the pass.
      found: forms.flatMap((form, index) =>
        seen.found.has(form) ? [`form ${index}`] : [],
      ),
    }),
  );
});
