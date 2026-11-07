import { writeFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget } from "./stack-start";

// WEB-E2E-029 (connector call): on the real stack, the driver (crates/tqa-sandbox/tests/egress/web_key_use.py) makes
// the read-audit table unwritable while the task's first model reply is held, so the reply's connector call can't use
// the person's credential. The call's step and the run say so in plain words, never as a provider's or connector's
// error. Every value in the result file is a read; the driver checks it, and that nothing more reached the remote.
test.skip(
  !process.env.TQA_CONNECTOR_CONVERSATION ||
    process.env.TQA_STACK_SUITE !== "key-use-connector",
  "Requires the owned key-use connector fixture",
);

const OUTCOME =
  "The key could not be used because its use could not be recorded. Try again in a moment; if it keeps happening, tell an administrator.";
const STATUS =
  "Failed: The key could not be used because its use could not be recorded";
/** Words a person must never see for the gateway's own refusal. */
const RAW = ["provider said", "503", "key_use_unrecorded"];

type Event = { type: string; reason?: string };

async function events(page: Page, task: string) {
  const response = await page.request.get(
    `/api/v1/conversations/${task}/events`,
  );
  expect(response.ok()).toBe(true);
  return (
    (await response.json()) as { items: { event?: Event }[] }
  ).items.flatMap((frame) => (frame.event ? [frame.event] : []));
}

test("WEB-E2E-029 a connector call whose key use can't be recorded fails with plain words", async ({
  page,
}) => {
  test.setTimeout(budget(1, 270_000));
  const task = process.env.TQA_CONNECTOR_CONVERSATION ?? "";
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/login");
  await page
    .getByLabel("Email", { exact: true })
    .fill(process.env.TQA_KEY_USE_EMAIL ?? "");
  await page
    .getByLabel("Password", { exact: true })
    .fill(process.env.TQA_STACK_PASSWORD ?? "");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByLabel("Message", { exact: true })).toBeFocused();
  await page.goto(`/c/${task}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("File the crash in the tracker, please.");
  await message.press("Enter");
  // The first reply is held while the driver breaks the table; the call then fails, and the next model call too.
  await expect(page.locator("p.activity-next").first()).toContainText(OUTCOME, {
    timeout: afterStart(90_000),
  });
  await expect(page.getByLabel("Run status")).toHaveText(STATUS, {
    timeout: afterStart(150_000),
  });
  await axe(page);
  // What a person can read: the code may sit only in the card's collapsed Result, which isn't shown.
  const content = await page.locator("body").innerText();
  const stored = await events(page, task);
  const result = {
    reason: stored.find((event) => event.type === "run.failed")?.reason,
    toolEvents: stored
      .filter((event) => event.type.startsWith("tool."))
      .map((event) => JSON.stringify(event)),
    rawShown: RAW.filter((raw) => content.includes(raw)),
    // Where they show, so a failure names the line a person reads.
    rawLines: content
      .split("\n")
      .filter((line) => RAW.some((raw) => line.includes(raw)))
      .map((line) => line.trim().slice(0, 300)),
  };
  const file = process.env.TQA_STACK_RESULT;
  if (file) await writeFile(file, JSON.stringify(result));
});
