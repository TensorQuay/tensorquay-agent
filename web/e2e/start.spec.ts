import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openCard } from "./cards";
import { expectModel } from "./choices";
import { chooseModel } from "./model";
import { signIn } from "./session";
import { probeStreams, reportStreams } from "./stream-probe";
import { expect, test, type Page } from "./test";

async function capture(page: Page, name: string) {
  if (!process.env.TQA_WEB_EVIDENCE_DIR) return;
  await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
  await page.screenshot({
    path: join(process.env.TQA_WEB_EVIDENCE_DIR, `${name}.png`),
  });
}

// TEST-PLAN §19a ("Sending…" after a first message): each test records its event streams, attached on a failure.
test.beforeEach(async ({ page }) => probeStreams(page));
test.afterEach(async ({ page }, info) => reportStreams(page, info));

for (const width of [1024, 1440]) {
  for (const theme of ["light", "dark"] as const) {
    test(`WEB-U-016/017 start, composer and activity states ${width} ${theme}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1000 });
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      const writes: string[] = [];
      page.on("request", (request) => {
        if (request.method() === "POST")
          writes.push(new URL(request.url()).pathname);
      });
      await page.goto("/");
      const input = page.getByRole("textbox", { name: "Message", exact: true });
      await expect(input).toBeFocused();
      // A person with no tasks yet: the keyboard hint is the placeholder's second line (WEB-U-431).
      await expect(input).toHaveAttribute(
        "placeholder",
        "Describe the task…\nEnter to send · Shift+Enter for a new line",
      );
      // What the task will use is in the composer: its model and environment pickers (WEB-U-102).
      await expect(
        page.getByRole("button", { name: "Model: Local model" }),
      ).toBeVisible();
      expect(writes).toEqual([]);
      await input.fill("An unsent draft");
      await page.getByRole("link", { name: "New task", exact: true }).click();
      await expect(input).toHaveValue("");
      await expect(input).toBeFocused();
      expect(writes).toEqual([]);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await capture(page, `start-${width}-${theme}`);
      await chooseModel(page, "second");
      await input.fill("Summarize the workspace.");
      await capture(page, `typing-${width}-${theme}`);
      let release: (() => void) | undefined;
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      await page.route("**/messages", async (route) => {
        await held;
        await route.continue();
      });
      await page.getByRole("button", { name: "Send", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Sending", exact: true }),
      ).toBeDisabled();
      await expect(
        page.getByRole("article", { name: "Your message" }),
      ).toContainText("Summarize the workspace.");
      await capture(page, `sending-${width}-${theme}`);
      release?.();
      await expect(page).toHaveURL(/\/c\//);
      await page.unroute("**/messages");
      await expect(page.getByLabel("Run status")).toHaveText("Done");
      const group = page.getByRole("button", { name: /Worked for/ });
      await expect(group).toHaveAttribute("aria-expanded", "false");
      await expectModel(page, "Second model");
      await expect(
        page.locator('.conversation-link[aria-current="page"]'),
        // The row's tooltip: the full title, then its time (WEB-U-070).
      ).toHaveAttribute("title", /^Summarize the workspace\. · /);
      await capture(page, `finished-${width}-${theme}`);
      await group.press("Enter");
      // The latest card opens with its group (finding T2).
      await openCard(
        page.getByRole("button", { name: "Ran a command: ls", exact: true }),
      );
      await expect(page.getByLabel("Result")).toContainText("Cargo.toml");
      await page.reload();
      await expect(group).toHaveAttribute("aria-expanded", "false");
      await expectModel(page, "Second model");
      await page.getByRole("link", { name: "New task", exact: true }).click();
      await expect(input).toBeFocused();
      await input.fill("Hold activity");
      await input.press("Enter");
      await expect(
        page.getByRole("button", { name: /Running a command: ls/ }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Stop", exact: true }),
      ).toBeEnabled();
      await capture(page, `live-${width}-${theme}`);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
      await page
        .getByRole("button", { name: "Stop", exact: true })
        .press("Enter");
      await expect(page.getByLabel("Run status")).toHaveText("Stopped");
      await expect(
        page.getByRole("button", { name: /Running a command: ls/ }),
      ).toHaveCount(0);
      await page.getByRole("link", { name: "New task", exact: true }).click();
      await expect(page).toHaveURL("/");
      await expect(input).toBeFocused();
      await input.fill("Fail the command");
      await input.press("Enter");
      await expect(page.getByLabel("Run status")).toHaveText(/^Failed: /);
      await expect(
        page.getByRole("button", { name: /Worked for/ }),
      ).toHaveAttribute("aria-expanded", "true");
      await expect(page.getByText(/Send the message again/)).toBeVisible();
      const outcome = page.locator(".turn-outcome");
      await expect(outcome).toHaveText(
        "The task's environment couldn't be reached. Send the message again in a moment.",
      );
      // The outcome is said once: a failed turn that changed no file has no summary line at all (WEB-U-373), so
      // nothing below it can repeat it.
      await expect(page.locator(".turn-summary")).toHaveCount(0);
      expect(
        await outcome.evaluate(
          (element) => getComputedStyle(element).fontFamily,
        ),
      ).toBe(
        await page
          .locator("body")
          .evaluate((element) => getComputedStyle(element).fontFamily),
      );
      expect(
        await outcome
          .locator('[data-tone="danger"]')
          .evaluate((element) => getComputedStyle(element).color),
      ).toBe(
        await page
          .getByLabel("Run status")
          .evaluate((element) => getComputedStyle(element).color),
      );

      // WEB-U-157 (the 13f screens): Thinking is marked as something to open, not a bare word.
      await expect(
        page.locator("details.reasoning > summary svg").first(),
      ).toBeVisible();
      // WEB-U-155 (the 13f screens, #4): the status is one word, and the title keeps its room.
      await expect(page.getByLabel("Run status")).toHaveAttribute(
        "data-short",
        "Failed",
      );
      expect(
        await page
          .getByRole("heading", { level: 1 })
          .evaluate((title) => title.scrollWidth <= title.clientWidth),
      ).toBe(true);
      // WEB-U-180: the failed command says so once, by its "Failed" mark.
      const failedLine = page.getByRole("button", {
        name: /^Ran a command: ls/,
      });
      await expect(failedLine).toContainText("Failed");
      await expect(failedLine).not.toContainText(/didn.t succeed/);
      expect(
        (await failedLine.textContent())?.match(/Failed/g) ?? [],
      ).toHaveLength(1);
      await capture(page, `failed-${width}-${theme}`);
      expect(
        await page.evaluate(
          () =>
            document.documentElement.scrollWidth <= innerWidth &&
            document.documentElement.scrollHeight <= innerHeight,
        ),
      ).toBe(true);
    });
  }
}

// The review's carry-overs for New task (UI-GUIDELINES §0/§5b): on a phone the title sits beside the menu button,
// not floated right; the box's placeholder doesn't repeat the heading; the suggestions read as body text, not bold.
test("WEB-U-017 New task at 390: title beside the menu, a placeholder of its own, light suggestions", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await page.goto("/");
  const menu = await page
    .getByRole("button", { name: "Open navigation" })
    .boundingBox();
  const title = await page
    .getByRole("region", { name: "Task header" })
    .getByRole("heading", { name: "New task" })
    .boundingBox();
  expect(menu && title).toBeTruthy();
  expect((title?.x ?? 0) - ((menu?.x ?? 0) + (menu?.width ?? 0))).toBeLessThan(
    24,
  );
  // Not the heading again; a person with no tasks yet also reads the keyboard hint under it (WEB-U-431; a narrow
  // window with a mouse is not a touch screen).
  await expect(
    page.getByRole("textbox", { name: "Message", exact: true }),
  ).toHaveAttribute(
    "placeholder",
    "Describe the task…\nEnter to send · Shift+Enter for a new line",
  );
  const weight = await page
    .getByRole("button", {
      name: "Draft a one-page plan for launching a newsletter",
    })
    .evaluate((button) => getComputedStyle(button).fontWeight);
  expect(Number(weight)).toBeLessThan(600);
});

// WEB-U-180 at a phone's width: the failed command's line says it once, by its mark.
test("WEB-U-180 a failed command says so once at 390", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "Message" });
  await input.fill("Fail the command");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Failed/);
  const line = page.getByRole("button", { name: /^Ran a command: ls/ });
  await expect(line).toContainText("Failed");
  await expect(line).not.toContainText(/didn.t succeed/);
});
