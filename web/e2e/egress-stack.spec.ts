import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import type { ErrorTarget } from "../src/protocol/generated/protocol";

import { parseStackFrame } from "./stack-frames";
import { afterStart } from "./stack-start";
import { bootstrap } from "./ux/flow";

test.skip(
  !process.env.TQA_STACK_URL,
  "Requires the owned stack with a one-entry egress allow-list",
);

const blocked =
  "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  // Evidence shows rendered content, never a loading skeleton.
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `egress-real-${name}.png`) });
}

/** Records the live refusal frames exactly as the browser received them. */
async function observeDenials(page: Page) {
  const session = await page.context().newCDPSession(page);
  await session.send("Network.enable");
  const denials: (ErrorTarget | null)[] = [];
  const invalid: string[] = [];
  session.on("Network.eventSourceMessageReceived", ({ data }) => {
    try {
      const frame = parseStackFrame(data);
      if (frame.type === "error" && frame.code === "egress_denied") {
        // The schema allows an absent or null seq on an unsequenced frame; the gateway sends null.
        expect(frame.seq ?? null, "a refusal is live-only").toBeNull();
        if (denials.length < 20) denials.push(frame.target ?? null);
      }
    } catch (error) {
      if (invalid.length < 20) invalid.push(String(error));
    }
  });
  return { denials, invalid };
}

// SEC-001 (web side): the real policy proxy refuses a fetch; the denied fetch line says so once, live and on replay.
test("SEC-001 real blocked network access", async ({ page }) => {
  const logged: string[] = [];
  page.on("console", (message) => {
    if (["error", "warning"].includes(message.type()))
      logged.push(message.text());
  });
  page.on("pageerror", (error) => logged.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  const stream = await observeDenials(page);
  await bootstrap(page);
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Fetch the example page.");
  await input.press("Enter");
  // ADR-0035: nothing asks first; the proxy's allow-list decides.
  await expect(page.getByLabel("Run status")).toHaveText("Done", {
    timeout: afterStart(30_000),
  });
  const conversationId = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  const main = page.getByRole("main");
  await expect(
    main.getByText(blocked, { exact: true }).filter({ visible: true }),
  ).toHaveCount(0);
  await expect(main.getByText(/something went wrong/i)).toHaveCount(0);
  await page.getByRole("button", { name: /Worked for/ }).click();
  await expect(
    main.getByText(blocked, { exact: true }).filter({ visible: true }),
  ).toHaveCount(1);
  expect(stream.invalid).toEqual([]);
  expect(stream.denials.length).toBeGreaterThan(0);
  for (const target of stream.denials)
    expect(target).toEqual({ host: "example.invalid", port: 443 });
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    await capture(page, `1440-${colorScheme}-live`);
  }
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await page.getByRole("button", { name: /Worked for/ }).click();
  await expect(
    main.getByText(blocked, { exact: true }).filter({ visible: true }),
  ).toHaveCount(1);
  await capture(page, "1440-dark-replay");
  expect(logged).toEqual([]);
  if (process.env.TQA_STACK_RESULT)
    await writeFile(
      process.env.TQA_STACK_RESULT,
      JSON.stringify({
        denials: stream.denials.map((target) => ({
          conversationId,
          ...target,
        })),
      }),
      { mode: 0o600 },
    );
});
