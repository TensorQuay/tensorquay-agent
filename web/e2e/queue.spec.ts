import { mkdir } from "node:fs/promises";
import { networkInterfaces } from "node:os";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openChoices } from "./choices";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

const address = Object.values(networkInterfaces())
  .flat()
  .find((entry) => entry?.family === "IPv4" && !entry.internal)?.address;
if (!address)
  throw new Error("The queue contract needs a non-loopback interface.");
test.use({ baseURL: `http://${address}:3100` });

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `queue-${name}.png`) });
}

/** Only the fake front exposes this clock-free scheduler control. */
async function advance(page: Page, conversation: string) {
  const response = await page.request.post(
    `/api/v1/conversations/${conversation}/_fixture/admission`,
  );
  expect(response.status()).toBe(200);
}

async function waitForSlot(page: Page, id: string) {
  const bar = page.getByRole("region", { name: "Task header" });
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Wait for a free slot");
  const accepted = page.waitForResponse((response) =>
    response.url().endsWith(`/conversations/${id}/messages`),
  );
  await input.press("Enter");
  const response = await accepted;
  expect(response.status()).toBe(202);
  expect(response.headers()["x-tqa-execution-status"]).toBe("queued");
  await expect(bar.getByRole("status")).toHaveText(
    "Waiting for a free slot · 2 ahead",
  );
  await expect(input).toHaveAttribute("placeholder", "Reply…");
  // WEB-U-161 (a screen review before 17.0): the waiting is said once, in the header; the composer says what to do.
  await expect(page.getByText("You can reply once it starts.")).toHaveCount(1);
  await expect(page.getByText(/Waiting for a free slot/)).toHaveCount(1);
  // WEB-U-161: the duplicated words it replaced are gone: the bubble's "Queued" line and the composer's sentence.
  // Exact copy, not /queued/i: an agent's own answer may use the word (the fixture's does).
  await expect(page.getByText("Queued", { exact: true })).toHaveCount(0);
  await expect(page.getByText("Your message is queued.")).toHaveCount(0);
  await expect(input).toBeDisabled();
  await expect(page.getByText("Sending…", { exact: true })).toHaveCount(0);
  await openChoices(page);
  await expect(page.getByRole("button", { name: /^Model:/ })).toBeDisabled();
  expect(await page.evaluate(() => isSecureContext)).toBe(false);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
}

for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-018 queued, waking and sleeping ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (["warning", "error"].includes(message.type()))
          errors.push(message.text());
      });
      await page.setViewportSize({ width, height: 1000 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      const path = await createConversation(page);
      const id = path.slice("/c/".length);
      await page.goto(path);
      const bar = page.getByRole("region", { name: "Task header" });
      const input = page.getByRole("textbox", { name: "Message" });
      await input.fill("Review the workspace");
      await input.press("Enter");
      await expect(bar.getByRole("status")).toHaveText("Done");
      await waitForSlot(page, id);
      await capture(page, `${width}-${colorScheme}-waiting`);
      await advance(page, id);
      await expect(bar.getByRole("status")).toHaveText(
        "Waiting for a free slot · 1 ahead",
      );
      await page.reload();
      await expect(bar.getByRole("status")).toHaveText(
        "Waiting for a free slot · 1 ahead",
      );
      await expect(page.getByText(/Send a message to begin/)).toHaveCount(0);
      await advance(page, id);
      await expect(bar.getByRole("status")).toHaveText(
        "Waiting for a free slot · next",
      );
      await capture(page, `${width}-${colorScheme}-next`);
      await advance(page, id);
      await expect(bar.getByRole("status")).toHaveText("Starting…");
      await expect(input).toBeDisabled();
      await capture(page, `${width}-${colorScheme}-starting`);
      await advance(page, id);
      await expect(bar.getByRole("status")).toHaveText("Working…");
      await expect(bar.getByRole("status")).toHaveText("Done");
      // A sleeping environment isn't news in the bar.
      await expect(bar.locator(".environment-status")).toHaveCount(0);
      await expect(input).toBeEnabled();
      await expect(
        page.getByRole("article", { name: "Your message" }),
      ).toHaveCount(2);
      await capture(page, `${width}-${colorScheme}-asleep`);
      await page.reload();
      await expect(bar.getByRole("status")).toHaveText("Done");
      await expect(page.getByText("Queued", { exact: true })).toHaveCount(0);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      expect(errors).toEqual([]);
    });

// c15's WEB-E2E-018 on the real stack: a fresh task whose first message waits has no frames and a stored run status
// of "idle" until it's admitted, and a reload read that as "Ready" (WEB-U-249's stored status hid the snapshot).
for (const [width, colorScheme] of [
  [1024, "light"],
  [1440, "dark"],
] as const)
  test(`WEB-E2E-018 a fresh task's first message waits across a reload ${width} ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1000 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    const path = await createConversation(page);
    const id = path.slice("/c/".length);
    await page.goto(path);
    const bar = page.getByRole("region", { name: "Task header" });
    await waitForSlot(page, id);
    // As the gateway lists it: no run yet.
    const stored = await page.request.get(`/api/v1/conversations/${id}`);
    const { run_status } = (await stored.json()) as { run_status: string };
    expect(run_status).toBe("idle");
    await page.reload();
    await expect(bar.getByRole("status")).toHaveText(
      "Waiting for a free slot · 2 ahead",
    );
    await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
    await expect(page.getByText(/Send a message to begin/)).toHaveCount(0);
    for (let stage = 0; stage < 3; stage += 1) await advance(page, id);
    await page.reload();
    await expect(bar.getByRole("status")).toHaveText("Starting…");
    await advance(page, id);
    await expect(bar.getByRole("status")).toHaveText("Done");
    await capture(page, `${width}-${colorScheme}-fresh-done`);
  });

// A screen review before 17.0 (GW-I-173): a message waiting for a free slot shows with Cancel, also after a reload and
// in another tab; cancelling it ends the waiting everywhere, and one the gateway had already claimed says so.
for (const [width, colorScheme] of [
  [1024, "light"],
  [1440, "dark"],
] as const)
  test(`WEB-U-167 cancel a message waiting for a slot ${width} ${colorScheme}`, async ({
    page,
    context,
  }) => {
    await page.setViewportSize({ width, height: 1000 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    const path = await createConversation(page);
    const id = path.slice("/c/".length);
    await page.goto(path);
    const bar = page.getByRole("region", { name: "Task header" });
    const input = page.getByRole("textbox", { name: "Message" });
    await input.fill("Review the workspace");
    await input.press("Enter");
    await expect(bar.getByRole("status")).toHaveText("Done");
    await waitForSlot(page, id);
    const mine = page.getByRole("article", { name: "Your message" });
    const waiting = mine.filter({ hasText: "Wait for a free slot" });
    await expect(waiting).toHaveCount(1);
    await expect(
      waiting.getByRole("button", { name: "Cancel this message" }),
    ).toBeVisible();
    await capture(page, `${width}-${colorScheme}-cancel`);

    // A reload keeps it, and its Cancel: the list is the gateway's, not the tab's.
    await page.reload();
    await expect(bar.getByRole("status")).toHaveText(
      "Waiting for a free slot · 2 ahead",
    );
    await expect(waiting).toHaveCount(1);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

    // Cancelled in another tab: both stop waiting, and the box is open again.
    const other = await context.newPage();
    await other.setViewportSize({ width, height: 1000 });
    await other.goto(path);
    await other
      .getByRole("article", { name: "Your message" })
      .filter({ hasText: "Wait for a free slot" })
      .getByRole("button", { name: "Cancel this message" })
      .click();
    for (const tab of [other, page]) {
      await expect(
        tab
          .getByRole("article", { name: "Your message" })
          .filter({ hasText: "Wait for a free slot" }),
      ).toHaveCount(0);
      await expect(tab.getByText(/Waiting for a free slot/)).toHaveCount(0);
      await expect(tab.getByText("Sending…", { exact: true })).toHaveCount(0);
      await expect(tab.getByRole("textbox", { name: "Message" })).toBeEnabled();
    }
    await other.close();
    await expect(mine).toHaveCount(1);
    await capture(page, `${width}-${colorScheme}-cancelled`);

    // The race: the slot came and the gateway claimed the message just before Cancel.
    await waitForSlot(page, id);
    const claimed = await page.request.post(
      `/api/v1/conversations/${id}/_fixture/claim`,
    );
    expect(claimed.status()).toBe(200);
    await waiting.getByRole("button", { name: "Cancel this message" }).click();
    await expect(
      page.getByRole("status").filter({ hasText: "It had already started." }),
    ).toBeVisible();
    await expect(waiting).toHaveCount(1);
    for (let stage = 0; stage < 4; stage += 1) await advance(page, id);
    await expect(bar.getByRole("status")).toHaveText("Done");
    await expect(mine).toHaveCount(2);
  });
