import { readFileSync } from "node:fs";

import { knownFailure } from "../src/transcript/failure-keys";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-U-016 end to end, after the owner's test on a Mac showed a raw "budget.output_tokens." in red: every failure a
// run can end with reaches the page as what happened and what to do, in plain words, and its recorded reason never
// shows. The fake model fails on "Fail with <code> [reason]" (e2e/front/stream.mjs).
// Read, not imported: Playwright loads specs as ES modules, which refuse a JSON import without an attribute.
const en = JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
  messages: Record<string, string>;
};
const protocolSchema = JSON.parse(
  readFileSync("src/protocol/generated/protocol.schema.json", "utf8"),
) as {
  definitions: { ErrorCode: { oneOf: { const: string }[] } };
};
const words = (key: string) => en.messages[key];
/**
 * What the page says about the failure (the turn's outcome and the run status) never holds the recorded reason. The
 * person's own message does: it is how the fake front is told what to fail with.
 */
async function neverSays(page: Page, reason: string) {
  for (const said of [
    page.locator(".turn-outcome"),
    page.getByLabel("Run status"),
  ])
    await expect(said).not.toContainText(reason);
}
const cases: [code: string, reason: string][] = [
  ...protocolSchema.definitions.ErrorCode.oneOf.map(
    (entry): [string, string] => [entry.const, `diag.${entry.const}`],
  ),
  ["reply_cut", "the model stopped at its output limit (finish: length)"],
  ["budget", "budget.output_tokens"],
  ["budget", "budget.wall_clock"],
  ["budget", "the run's step budget was reached"],
  ["budget", "context window exceeded"],
];

for (const [code, reason] of cases)
  test(`WEB-U-016 a run failing with ${code} (${reason}) reads in plain words`, async ({
    page,
  }) => {
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByLabel("Message", { exact: true });
    await input.fill(`Fail with ${code} ${reason}`);
    await input.press("Enter");
    const known = knownFailure(code, reason);
    if (!known) throw new Error(`No words for ${code}`);
    await expect(page.getByLabel("Run status")).toHaveText(
      `Failed: ${words(known.reason)}`,
    );
    // Only provider_error keeps its classification as the muted code line (UX review 0927b, item 4); a decision
    // engine's stop adds the engine's own reason, quoted, after the sentence (WEB-U-328).
    const engine =
      code === "decision_stop" ? ` ${words("fail.engineSaid")} ${reason}` : "";
    await expect(page.locator(".turn-outcome")).toHaveText(
      `${words(known.reason)}.${engine} ${words(known.next)}${code === "provider_error" ? ` Code: ${reason}` : ""}`,
    );
    if (code !== "provider_error" && code !== "decision_stop")
      await neverSays(page, reason);
  });

test("WEB-U-016 the owner's failure: a spent output budget, in plain words", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Fail with budget budget.output_tokens");
  await input.press("Enter");
  await expect(page.locator(".turn-outcome")).toHaveText(
    "The reply was cut off at the task's length limit. Ask for a shorter answer, or send a message to continue.",
  );
  await expect(page.locator(".turn-outcome [data-tone=danger]")).toHaveText(
    "The reply was cut off at the task's length limit.",
  );
  await neverSays(page, "budget.output_tokens");
});

test("WEB-U-016 a code this page doesn't know: a plain sentence first, the code after, muted", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Fail with mystery_code internal.mystery");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(
    "Failed: The run stopped",
  );
  const outcome = page.locator(".turn-outcome");
  await expect(outcome).toHaveText(
    "The run stopped. Send a new message to retry after checking the failed activity. Code: mystery_code",
  );
  await expect(outcome.locator(".turn-code")).toHaveClass(/muted/);
  await neverSays(page, "internal.mystery");
});

test("WEB-U-016 a server refusing too long an input: its words, then what helps", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Send too long an input");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(
    "Failed: The conversation is too long for the model",
  );
  await expect(page.locator(".turn-outcome")).toHaveText(
    "The provider said: the request exceeds the available context size, try increasing it Start a new task, or ask for a short summary first. Code: provider_error 400",
  );
});

test("WEB-U-016 a command the shell couldn't find says so", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill("Run a missing command");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  // Failed activity may already be open (WEB-U-016); open only what's closed.
  for (const name of [/Worked for/, /command: ls/]) {
    const line = page.getByRole("button", { name }).first();
    if ((await line.getAttribute("aria-expanded")) === "false")
      await line.click();
  }
  await expect(
    page.getByText("Didn't succeed (code 127: command not found)"),
  ).toBeVisible();
});
