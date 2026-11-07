import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test } from "@playwright/test";

import { axe } from "./settings-page";
import { afterStart, budget, scaled } from "./stack-start";

// WEB-E2E-029 (a push with a person's host key, ADR-0037): the driver (crates/tqa-sandbox/tests/egress/web_key_use.py
// --push) holds the task's plain-git push until the read-audit table is unwritable. The refusal is said only live, by
// the host_key.refused frame (never journaled, and the stored key uses hold no row for an unrecorded use), so the page
// must be open when it happens. A bounded marker handshake, as E2E-AGENTS-020's: the spec opens the task and writes
// `page-ready`; the driver then releases the push; the spec waits for the step's line. After a reload no raw code shows.
// The driver checks the rest: the outcome `unrecorded` and nothing reaching the origin.
test.skip(
  process.env.TQA_STACK_SUITE !== "key-use-push",
  "Requires the owned key-use push fixture",
);

/** The bearer key and host the driver adds, as the step names them (activity.keyUnrecorded). */
const KEY = process.env.TQA_PUSH_KEY_NAME ?? "";
const HOST = process.env.TQA_PUSH_HOST ?? "";
const LINE = `${KEY} wasn't sent to ${HOST}: its use couldn't be recorded. Try again in a moment.`;
/** The driver's failure marker: when present, this spec fails with what the driver wrote. */
const FAILED = "push-failed";

function markers() {
  const directory = process.env.TQA_KEY_USE_MARKERS ?? "";
  if (!directory) throw new Error("The driver must supply TQA_KEY_USE_MARKERS");
  return directory;
}

/** Throws with the driver's words if it wrote its failure marker. */
async function driverFailed() {
  const failure = await readFile(join(markers(), FAILED), "utf8").catch(
    () => null,
  );
  if (failure !== null)
    throw new Error(`The driver wrote ${FAILED}: ${failure}`);
}

test("WEB-E2E-029 a push whose key use can't be recorded says so as it happens", async ({
  page,
}) => {
  // One start (the run's, if it is still starting), then the push and the reload.
  test.setTimeout(budget(1, 480_000));
  const task = process.env.TQA_PUSH_CONVERSATION ?? "";
  expect(
    KEY && HOST && task,
    "the driver names the key (TQA_PUSH_KEY_NAME), the host (TQA_PUSH_HOST) and the task (TQA_PUSH_CONVERSATION)",
  ).toBeTruthy();
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

  await test.step("the page is live while the push is held, then says the key wasn't sent", async () => {
    await driverFailed();
    await expect(page.getByLabel("Run status")).toHaveText(/^Working/, {
      timeout: afterStart(60_000),
    });
    await writeFile(
      join(markers(), "page-ready"),
      "the task is open and working",
    );
    // The driver releases the push only after `page-ready`; the line follows its refusal.
    const line = page.getByText(LINE, { exact: true });
    const bound = scaled(300_000);
    await expect
      .poll(
        async () => {
          await driverFailed();
          return line.isVisible();
        },
        {
          timeout: bound,
          message: `the step's line within ${bound / 1000} s of page-ready: ${LINE}`,
        },
      )
      .toBe(true);
    await expect(line).toHaveCount(1);
  });

  await test.step("after the run, a reload shows no raw code", async () => {
    await expect(page.getByLabel("Run status")).not.toHaveText(/^Working/, {
      timeout: scaled(60_000),
    });
    await page.reload();
    await expect(page.getByLabel("Run status")).toBeVisible();
    await expect(
      page.getByText(/key_use_unrecorded/).filter({ visible: true }),
    ).toHaveCount(0);
    await axe(page);
  });

  const result = process.env.TQA_STACK_RESULT;
  if (result) await writeFile(result, JSON.stringify({ line: LINE }));
});
